import { RECONNECT_FIRST_MS, nextReconnectWait } from '../../shared/host/reconnect'
import type { HostConnection } from './client'
import type { LocalHost } from './launcher'

/**
 * The desktop's one connection to the local host, made when the desktop starts and again
 * whenever it is wanted after the last one failed or ended.
 *
 * A connect writes no settings: the host keeps its own, which every client
 * reads and changes over its own connection.
 *
 * The desktop never stops the host on its own. A deliberate quit tells it what
 * becomes of what this device started (`host:quit`) and lets go; the host
 * decides its own exit. Quitting is bounded: a host that does not answer in
 * time is let go of all the same, and a desktop that crashes or is killed tells
 * it nothing. The user can stop it from Settings (`stopHost`), which ends
 * every session; the next request then starts a fresh one. Or, when this
 * desktop carries a newer floti, they can restart it for the update
 * (`restartHost`): the host leaves its sessions running in its keeper and
 * exits, or, for a host on protocol 1.x from before the restart frames, its
 * process alone is ended as a crash would end it, leaving its keeper; then
 * the link starts the floti this desktop carries and connects to it, and that
 * host takes the keeper back. Neither end is a crash to the link, so neither
 * is retried or marked lost.
 *
 * A connection that ends without a stop or a quit is a host that crashed or
 * was killed. The link then connects again by itself, the first try after
 * `RECONNECT_FIRST_MS` and each wait after a failed try twice the last, up to
 * two minutes, until it connects or the desktop quits. It starts a new host only in
 * place of one a desktop started (this desktop, or another whose host it found
 * and which says so in `app:info`); any other host is waited for, never
 * replaced. Every `onConnected` listener runs again on the new connection.
 *
 * This PC's host's link also tries again after a connect that failed
 * (`retryFailedConnects`): a host that refuses this Floti or cannot be
 * reached is tried on the same waits, and nothing else connects while it
 * waits. `usable()` is what a page's connection waits on: every page open
 * joins the one connect in flight or waits for the next that succeeds, so a
 * window whose reads wait through a host that cannot be used starts no
 * connect, and no host, of its own. Main's own readers asking `connection()`
 * meanwhile are answered at once with why the last connect failed. Only the
 * retry schedule and Reconnect now (the window's Try again) connect again.
 *
 * A host on another computer is reached by a link of its own
 * (`remote.ts`), made with the same function: its `start` never starts a
 * host, and it keeps trying from its first
 * connect (`keepConnected`). A link that merely drops sends nothing; only a
 * quit sends `host:quit`, and forgetting a host (`release`) sends nothing at
 * all.
 */

/** How long a quit waits for the host's answer, and then for the bridge to exit. */
export const HOST_STOP_DEADLINE_MS = 10_000

export interface HostLink {
  /** The connection, connecting first when there is none. Rejects with the sentence to show. */
  connection(): Promise<HostConnection>
  /**
   * The connection once the host can be used, for a page's connection to
   * wait on: the one there is, the connect in flight, or, while the link
   * waits to try again, the next connect that succeeds, without starting
   * one. It connects only when nothing is in flight or waiting (the first
   * connect, or after a stop). Rejects only once Floti quits. A link without
   * `retryFailedConnects` answers as `connection()` does.
   */
  usable(): Promise<HostConnection>
  /** The connection there is now, or null. */
  current(): HostConnection | null
  /** Settles once the attempt in flight has, without starting one. */
  settled(): Promise<void>
  /**
   * Called on every connect, before `connection()` resolves, so nothing is
   * asked of the host before what it needs to answer is subscribed.
   */
  onConnected(listener: (connection: HostConnection) => void | Promise<void>): void
  /**
   * The desktop is quitting on purpose: tells the host that the plain terminals this
   * device opened end, and with `end` the sessions it started too, then lets
   * go of the connection and its bridge; resolves once the bridge has exited,
   * or the deadline passed. The host is never stopped here. Nothing connects
   * after it.
   */
  quit(options: { end: boolean }): Promise<void>
  /**
   * Lets go of the host without telling it anything: no `host:quit`, so
   * nothing there ends, not even a plain terminal. Stops trying to connect;
   * nothing connects after it. Resolves once the bridge has exited, or the
   * deadline passed. For a host this device forgets.
   */
  release(): Promise<void>
  /**
   * Connects now, and keeps trying with the same backoff after every failed
   * try as after a dropped connection, until it connects or the link is quit
   * or released.
   */
  keepConnected(): void
  /**
   * The user's Reconnect now: while the link is waiting to try again, the
   * wait is dropped and it tries at once, and the next wait starts from the
   * first again. A link that is connected, already trying, quit or released
   * does nothing. It asks for nothing but the connection: no request from
   * before the drop is sent again.
   */
  reconnectNow(): void
  /**
   * The connection there is now stopped answering (a remote host's link
   * after the computer woke, `remote.ts`): lets go of it without a word to
   * the host and ends its bridge's processes at once (`LocalHost.kill`, where
   * the bridge has it), and the link connects again as after any connection
   * that ended without a stop, so `endedOnPurpose` stays false. Does nothing
   * with no connection, or once quit or released.
   */
  drop(): void
  /**
   * The user asked, and agreed to the count: stops the host (`host:stop`
   * with the plain-stop reason), so every session in it ends and is recorded
   * as ended, then lets go of the connection and its bridge. Resolves once
   * the connection has ended, or the deadline passed. The next `connection()`
   * connects again as after any ended connection, which starts a fresh host.
   */
  stopHost(): Promise<void>
  /**
   * The user asked, and agreed to what it ends: restarts the host for an
   * update (`deps.restart`), then connects to the new host this desktop starts,
   * as after any ended connection. Every `onConnected` listener runs again
   * on the new connection. While it runs, `connection()` waits for it rather
   * than reaching the host that is exiting. Rejects with the sentence to show,
   * and changes nothing, when the host could not be asked.
   */
  restartHost(): Promise<void>
  /** Whether a restart for an update is under way: the connection's end is planned, and a new host follows at once. */
  restarting(): boolean
  /**
   * Whether the connection there is, or the last one, is ending or ended
   * because the desktop stopped or restarted the host or is quitting: an end that is
   * no crash.
   */
  endedOnPurpose(): boolean
  /** Whether the desktop starts a new host if the one it reaches now, or last reached, is gone. */
  restartsHost(): boolean
}

export interface HostLinkDeps {
  /**
   * Connects to the local host. With `startHost` false it only connects,
   * and a host that does not answer is a failure rather than one to start.
   */
  start: (options: { startHost: boolean }) => Promise<LocalHost>
  /** What the log calls the host: the local host when absent. */
  name?: string
  /**
   * This PC's host: a connect that fails is tried again after the same
   * waits as a connection that ended, and while the link waits,
   * `connection()` answers with why the last one failed rather than
   * connecting, and `usable()` waits for the next that succeeds.
   */
  retryFailedConnects?: boolean
  /** Hears why each connect attempt failed, before any retry is scheduled. */
  onAttemptFailed?: (error: Error) => void
  /**
   * Restarts the host for an update and resolves once it has gone
   * (`restartLocalHost`, which ends a protocol-1.x host's process alone);
   * absent for a host this link does not restart.
   */
  restart?: () => Promise<unknown>
  log: (line: string) => void
  setTimeout: (run: () => void, ms: number) => unknown
  clearTimeout: (timer: unknown) => void
}

export function createHostLink(deps: HostLinkDeps): HostLink {
  const name = deps.name ?? 'the local host'
  const listeners: Array<(connection: HostConnection) => void | Promise<void>> = []
  let local: LocalHost | null = null
  let attempt: Promise<HostConnection> | null = null
  let stopping: Promise<void> | null = null
  /** Set while the user's Stop host is under way, until the next connect. */
  let stoppingHost = false
  /** Set while the user's Restart host to update is under way, until the new host is connected or the restart failed. */
  let restartingHost = false
  /** Settles once a restart's old host has gone; `connection()` waits on it. */
  let restartGate: Promise<void> | null = null
  /** Whether the host reached now, or last, was started by a desktop. */
  let desktopHost = true
  /** Whether the next connect may start a host when none answers. */
  let mayStart = true
  let retryTimer: unknown = null
  let retryWait = RECONNECT_FIRST_MS
  /** Why the last connect failed, until one succeeds or the user's restart connects afresh (`retryFailedConnects`). */
  let lastFailure: Error | null = null
  /** What every `usable()` asked during a wait shares: settled by the next connect that succeeds, or by a quit. */
  let nextUsable: { promise: Promise<HostConnection>; resolve: (connection: HostConnection) => void; reject: (error: Error) => void } | null = null

  const waitForUsable = (): Promise<HostConnection> => {
    if (nextUsable === null) {
      let resolve: (connection: HostConnection) => void = () => undefined
      let reject: (error: Error) => void = () => undefined
      const promise = new Promise<HostConnection>((done, fail) => {
        resolve = done
        reject = fail
      })
      nextUsable = { promise, resolve, reject }
    }
    return nextUsable.promise
  }

  const usableNow = (connection: HostConnection): void => {
    lastFailure = null
    const waiting = nextUsable
    nextUsable = null
    waiting?.resolve(connection)
  }

  /** Tries to connect again after a wait, until it connects or the desktop quits. */
  const scheduleRetry = (): void => {
    if (stopping !== null || retryTimer !== null) return
    const wait = retryWait
    retryWait = nextReconnectWait(retryWait)
    retryTimer = deps.setTimeout(() => {
      retryTimer = null
      if (stopping !== null || local !== null) return
      link.connection().then(
        () => undefined,
        (error: unknown) => {
          deps.log(`host: ${name} could not be reached again (${(error as Error).message})`)
          scheduleRetry()
        }
      )
    }, wait)
  }

  /** `work`, or nothing once the deadline has passed. */
  const bounded = async (work: Promise<unknown>): Promise<void> => {
    let timer: unknown = null
    const deadline = new Promise<void>((resolve) => {
      timer = deps.setTimeout(resolve, HOST_STOP_DEADLINE_MS)
    })
    await Promise.race([
      work.then(
        () => undefined,
        () => undefined
      ),
      deadline
    ])
    deps.clearTimeout(timer)
  }

  const connect = async (): Promise<HostConnection> => {
    const started = await deps.start({ startHost: mayStart })
    if (stopping !== null) {
      await started.close()
      throw new Error('Floti is quitting.')
    }
    local = started
    stoppingHost = false
    retryWait = RECONNECT_FIRST_MS
    desktopHost = started.startedHost
    const { connection } = started
    void connection.ended.then(() => {
      if (local !== started) return
      local = null
      attempt = null
      if (stopping !== null || stoppingHost || restartingHost) {
        // Stopped or restarted on purpose: the next request starts a fresh host.
        mayStart = true
        return
      }
      mayStart = desktopHost
      deps.log(
        `host: the connection to ${name} ended without a stop; ${
          mayStart ? 'starting a new host' : 'connecting to it again once it answers'
        }`
      )
      scheduleRetry()
    })
    if (!started.startedHost) {
      // A host found running is replaced only if a desktop started it.
      void connection.request('app:info', null).then(
        (info) => {
          if (local === started) desktopHost = info?.desktop === true
        },
        () => undefined
      )
    }
    for (const listener of listeners) {
      try {
        await listener(connection)
      } catch (error) {
        deps.log(`host: setting up the connection failed (${(error as Error).message})`)
      }
    }
    return connection
  }

  const link: HostLink = {
    connection() {
      if (stopping !== null) return Promise.reject(new Error('Floti is quitting.'))
      if (restartGate !== null) return restartGate.then(() => link.connection())
      // Waiting to try again after a failed connect: only the wait connects.
      if (attempt === null && lastFailure !== null && retryTimer !== null) return Promise.reject(lastFailure)
      attempt ??= connect().then(
        (connection) => {
          usableNow(connection)
          return connection
        },
        (error: unknown) => {
          attempt = null
          deps.onAttemptFailed?.(error as Error)
          if (deps.retryFailedConnects === true) {
            lastFailure = error as Error
            scheduleRetry()
          }
          throw error
        }
      )
      return attempt
    },

    usable() {
      if (deps.retryFailedConnects !== true) return link.connection()
      if (stopping !== null) return Promise.reject(new Error('Floti is quitting.'))
      if (restartGate !== null) return restartGate.then(() => link.usable())
      // A failed attempt has scheduled its retry by now, so asking again waits for it.
      if (attempt !== null) return attempt.catch(() => link.usable())
      if (retryTimer !== null) return waitForUsable()
      return link.connection().catch(() => link.usable())
    },

    current: () => local?.connection ?? null,

    settled: () =>
      attempt === null
        ? Promise.resolve()
        : attempt.then(
            () => undefined,
            () => undefined
          ),

    onConnected(listener) {
      listeners.push(listener)
    },

    async stopHost() {
      const connection = await link.connection()
      const host = local
      stoppingHost = true
      try {
        await connection.request('host:stop', { reason: 'stop' })
      } catch (error) {
        // Not stopped, unless the connection went with the asking.
        if (local !== null && local.connection === connection) stoppingHost = false
        throw error
      }
      await bounded(connection.ended)
      // The bridge goes with the host it reached; this makes sure of it.
      if (host !== null && host.connection === connection) await bounded(host.close())
    },

    async restartHost() {
      const { restart } = deps
      if (restart === undefined) throw new Error(`Floti does not restart ${name}.`)
      if (stopping !== null) throw new Error('Floti is quitting.')
      if (restartGate !== null) return restartGate
      let opened: () => void = () => undefined
      restartGate = new Promise<void>((resolve) => {
        opened = resolve
      })
      restartingHost = true
      try {
        await restart()
        // The old host closed the connection; this makes sure the bridge went with it.
        const before = local
        if (before !== null) await bounded(before.close())
        local = null
        attempt = null
        mayStart = true
        // The user's restart connects afresh, whatever the wait says.
        lastFailure = null
      } catch (error) {
        restartingHost = false
        // A host that went all the same is connected to again, as after a crash.
        if (local === null) scheduleRetry()
        throw error
      } finally {
        restartGate = null
        opened()
      }
      try {
        await link.connection()
      } catch (error) {
        // The new host did not start or answer: tried again, as after a crash.
        scheduleRetry()
        throw error
      } finally {
        restartingHost = false
      }
    },

    restarting: () => restartingHost,

    endedOnPurpose: () => stopping !== null || stoppingHost || restartingHost,

    restartsHost: () => (local === null ? mayStart : desktopHost),

    quit({ end }) {
      return letGo((host) => bounded(host.connection.request('host:quit', { end })))
    },

    release() {
      return letGo(() => Promise.resolve())
    },

    keepConnected() {
      link.connection().then(
        () => undefined,
        (error: unknown) => {
          deps.log(`host: ${name} could not be reached (${(error as Error).message})`)
          scheduleRetry()
        }
      )
    },

    reconnectNow() {
      if (stopping !== null || local !== null || attempt !== null) return
      if (retryTimer !== null) {
        deps.clearTimeout(retryTimer)
        retryTimer = null
      }
      retryWait = RECONNECT_FIRST_MS
      link.keepConnected()
    },

    drop() {
      const host = local
      if (stopping !== null || host === null) return
      deps.log(`host: letting go of the connection to ${name}, which stopped answering`)
      // Closing ends the connection first, so the end `connect` waits on runs
      // at once and tries again, whatever the bridge's transport takes to exit.
      host.close().catch((error: unknown) => {
        deps.log(`host: the bridge to ${name} did not close (${(error as Error).message})`)
      })
      // A dead transport would wait out its socket for minutes; its processes end now.
      host.kill?.()
    }
  }

  /** Stops trying, says `last` to the host there is, then lets go of it. */
  function letGo(last: (host: LocalHost) => Promise<void>): Promise<void> {
    if (retryTimer !== null) {
      deps.clearTimeout(retryTimer)
      retryTimer = null
    }
    const waiting = nextUsable
    nextUsable = null
    waiting?.reject(new Error('Floti is quitting.'))
    stopping ??= (async () => {
      if (attempt !== null) await bounded(attempt)
      const host = local
      if (host === null) return
      local = null
      await last(host)
      await bounded(host.close())
    })()
    return stopping
  }
  return link
}
