import { LOCAL_HOST_KEY, hostOf, type PaneRef } from '@floti/core/types'

/**
 * What each host is told is on screen (`session:focus`): the ids of its
 * sessions in front of a pane, from the tabs in front of every pane. Each host
 * judges by it whom an exit is worth a notification for, and which window a
 * session's desktop tools open in; only this side knows it.
 *
 * Every host told something before is told again, with nothing when nothing of
 * its is on screen now, so no host goes on believing a tab is shown that has
 * gone. This PC's host is always told.
 */
export function focusByHost(visible: readonly PaneRef[], toldBefore: ReadonlySet<string>): Map<string, number[]> {
  const byHost = new Map<string, number[]>([[LOCAL_HOST_KEY, []]])
  for (const host of toldBefore) byHost.set(host, [])
  for (const ref of visible) {
    if (ref.kind !== 'session') continue
    const host = hostOf(ref)
    byHost.set(host, [...(byHost.get(host) ?? []), ref.id])
  }
  return byHost
}

/** The hosts to tell again next time: this PC's, and every one with a session on screen now. */
export function stillTold(byHost: ReadonlyMap<string, readonly number[]>): Set<string> {
  return new Set([...byHost].filter(([host, ids]) => host === LOCAL_HOST_KEY || ids.length > 0).map(([host]) => host))
}
