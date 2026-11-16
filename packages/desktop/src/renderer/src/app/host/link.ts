import {
  HostRefusedError,
  connectHost,
  type ByteTransport,
  type HostConnection
} from '../../../../shared/host/client'
import type { Capability } from '../../../../shared/host/protocol'
import { RECONNECT_FIRST_MS, nextReconnectWait } from '../../../../shared/host/reconnect'
import { unreachable } from '../../../../shared/host/sentences'
import type { HelmBridge, HostLinkOpened, RefusedVersions } from '../../../../shared/ipc'

/**
 * This page's own protocol connection to a host.
 *
 * Main starts a `floti bridge` for the page and pipes its frames over
 * `hostLink:*` without reading them (`main/host/page-link.ts`); the page
 * speaks the protocol itself, through the client both builds share. The
 * device key and the Noise session stay in the bridge.
 *
 * Every caller joins one connect attempt, so a read asked before the first
 * connect has settled waits for it rather than answering that there is
 * nothing there. This PC's host's link holds that attempt while the host
 * cannot be used - it refuses this Floti, or cannot be reached - and it
 * resolves on the next connect that succeeds: main answers the page's open
 * only once its own link has a host (`HostLink.usable`), and only main's
 * retry schedule or the window's Try again (`reconnectNow`) connects again,
 * so waiting reads start no connect, and no host, of their own. The window
 * says why once, from main's steps; a read that has something of its own to
 * show meanwhile asks `connectionIfUsable` instead. A host on another
 * computer's link rejects with the sentence saying why.
 *
 * When the host's side ends the connection, main says whether it ended the
 * host on purpose (Stop host, or a quit) and whether Helm starts a new one,
 * and the link tells its `onEnded` listeners. After an end on purpose the
 * next caller connects again. After any other end - a host that crashed or
 * was killed - the link connects again by itself, waiting as main's link
 * does (`shared/host/reconnect.ts`), until it connects; its `onConnected`
 * listeners then run again. A connection the page closed itself is no end
 * of the host. After a Restart host to update the link connects again at
 * once: main holds the connect until the new host is up.
 *
 * A host that refuses this Helm's `hello` names both protocol versions and
 * the side to update. The link tells its `onRefused` listeners, so the
 * window can say which side is older: this PC's link from main's word that
 * its host cannot be used (`usability`), or from a page bridge refused after
 * main connected, which it tries again after the same waits. A host on
 * another computer's link rejects with that sentence, as a
 * `HostRefusedError` carrying the versions.
 */

/** What the app window says it can do: draw terminals, show files and draw plugins' pages. */
export const WINDOW_CAPABILITIES: readonly Capability[] = ['terminal', 'files', 'pluginPages']

/** How the host's side of a connection ended. */
export interface HostEnd {
  /** Main's link ended the host on purpose: a stop or a quit, which is no crash. */
  planned: boolean
  /** Helm starts a new host in place of this one, rather than waiting for it to answer again. */
  restarts: boolean
  /** The user's Restart host to update ended it; the page connects to the new host at once. */
  restarting?: boolean
}

export interface PageLink {
  /**
   * The connection, connecting first when there is none. This PC's host's
   * link waits through a host that cannot be used; any other rejects with the
   * sentence to show.
   */
  connection(): Promise<HostConnection>
  /**
   * The connection as `connection()` gives it, but rejecting with main's
   * sentence, rather than waiting, while this PC's host cannot be used: for a
   * read that has something of its own to answer meanwhile (Floti's own
   * settings). The same as `connection()` for a link with no `usability`.
   */
  connectionIfUsable(): Promise<HostConnection>
  /**
   * Called on every connect, before `connection()` resolves, so a channel
   * subscribed here is subscribed before anything is asked of the host.
   */
  onConnected(listener: (connection: HostConnection) => void): void
  /** Called each time the host's side ends the connection, saying how. */
  onEnded(listener: (end: HostEnd) => void): void
  /** Called each time the host refused this Helm's `hello`, with the versions it named. */
  onRefused?(listener: (refused: RefusedVersions) => void): void
  /**
   * The user's Reconnect now, or Try again: a link waiting to connect again
   * drops the wait and connects at once, and the next wait starts from the
   * first again. This PC's host's link also asks main's link to drop its own
   * wait (`hostLink:reconnect`), which is what connects while that host
   * cannot be used. A link that is connected or connecting does nothing
   * else, and one that does not connect by itself (`retries: false`, a remote
   * host's) has no wait to drop: it is told when to connect, once main's own
   * link is back. Nothing from before the drop is sent again.
   */
  reconnectNow(): void
}

/** Why this PC's host cannot be used, as main last said it: the sentence, and the versions a refusal named. */
export interface HostUnusable {
  sentence: string
  refused?: RefusedVersions
}

/** Main's word on whether this PC's host can be used (`hostLink:step`), as the page link reads it. */
export interface HostUsability {
  /** Why it cannot be used now, or null while it can be, or before anything failed. */
  get(): HostUnusable | null
  /** Calls `listener` on every change; returns the unsubscribe. */
  subscribe(listener: () => void): () => void
}

export interface PageLinkDeps {
  /** The preload's bridge: only `hostLink:*` is used. */
  bridge: Pick<HelmBridge, 'invoke' | 'send' | 'on'>
  capabilities: readonly Capability[]
  log?: (line: string) => void
  /** The clock the waits before connecting again run on; the page's own by default. */
  setTimeout?: (run: () => void, ms: number) => unknown
  clearTimeout?: (timer: unknown) => void
  /** A host on another computer, by its id; this PC's host when absent. */
  host?: string
  /**
   * Whether the link connects again by itself after the host's side ended
   * without a stop: true by default. A remote host's page link is told when
   * to connect instead, once main's own link to that host is back.
   */
  retries?: boolean
  /** This PC's host only: main's word on whether it can be used, which `connectionIfUsable` and `onRefused` hear. */
  usability?: HostUsability
}

/** A page bridge that failed after main had opened it: this PC's host's link waits and tries again. */
class BridgeFailedError extends Error {}

/** What a read that does not wait is answered with while this PC's host cannot be used. */
const unusableError = (why: HostUnusable): Error =>
  why.refused === undefined ? new Error(why.sentence) : new HostRefusedError(why.sentence, why.refused)

/**
 * A byte transport over this page's `hostLink:*` channels, for one
 * connection. `hostEnded` hears how the host's side ended, before the
 * connection does.
 */
export function relayTransport(
  bridge: PageLinkDeps['bridge'],
  connection: number,
  hostEnded?: (end: HostEnd) => void
): ByteTransport {
  let ended = false
  let unsubscribe: Array<() => void> = []
  const stop = (): void => {
    ended = true
    for (const off of unsubscribe) off()
    unsubscribe = []
  }
  return {
    start(receiver) {
      unsubscribe = [
        bridge.on('hostLink:data', (payload) => {
          if (payload.connection === connection && !ended) receiver.data(payload.bytes)
        }),
        bridge.on('hostLink:ended', (payload) => {
          if (payload.connection !== connection || ended) return
          stop()
          hostEnded?.({
            planned: payload.planned,
            restarts: payload.restarts,
            ...(payload.restarting === true ? { restarting: true } : {})
          })
          receiver.end(payload.why)
        })
      ]
    },
    write(bytes) {
      if (!ended) bridge.send('hostLink:send', { connection, bytes })
    },
    end() {
      if (ended) return
      stop()
      bridge.send('hostLink:close', { connection })
    }
  }
}

export function createPageLink(deps: PageLinkDeps): PageLink {
  const listeners: Array<(connection: HostConnection) => void> = []
  const endListeners: Array<(end: HostEnd) => void> = []
  const refusedListeners: Array<(refused: RefusedVersions) => void> = []
  const setTimer = deps.setTimeout ?? ((run: () => void, ms: number): unknown => setTimeout(run, ms))
  const clearTimer = deps.clearTimeout ?? ((timer: unknown): void => clearTimeout(timer as ReturnType<typeof setTimeout>))
  let attempt: Promise<HostConnection> | null = null
  let retryTimer: unknown = null
  let retryWait = RECONNECT_FIRST_MS
  /** This PC's host's link: its connect waits through a host that cannot be used, on main's one connect. */
  const holds = deps.retries !== false && deps.host === undefined
  /** Ends a held connect's wait before it tries again, at once (Reconnect now). */
  let wakeHold: (() => void) | null = null

  /** A held connect's wait before it tries again: the link's next wait, or until Reconnect now. */
  const holdWait = (): Promise<void> =>
    new Promise((resolve) => {
      const wait = retryWait
      retryWait = nextReconnectWait(retryWait)
      const wake = (): void => {
        clearTimer(timer)
        wakeHold = null
        resolve()
      }
      const timer = setTimer(wake, wait)
      wakeHold = wake
    })

  /** Connects again after a wait, and again after a longer one each time that fails. */
  const scheduleRetry = (): void => {
    if (retryTimer !== null) return
    const wait = retryWait
    retryWait = nextReconnectWait(retryWait)
    retryTimer = setTimer(() => {
      retryTimer = null
      link.connection().then(
        () => undefined,
        () => scheduleRetry()
      )
    }, wait)
  }

  const hostEnded = (end: HostEnd): void => {
    for (const listener of endListeners) {
      try {
        listener(end)
      } catch (error) {
        deps.log?.(`host: hearing the connection end failed (${(error as Error).message})`)
      }
    }
    if (end.restarting === true) {
      // Main's link answers once the new host is up, or says why it is not.
      link.connection().then(
        () => undefined,
        () => scheduleRetry()
      )
    } else if (!end.planned && deps.retries !== false) scheduleRetry()
  }

  const refusedWith = (refused: RefusedVersions): void => {
    for (const listener of refusedListeners) {
      try {
        listener(refused)
      } catch (error) {
        deps.log?.(`host: hearing a refusal failed (${(error as Error).message})`)
      }
    }
  }

  const open = (): Promise<HostLinkOpened> =>
    deps.bridge.invoke('hostLink:open', deps.host === undefined ? undefined : { host: deps.host })

  /** Connects once; `ended` hears how the host's side ended this connection, if it does. */
  const connect = async (ended: (end: HostEnd) => void): Promise<HostConnection> => {
    const opened = await open()
    if ('unreachable' in opened) {
      if (opened.refused === undefined) throw new Error(opened.unreachable)
      refusedWith(opened.refused)
      throw new HostRefusedError(opened.unreachable, opened.refused)
    }
    let connection: HostConnection
    try {
      connection = await connectHost({
        transport: relayTransport(deps.bridge, opened.connection, ended),
        appVersion: opened.appVersion,
        ...(opened.deviceName !== undefined ? { deviceName: opened.deviceName } : {}),
        capabilities: deps.capabilities,
        ...(deps.log !== undefined ? { log: deps.log } : {})
      })
    } catch (error) {
      // The bridge's exit said why, as main's link would; a refused hello
      // names both versions, and the side to update.
      if (error instanceof HostRefusedError) {
        if (error.refused !== null) refusedWith(error.refused)
        throw new HostRefusedError(unreachable(error.message), error.refused)
      }
      throw new BridgeFailedError((error as Error).message, { cause: error })
    }
    for (const listener of listeners) {
      try {
        listener(connection)
      } catch (error) {
        deps.log?.(`host: setting up the connection failed (${(error as Error).message})`)
      }
    }
    return connection
  }

  // Main's word that this PC's host refused this Floti, told once per refusal.
  let toldUnusable: HostUnusable | null = null
  deps.usability?.subscribe(() => {
    const why = deps.usability?.get() ?? null
    if (why === toldUnusable) return
    toldUnusable = why
    if (why?.refused !== undefined) refusedWith(why.refused)
  })

  const link: PageLink = {
    connection() {
      if (attempt !== null) return attempt
      let made: Promise<HostConnection> | null = null
      const once = (): Promise<HostConnection> => {
        let end: HostEnd | null = null
        return connect((how) => {
          end = how
        }).then((connection) => {
          retryWait = RECONNECT_FIRST_MS
          // Once it ends, the next caller connects again; a host that went
          // without a stop is connected to again without waiting for one.
          void connection.ended.then(() => {
            if (attempt === made) attempt = null
            if (end !== null) hostEnded(end)
          })
          return connection
        })
      }
      /**
       * Main answers this PC's host's open once its own link has a host, so
       * the attempt waits there. A bridge refused or failed after that (the
       * host went in between) is tried again after the link's waits, and the
       * attempt holds; only main's own answer that nothing comes (Floti is
       * quitting, the page went) ends it.
       */
      const held = async (): Promise<HostConnection> => {
        for (;;) {
          try {
            return await once()
          } catch (error) {
            if (!(error instanceof HostRefusedError || error instanceof BridgeFailedError)) throw error
            deps.log?.(`host: this PC’s host could not be used yet (${error.message}); waiting for it`)
            await holdWait()
          }
        }
      }
      const tried: Promise<HostConnection> = (holds ? held() : once()).catch((error: unknown) => {
        if (attempt === tried) attempt = null
        throw error
      })
      made = tried
      attempt = tried
      return tried
    },

    connectionIfUsable() {
      const usability = deps.usability
      if (usability === undefined) return link.connection()
      const why = usability.get()
      if (why !== null) return Promise.reject(unusableError(why))
      return new Promise<HostConnection>((resolve, reject) => {
        const stop = usability.subscribe(() => {
          const now = usability.get()
          if (now === null) return
          stop()
          reject(unusableError(now))
        })
        link.connection().then(
          (connection) => {
            stop()
            resolve(connection)
          },
          (error: unknown) => {
            stop()
            reject(error as Error)
          }
        )
      })
    },

    onConnected(listener) {
      listeners.push(listener)
    },

    onEnded(listener) {
      endListeners.push(listener)
    },

    onRefused(listener) {
      refusedListeners.push(listener)
    },

    reconnectNow() {
      if (holds) {
        // While this PC's host cannot be used, main's link is what connects.
        deps.bridge.invoke('hostLink:reconnect').catch((error: unknown) => {
          deps.log?.(`host: Try again did not reach Floti (${(error as Error).message})`)
        })
        if (wakeHold !== null) {
          retryWait = RECONNECT_FIRST_MS
          wakeHold()
        }
      }
      if (attempt !== null || retryTimer === null) return
      clearTimer(retryTimer)
      retryTimer = null
      retryWait = RECONNECT_FIRST_MS
      link.connection().then(
        () => undefined,
        () => scheduleRetry()
      )
    }
  }
  return link
}
