import type { ChildReport } from '../types'
import { sameReport } from './familyReport'

/**
 * What the host's listing keeps current on a session a window already holds:
 * what a child last told its parent, what the session's agent says it is
 * doing, and whether a stopped child is kept under its parent. Each comes and
 * goes with the session's turns or a person's Remove, so each listing is the
 * word on them, while the rest of a held row stays as the window has it.
 */
export interface ListedFacts {
  lastReport?: ChildReport
  title?: string
  kept?: boolean
}

/**
 * `known` with the report, title and kept `fresh` carries: the very same row
 * when they agree, and a copy that takes them (or goes without one) when they
 * do not. Nothing else of `known` is touched, because a window may hold facts
 * about the row that the host's listing has not caught up with, such as a name
 * it has just changed.
 */
export function withListedOf<T extends ListedFacts>(known: T, fresh: ListedFacts): T {
  if (
    sameReport(known.lastReport, fresh.lastReport) &&
    known.title === fresh.title &&
    (known.kept === true) === (fresh.kept === true)
  ) {
    return known
  }
  const next = { ...known }
  if (fresh.lastReport === undefined) delete next.lastReport
  else next.lastReport = fresh.lastReport
  if (fresh.title === undefined) delete next.title
  else next.title = fresh.title
  if (fresh.kept === true) next.kept = true
  else delete next.kept
  return next
}

/**
 * `known` as a listing that does not name it leaves it: a child the window
 * held as kept that the host lists no longer is not kept any more (a person
 * removed it, it was resumed as a new row, or its parent ended). The very same
 * row when it was not kept.
 */
export function unlistedOf<T extends ListedFacts>(known: T): T {
  if (known.kept !== true) return known
  const next = { ...known }
  delete next.kept
  return next
}

/**
 * The rows `current` holds, each taking the report, title and kept its row in
 * `listed` carries, or null when none changed on any row, so the sessions a
 * window keeps are not replaced (and everything drawn from them redrawn) by a
 * listing that said nothing new. A row `current` holds that `listed` does not
 * have keeps everything but kept (`unlistedOf`).
 */
export function withListed<T extends { id: number } & ListedFacts>(
  current: readonly T[],
  listed: readonly ({ id: number } & ListedFacts)[]
): T[] | null {
  const byId = new Map(listed.map((row) => [row.id, row]))
  let changed = false
  const next = current.map((row) => {
    const fresh = byId.get(row.id)
    const taken = fresh === undefined ? unlistedOf(row) : withListedOf(row, fresh)
    if (taken !== row) changed = true
    return taken
  })
  return changed ? next : null
}
