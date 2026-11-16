import type { HostConnection, HostSubscription } from '../../../../shared/host/client'
import {
  historyPageFromHost,
  historyQueryForHost,
  historySessionFromHost,
  historySummaryFromHost
} from '../../../../shared/host/records/history'
import type { EventPayload } from '../../../../shared/ipc'
import type { HostAdapter } from './router'

/**
 * History (the Ctrl+H palette) and the transcript archive while sessions run
 * in the local host: the host indexes Claude Code's history and archives its
 * transcripts into its own database, and the window reads them over its own
 * connection, in ipc.ts's shapes.
 *
 * On every connect it subscribes to the host's `history:changed` and
 * `archive:changed` and hands each state to the window's
 * listeners, converted, so a conversation the host indexes after the view
 * opened reaches it without a refresh.
 *
 * Every read goes through `link.connection()`, which joins the first connect
 * while it is in flight, so a read asked before the window has reached the
 * host gets the host's answer, never an empty one first. When the host cannot
 * be reached, a read rejects with the link's sentence, never an empty list or
 * a zero standing in for "could not look".
 */

/** As much of a page's link as an adapter needs. */
export interface HostReach {
  connection(): Promise<HostConnection>
  /** As `connection()`, but rejecting rather than waiting while this PC's host cannot be used (`PageLink.connectionIfUsable`). */
  connectionIfUsable?(): Promise<HostConnection>
  onConnected(listener: (connection: HostConnection) => void): void
}

type Pushed = 'history:changed' | 'archive:changed'

export function createHistoryAdapter(link: HostReach, log: (line: string) => void = () => undefined): HostAdapter {
  const listeners: { [K in Pushed]: Set<(payload: EventPayload<K>) => void> } = {
    'history:changed': new Set(),
    'archive:changed': new Set()
  }
  let subscriptions: HostSubscription[] = []

  const host = (): Promise<HostConnection> => link.connection()
  const tell = <K extends Pushed>(channel: K, payload: EventPayload<K>): void => {
    for (const listener of listeners[channel]) listener(payload)
  }
  const ended = (channel: string) => (error: Error) => log(`host: ${channel} ended (${error.message})`)

  link.onConnected((connection) => {
    for (const subscription of subscriptions) subscription.unsubscribe()
    const history = (summary: Parameters<typeof historySummaryFromHost>[0]): void =>
      tell('history:changed', historySummaryFromHost(summary))
    const archive = (stats: EventPayload<'archive:changed'>): void => tell('archive:changed', stats)
    subscriptions = [
      connection.subscribe('history:changed', { snapshot: history, event: history, error: ended('history:changed') }),
      connection.subscribe('archive:changed', { snapshot: archive, event: archive, error: ended('archive:changed') })
    ]
  })

  /** A listener on a pushed state; listening is reason enough to connect. */
  const listen =
    <K extends Pushed>(channel: K) =>
    (listener: (payload: EventPayload<K>) => void): (() => void) => {
      listeners[channel].add(listener)
      void host().catch(() => undefined)
      return () => {
        listeners[channel].delete(listener)
      }
    }

  return {
    requests: {
      'history:summary': async () => historySummaryFromHost(await (await host()).request('history:summary', null)),
      'history:sessions': async (query) =>
        historyPageFromHost(await (await host()).request('history:sessions', historyQueryForHost(query ?? {}))),
      'history:prompts': async ({ sessionId }) => (await host()).request('history:prompts', { sessionId }),
      'history:projects': async () => (await host()).request('history:projects', null),
      'history:refresh': async () => historySummaryFromHost(await (await host()).request('history:refresh', null)),
      'history:rename': async ({ sessionId, name }) => {
        const renamed = await (await host()).request('history:rename', { sessionId, name })
        return renamed === null ? null : historySessionFromHost(renamed)
      },
      'archive:conversation': async ({ sessionId }) => (await host()).request('archive:conversation', { sessionId }),
      'archive:stats': async () => (await host()).request('archive:stats', null)
    },
    events: {
      'history:changed': listen('history:changed'),
      'archive:changed': listen('archive:changed')
    }
  }
}
