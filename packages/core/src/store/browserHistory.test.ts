import Database from 'better-sqlite3'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import {
  BROWSER_HISTORY_MAX,
  BROWSER_HISTORY_TITLE_MAX,
  BROWSER_HISTORY_URL_MAX,
  openBrowserHistory,
  type BrowserHistory
} from './browserHistory'
import { openStore, type Store } from './db'
import { migrate } from './migrate'
import { MIGRATIONS } from './migrations.generated'
import { readSettings } from './settings'

let store: Store
let clock: number

beforeEach(() => {
  store = openStore({ file: ':memory:' })
  clock = 1_000
})

afterEach(() => {
  store.close()
})

/** A history on the test's store whose clock moves a millisecond per visit. */
const open = (max?: number): BrowserHistory => openBrowserHistory(store, { now: () => ++clock, ...(max === undefined ? {} : { max }) })

/** The table as it is on disk, newest first. */
const onDisk = (): Array<{ url: string; title: string; visits: number; last_visit: number }> =>
  store.raw.prepare('SELECT url, title, visits, last_visit FROM browser_history ORDER BY last_visit DESC').all() as Array<{
    url: string
    title: string
    visits: number
    last_visit: number
  }>

const sorted = (history: BrowserHistory): string[] =>
  [...history.entries()].sort((a, b) => b.lastVisit - a.lastVisit).map((entry) => entry.url)

describe('the browser history', () => {
  it('counts visits, keeps the newest last visit and the title, and writes each change through', () => {
    const history = open()
    history.visit('https://example.com/', 'Example')
    history.visit('http://localhost:3000/')
    history.visit('https://example.com/')

    expect(onDisk()).toEqual([
      { url: 'https://example.com/', title: 'Example', visits: 2, last_visit: 1_003 },
      { url: 'http://localhost:3000/', title: '', visits: 1, last_visit: 1_002 }
    ])
    // A second copy read from the table agrees with the first.
    expect([...open().entries()].sort((a, b) => a.url.localeCompare(b.url))).toEqual([
      { url: 'http://localhost:3000/', title: '', visits: 1, lastVisit: 1_002 },
      { url: 'https://example.com/', title: 'Example', visits: 2, lastVisit: 1_003 }
    ])
  })

  it('takes a title the page says later, and keeps it when a later visit says none', () => {
    const history = open()
    history.visit('https://example.com/')
    history.retitle('https://example.com/', '  Example Domain ')
    history.visit('https://example.com/', '')
    expect(onDisk()[0]).toMatchObject({ title: 'Example Domain', visits: 2 })
  })

  it('ignores a title for a page it does not hold', () => {
    const history = open()
    history.retitle('https://nowhere.test/', 'Nowhere')
    expect(onDisk()).toEqual([])
    expect(history.entries()).toEqual([])
  })

  it('keeps only http and https addresses, and none past the length limit', () => {
    const history = open()
    history.visit('about:blank')
    history.visit('file:///C:/notes.html')
    history.visit('data:text/html,hi')
    history.visit(`https://example.com/${'a'.repeat(BROWSER_HISTORY_URL_MAX)}`)
    expect(onDisk()).toEqual([])
  })

  it('clips a long title', () => {
    const history = open()
    history.visit('https://example.com/', 'x'.repeat(BROWSER_HISTORY_TITLE_MAX + 50))
    expect(onDisk()[0]?.title).toHaveLength(BROWSER_HISTORY_TITLE_MAX)
  })

  it(`keeps the newest ${String(BROWSER_HISTORY_MAX)} pages by last visit`, () => {
    const history = open()
    for (let at = 0; at <= BROWSER_HISTORY_MAX; at++) history.visit(`https://example.com/${String(at)}`)
    expect(history.entries()).toHaveLength(BROWSER_HISTORY_MAX)
    expect(onDisk()).toHaveLength(BROWSER_HISTORY_MAX)
    expect(history.entries().some((entry) => entry.url === 'https://example.com/0')).toBe(false)
    expect(onDisk().at(-1)?.url).toBe('https://example.com/1')
  })

  it('drops the oldest past the limit, not the least visited, and a visit makes an old page new', () => {
    const history = open(2)
    history.visit('https://a.test/')
    history.visit('https://a.test/')
    history.visit('https://b.test/')
    history.visit('https://a.test/')
    history.visit('https://c.test/')
    expect(sorted(history)).toEqual(['https://c.test/', 'https://a.test/'])
    expect(onDisk().map((row) => row.url)).toEqual(['https://c.test/', 'https://a.test/'])
  })

  it('removes one page, and says whether it was there', () => {
    const history = open()
    history.visit('https://a.test/')
    history.visit('https://b.test/')
    expect(history.remove('https://a.test/')).toBe(true)
    expect(history.remove('https://a.test/')).toBe(false)
    expect(sorted(history)).toEqual(['https://b.test/'])
    expect(onDisk().map((row) => row.url)).toEqual(['https://b.test/'])
  })

  it('clears every page', () => {
    const history = open()
    history.visit('https://a.test/')
    history.visit('https://b.test/')
    history.clear()
    expect(history.entries()).toEqual([])
    expect(onDisk()).toEqual([])
  })
})

describe('migration 0034', () => {
  const SEEDS = '0034_browser_history'

  /** A database holding every migration before the history, with `recent` as the retired setting's raw value. */
  function earlierDatabase(recent: string | null): Database.Database {
    const raw = new Database(':memory:')
    raw.exec('CREATE TABLE __helm_migrations (tag TEXT PRIMARY KEY, applied_at TEXT NOT NULL)')
    const upTo = MIGRATIONS.findIndex((migration) => migration.tag === SEEDS)
    expect(upTo).toBeGreaterThan(0)
    for (const migration of MIGRATIONS.slice(0, upTo)) {
      for (const statement of migration.statements) raw.exec(statement)
      raw.prepare('INSERT INTO __helm_migrations (tag, applied_at) VALUES (?, ?)').run(migration.tag, '2026-11-01T00:00:00.000Z')
    }
    raw.prepare("INSERT INTO app_settings (key, value) VALUES ('browserSearch', '\"bing\"')").run()
    if (recent !== null) raw.prepare("INSERT INTO app_settings (key, value) VALUES ('browserRecentUrls', ?)").run(recent)
    return raw
  }

  const rows = (raw: Database.Database): Array<{ url: string; title: string; visits: number }> =>
    raw.prepare('SELECT url, title, visits FROM browser_history ORDER BY last_visit DESC').all() as Array<{
      url: string
      title: string
      visits: number
    }>
  const settingKeys = (raw: Database.Database): string[] =>
    (raw.prepare('SELECT key FROM app_settings ORDER BY key').all() as Array<{ key: string }>).map((row) => row.key)

  it('seeds the history from the recent addresses, newest first, and removes the setting', () => {
    const raw = earlierDatabase(JSON.stringify(['http://localhost:3000/', 'https://example.com/docs', 'http://localhost:3000/']))
    try {
      migrate(raw)
      expect(rows(raw)).toEqual([
        { url: 'http://localhost:3000/', title: '', visits: 1 },
        { url: 'https://example.com/docs', title: '', visits: 1 }
      ])
      expect(settingKeys(raw)).toEqual(['browserSearch'])
    } finally {
      raw.close()
    }
  })

  it('seeds nothing from a value that is not a list of http addresses, and still removes it', () => {
    for (const recent of ['not json', '"http://localhost:3000/"', JSON.stringify([42, 'file:///C:/x.html', null])]) {
      const raw = earlierDatabase(recent)
      try {
        migrate(raw)
        expect(rows(raw)).toEqual([])
        expect(settingKeys(raw)).toEqual(['browserSearch'])
      } finally {
        raw.close()
      }
    }
  })

  it('makes an empty history on a database that never had the setting', () => {
    const raw = earlierDatabase(null)
    try {
      migrate(raw)
      expect(rows(raw)).toEqual([])
    } finally {
      raw.close()
    }
  })

  it('leaves settings that read back without the retired key', () => {
    const fresh = openStore({ file: ':memory:' })
    try {
      expect(readSettings(fresh)).not.toHaveProperty('browserRecentUrls')
    } finally {
      fresh.close()
    }
  })
})
