/**
 * What the address bar suggests while somebody types, ranked from the
 * browser's own history.
 *
 * Pure: a decision about strings and a list, with nothing read from disk and
 * nothing sent anywhere. Main ranks over its in-memory copy of the history
 * (`store/browserHistory.ts`) and answers the bar with the result, so typing
 * reaches no search engine and no other machine (SECURITY.md).
 */
import { browserSearchName, resolveBrowserAddress, type BrowserSearch } from './reach'

/** How many rows the bar offers, the Search row included. */
export const BROWSER_SUGGESTIONS_MAX = 8

/** One page in the browser's history. */
export interface BrowserHistoryEntry {
  url: string
  /** The page's title, or '' until it has said one. */
  title: string
  visits: number
  /** Milliseconds since the epoch. */
  lastVisit: number
}

/** A row the bar offers: search for what was typed, or go to a page from history. */
export type BrowserSuggestion =
  | { kind: 'search'; text: string; engine: string }
  | { kind: 'page'; url: string; title: string }

/**
 * The bar's suggestions for one text.
 *
 * `text` is the input they answer, so a bar that has moved on since asking can
 * tell a stale answer from a current one. `completion` is the inline
 * completion: the top page, when the text starts its address, and the `rest`
 * of that address to show selected after what was typed.
 */
export interface BrowserSuggestions {
  text: string
  rows: BrowserSuggestion[]
  completion: { url: string; rest: string } | null
}

const SCHEME = /^([a-z][a-z0-9+.-]*):\/\//i

/** An address without its scheme or `www.`, and nothing else changed. */
const bareAddress = (text: string): string => text.trim().replace(SCHEME, '').replace(/^www\./i, '')

/**
 * An address the way a person types it: no scheme, no `www.`, and no lone
 * trailing slash on a bare host. `https://www.example.com/` is `example.com`;
 * `http://localhost:3000/app/` keeps its path as it is.
 */
export function typedAddress(url: string): string {
  const bare = bareAddress(url)
  return /^[^/?#]+\/$/.test(bare) ? bare.slice(0, -1) : bare
}

/** The scheme typed in front of an address, lower-cased, or null. */
const schemeOf = (text: string): string | null => SCHEME.exec(text.trim())?.[1]?.toLowerCase() ?? null

/** The host without `www.`, or '' for what does not parse. */
function hostOf(url: string): string {
  try {
    return new URL(url).hostname.toLowerCase().replace(/^www\./, '')
  } catch {
    return ''
  }
}

/** Whether `needle` occurs in `hay` where a word starts: at the start, or after anything but a letter or digit. */
function atWordStart(hay: string, needle: string): boolean {
  for (let at = hay.indexOf(needle); at !== -1; at = hay.indexOf(needle, at + 1)) {
    if (at === 0 || !/[\p{L}\p{N}]/u.test(hay.charAt(at - 1))) return true
  }
  return false
}

/**
 * How well an entry matches: 0 for the address exactly as typed, 1 for an
 * address that starts with the text, 2 for a word start in the host or title,
 * 3 for the text anywhere in the address or title; null for no match.
 */
type Tier = 0 | 1 | 2 | 3

function tierOf(entry: BrowserHistoryEntry, query: string, scheme: string | null): Tier | null {
  const address = typedAddress(entry.url).toLowerCase()
  const title = entry.title.toLowerCase()
  const schemeFits = scheme === null || entry.url.toLowerCase().startsWith(`${scheme}://`)
  if (schemeFits && address === query) return 0
  if (schemeFits && address.startsWith(query)) return 1
  if (atWordStart(hostOf(entry.url), query) || atWordStart(title, query)) return 2
  if (entry.url.toLowerCase().includes(query) || title.includes(query)) return 3
  return null
}

/** More visits first, then the more recent. */
const byUse = (a: BrowserHistoryEntry, b: BrowserHistoryEntry): number =>
  b.visits - a.visits || b.lastVisit - a.lastVisit || a.url.localeCompare(b.url)

/**
 * The history entries matching `text`, best first: an address that starts
 * with it (after the scheme and `www.`), the one that is exactly it first,
 * then a word in the host or title that starts with it, then the text
 * anywhere in the address or title. Ties go to the page visited more, then the
 * one visited last.
 *
 * Empty text matches nothing; `rankBrowserHistory` is for typed text.
 */
export function rankBrowserHistory(
  entries: readonly BrowserHistoryEntry[],
  text: string
): Array<{ entry: BrowserHistoryEntry; tier: Tier }> {
  const query = typedAddress(text).toLowerCase()
  if (query === '') return []
  const scheme = schemeOf(text)
  const ranked: Array<{ entry: BrowserHistoryEntry; tier: Tier }> = []
  for (const entry of entries) {
    const tier = tierOf(entry, query, scheme)
    if (tier !== null) ranked.push({ entry, tier })
  }
  return ranked.sort((a, b) => a.tier - b.tier || byUse(a.entry, b.entry))
}

/**
 * The rest of `entry`'s address after `text`, when `text` starts it as typed
 * (after the scheme and `www.` on both sides); null otherwise. '' when the text
 * is the whole address: nothing to show, but Enter still goes to the page's
 * own address, scheme and all.
 */
export function inlineCompletion(text: string, entry: BrowserHistoryEntry): string | null {
  if (text.trim() === '' || /\s/.test(text)) return null
  // What was typed exactly as typed, slash and all: the rest is shown right
  // after it, so the two have to line up character for character.
  const typed = bareAddress(text).toLowerCase()
  if (typed === '') return null
  const scheme = schemeOf(text)
  if (scheme !== null && !entry.url.toLowerCase().startsWith(`${scheme}://`)) return null
  // The address as a person types it, or with its lone slash when that is
  // what was typed (`example.com/` for `https://example.com/`).
  for (const address of [typedAddress(entry.url), bareAddress(entry.url)]) {
    if (address.toLowerCase().startsWith(typed)) return address.slice(typed.length)
  }
  return null
}

/** Whether Enter on `text` searches for it rather than opening an address. */
function searches(text: string, search: BrowserSearch): boolean {
  if (search === 'off') return false
  return resolveBrowserAddress(text, search).url !== null && resolveBrowserAddress(text, 'off').url === null
}

/**
 * What the bar offers for `text`.
 *
 * With nothing typed, the pages visited last. Otherwise the ranked matches,
 * and - when Enter would search for the text - a "Search <engine> for" row
 * first. When the top match completes inline, Enter opens that page rather
 * than searching, so its row comes first and the Search row after it.
 */
export function suggestBrowserAddresses(
  entries: readonly BrowserHistoryEntry[],
  text: string,
  search: BrowserSearch,
  max: number = BROWSER_SUGGESTIONS_MAX
): BrowserSuggestions {
  const page = (entry: BrowserHistoryEntry): BrowserSuggestion => ({ kind: 'page', url: entry.url, title: entry.title })

  if (text.trim() === '') {
    const recent = [...entries].sort((a, b) => b.lastVisit - a.lastVisit || a.url.localeCompare(b.url))
    return { text, rows: recent.slice(0, max).map(page), completion: null }
  }

  const ranked = rankBrowserHistory(entries, text)
  const top = ranked[0]
  const rest = top !== undefined && top.tier <= 1 ? inlineCompletion(text, top.entry) : null
  const completion = top !== undefined && rest !== null ? { url: top.entry.url, rest } : null
  const engine = browserSearchName(search)
  const searchRow: BrowserSuggestion[] =
    engine !== null && searches(text, search) ? [{ kind: 'search', text: text.trim(), engine }] : []
  const pages = ranked.map(({ entry }) => page(entry))
  const rows =
    completion !== null ? [...pages.slice(0, 1), ...searchRow, ...pages.slice(1)] : [...searchRow, ...pages]
  return { text, rows: rows.slice(0, max), completion }
}
