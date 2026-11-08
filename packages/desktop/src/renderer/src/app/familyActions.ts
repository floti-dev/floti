import { bridgeFor } from './bridge'

/**
 * Stop on a child's card: the window asks first, as it does before any running
 * session ends (`session:close` asks when it is not forced), and once the user
 * agrees the host ends the session. The window keeps the session where it was
 * ended so its card can say how (`keep`); its ended row follows on
 * `session:exit` and the card shrinks to a header reading "Stopped by you".
 *
 * Settles true when the child was ended and false when the user declined. A
 * request that cannot reach the host leaves the card running and settles
 * false: the window already says the host is gone, and Stop is there to be
 * pressed again.
 */
export function stopChild(host: string, id: number): Promise<boolean> {
  return bridgeFor(host)
    .invoke('session:close', { id, keep: true })
    .then((answer) => answer.closed)
    .catch(() => false)
}

/**
 * Remove on a kept child's row, and Remove stopped children on its parent's:
 * the host stops keeping each of `ids`, and they leave the Sessions tree and
 * the family view on every device. It asks nothing first: the children stay in
 * History and their parents can still resume them. A request that cannot reach
 * the host leaves the rows where they are, to be removed again.
 */
export function removeKept(host: string, ids: readonly number[]): Promise<void> {
  if (ids.length === 0) return Promise.resolve()
  return bridgeFor(host)
    .invoke('session:remove', { ids: [...ids] })
    .then(
      () => undefined,
      () => undefined
    )
}
