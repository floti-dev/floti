import { useState } from 'react'

/**
 * When the window saw each host it is trying to reach go out of reach, by id,
 * in ms since the epoch: what Settings says as "its link failed at 14:02". A
 * host back in reach is forgotten, so a later drop is timed afresh. A host out
 * of reach from the window's start is timed from when the window first saw it.
 *
 * Kept by adjusting state during render when the set changes, as React
 * documents for state derived from props: an effect would paint one frame
 * without the time first.
 *
 * @param away the ids of the hosts out of reach now.
 * @param now the clock; `Date.now` outside a test.
 */
export function useAwaySince(away: readonly string[], now: () => number = Date.now): ReadonlyMap<string, number> {
  const key = [...away].sort().join('\n')
  const [held, setHeld] = useState<{ key: string; since: ReadonlyMap<string, number> }>(() => {
    const at = now()
    return { key, since: new Map(away.map((id) => [id, at])) }
  })
  if (held.key !== key) {
    const at = now()
    const since = new Map<string, number>()
    for (const id of away) since.set(id, held.since.get(id) ?? at)
    setHeld({ key, since })
    return since
  }
  return held.since
}

/** A time as the window shows one in a sentence: hours and minutes, in this computer's own way. */
export function clockTime(at: number): string {
  return new Date(at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
}
