import { eq, inArray } from 'drizzle-orm'
import type { BrowserHistoryEntry } from '../browser/suggest'
import type { Store } from './db'
import { browserHistory } from './schema'

/**
 * The browser's history: the `browser_history` table, and the copy of it main
 * keeps in memory so the address bar can be answered on every keystroke
 * without reading the disk.
 *
 * Every change is written through at once. A visit is one upsert, and a 501st
 * page drops the oldest by last visit in the same step, so the table never
 * holds more than `BROWSER_HISTORY_MAX` rows and the copy never disagrees
 * with it.
 */

/** How many pages the history keeps, by last visit. */
export const BROWSER_HISTORY_MAX = 500

/**
 * The longest address and title kept. A page can put megabytes in its
 * address or title; a suggestion needs neither in full, and one row should not
 * cost the history more than a page's worth.
 */
export const BROWSER_HISTORY_URL_MAX = 4096
export const BROWSER_HISTORY_TITLE_MAX = 500

export interface BrowserHistory {
  /** Every page, in no particular order. Ranking sorts. */
  entries(): readonly BrowserHistoryEntry[]
  /**
   * A navigation to `url` committed. Counts a visit and makes it the newest;
   * a non-empty `title` replaces the one kept. Only http and https addresses
   * are kept, and nothing over `BROWSER_HISTORY_URL_MAX`.
   */
  visit(url: string, title?: string | null): void
  /** The page at `url` said its title. Ignored for a page the history does not hold. */
  retitle(url: string, title: string): void
  /** Forget one page. False when it was not there. */
  remove(url: string): boolean
  /** Forget every page. */
  clear(): void
}

export interface BrowserHistoryOptions {
  /** The clock a visit is stamped with. `Date.now` unless a test steps it. */
  now?: () => number
  max?: number
}

const KEEPS = /^https?:\/\//i

const clipTitle = (title: string): string => title.trim().slice(0, BROWSER_HISTORY_TITLE_MAX)

/** Reads the table once and answers from memory from then on. */
export function openBrowserHistory(store: Store, options: BrowserHistoryOptions = {}): BrowserHistory {
  const now = options.now ?? Date.now
  const max = options.max ?? BROWSER_HISTORY_MAX
  const pages = new Map<string, BrowserHistoryEntry>()
  for (const row of store.db.select().from(browserHistory).all()) pages.set(row.url, { ...row })

  /** Drop the oldest pages past `max`, in memory and on disk. */
  const trim = (): void => {
    if (pages.size <= max) return
    const dropped = [...pages.values()]
      .sort((a, b) => b.lastVisit - a.lastVisit || a.url.localeCompare(b.url))
      .slice(max)
      .map((entry) => entry.url)
    for (const url of dropped) pages.delete(url)
    store.db.delete(browserHistory).where(inArray(browserHistory.url, dropped)).run()
  }
  trim()

  return {
    entries: () => [...pages.values()],

    visit(url, title) {
      if (!KEEPS.test(url) || url.length > BROWSER_HISTORY_URL_MAX) return
      const said = title === undefined || title === null ? '' : clipTitle(title)
      const before = pages.get(url)
      const entry: BrowserHistoryEntry = {
        url,
        title: said !== '' ? said : (before?.title ?? ''),
        visits: (before?.visits ?? 0) + 1,
        lastVisit: now()
      }
      pages.set(url, entry)
      store.db
        .insert(browserHistory)
        .values(entry)
        .onConflictDoUpdate({
          target: browserHistory.url,
          set: { title: entry.title, visits: entry.visits, lastVisit: entry.lastVisit }
        })
        .run()
      trim()
    },

    retitle(url, title) {
      const entry = pages.get(url)
      const said = clipTitle(title)
      if (entry === undefined || said === '' || said === entry.title) return
      entry.title = said
      store.db.update(browserHistory).set({ title: said }).where(eq(browserHistory.url, url)).run()
    },

    remove(url) {
      if (!pages.delete(url)) return false
      store.db.delete(browserHistory).where(eq(browserHistory.url, url)).run()
      return true
    },

    clear() {
      pages.clear()
      store.db.delete(browserHistory).run()
    }
  }
}
