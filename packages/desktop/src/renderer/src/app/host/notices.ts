import type { StandingNotice } from '@floti/core/types'
import type { HostConnection, HostSubscription } from '../../../../shared/host/client'
import type * as P from '../../../../shared/host/protocol'
import type { HostReach } from './history'
import { memorySeen, newerThan, newestOf, type NoticesSeen } from './noticesSeen'
import type { HostAdapter } from './router'

/**
 * The notices standing on a host: a child that needs a person, a start a limit
 * refused, an agent child's error. The host keeps them, so the window
 * subscribes on every connect, and the first snapshot is whatever still stands.
 * Dismissing one, and allowing a parent more starts, are the host's: the window
 * asks over its own connection, in ipc.ts's shapes.
 *
 * While you were away is what stood at connect and began after the newest
 * notice this device had shown of the host (`NoticesSeen`): a connection that
 * drops while the laptop sleeps, or a restart, lists nothing it has shown
 * already. Every notice the window is given counts as shown.
 *
 * The latest list is kept and handed to a listener that arrives after it, so a
 * page that mounts once the host has answered is not left without the notices.
 * With no host the requests reject with the link's sentence.
 */

export function noticeFromHost(notice: P.StandingNotice): StandingNotice {
  return {
    id: notice.id,
    kind: notice.kind,
    parentId: notice.parentId,
    parentName: notice.parentName,
    childId: notice.childId ?? null,
    childName: notice.childName ?? null,
    text: notice.text,
    since: notice.since,
    limit: notice.limit ?? null,
    allowMore: notice.allowMore ?? null,
    cause: notice.cause ?? null
  }
}

/** One notice as this window tells it apart from the same host's next one with its id: a child that waits again begins a new notice. */
export const noticeKey = (notice: Pick<StandingNotice, 'id' | 'since'>): string => `${notice.id}\n${notice.since}`

/**
 * What a window holds of one host's notices: those standing now, and those
 * that stood when its connection to the host opened, had not been shown on
 * this device before and still stand (While you were away), which are none
 * once the window closed that list.
 */
export interface NoticesView {
  /** Every notice standing now. */
  notices: readonly StandingNotice[]
  /** The ones standing when the connection opened, newer than any this device had shown, that still stand; empty once closed in this window. */
  away: readonly StandingNotice[]
  /** The `noticeKey` of each notice that stood when the connection opened, kept after it resolves. */
  atConnect: ReadonlySet<string>
}

export interface NoticesStore {
  get(): NoticesView
  /** Called on every change; also asks for the host's list. Returns the stop. */
  subscribe(listener: () => void): () => void
  /** Closes the While you were away list in this window, until the next connect. */
  closeAway(): void
}

export type NoticesAdapter = HostAdapter & { store: NoticesStore }

/**
 * `seen` keeps what this device has shown of the host's notices; without one
 * it is kept in memory, which holds across reconnects but not a restart.
 */
export function createNoticesAdapter(
  link: HostReach,
  log: (line: string) => void = () => undefined,
  seen: NoticesSeen = memorySeen()
): NoticesAdapter {
  const listeners = new Set<(notices: StandingNotice[]) => void>()
  const watchers = new Set<() => void>()
  let subscription: HostSubscription | null = null
  let latest: StandingNotice[] | null = null
  let atConnect: ReadonlySet<string> = new Set()
  /** The newest notice shown before this connection opened; undefined while it is read. */
  let shownBefore: string | null | undefined = undefined
  /** Counts connections, so a mark read for one that has gone is dropped. */
  let connections = 0
  let awayClosed = false
  let view: NoticesView = { notices: [], away: [], atConnect }

  const host = (): Promise<HostConnection> => link.connection()
  const refresh = (): void => {
    const notices = latest ?? []
    const before = shownBefore
    view = {
      notices,
      away:
        awayClosed || before === undefined
          ? []
          : notices.filter((notice) => atConnect.has(noticeKey(notice)) && newerThan(notice.since, before)),
      atConnect
    }
    for (const watcher of [...watchers]) watcher()
  }
  /** What the window was given is shown: once the mark it opened with is known, so that one still lists what is new. */
  const markShown = (): void => {
    if (latest === null || shownBefore === undefined) return
    const newest = newestOf(latest.map((notice) => notice.since))
    if (newest !== null) seen.mark(newest)
  }
  const take = (listed: readonly P.StandingNotice[]): void => {
    const first = latest === null
    latest = listed.map(noticeFromHost)
    if (first) atConnect = new Set(latest.map(noticeKey))
    markShown()
    for (const listener of listeners) listener(latest)
    refresh()
  }
  const opened = (mark: string | null): void => {
    shownBefore = mark
    markShown()
    refresh()
  }

  link.onConnected((connection) => {
    subscription?.unsubscribe()
    latest = null
    atConnect = new Set()
    awayClosed = false
    connections += 1
    const opening = connections
    shownBefore = undefined
    const mark = seen.read()
    if (mark instanceof Promise) {
      mark.then(
        (read) => {
          if (opening === connections) opened(read)
        },
        () => {
          if (opening === connections) opened(null)
        }
      )
    } else {
      shownBefore = mark
    }
    refresh()
    subscription = connection.subscribe('notices:changed', {
      snapshot: take,
      event: take,
      error: (error) => log(`host: notices:changed ended (${error.message})`)
    })
  })

  const store: NoticesStore = {
    get: () => view,
    subscribe: (listener) => {
      watchers.add(listener)
      void host().catch(() => undefined)
      return () => {
        watchers.delete(listener)
      }
    },
    closeAway: () => {
      if (awayClosed) return
      awayClosed = true
      refresh()
    }
  }

  return {
    store,
    requests: {
      'notices:dismiss': async ({ id }): Promise<void> => {
        await (await host()).request('notices:dismiss', { id })
      },
      'notices:allowMore': async ({ parentId }): Promise<{ added: number }> => {
        const allowed = await (await host()).request('notices:allowMore', { parentId })
        return { added: allowed.added }
      }
    },
    events: {
      'notices:changed': (listener) => {
        listeners.add(listener)
        if (latest !== null) listener(latest)
        void host().catch(() => undefined)
        return () => {
          listeners.delete(listener)
        }
      }
    }
  }
}
