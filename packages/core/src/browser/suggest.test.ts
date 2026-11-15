import { describe, expect, it } from 'vitest'
import {
  BROWSER_SUGGESTIONS_MAX,
  inlineCompletion,
  rankBrowserHistory,
  suggestBrowserAddresses,
  typedAddress,
  type BrowserHistoryEntry
} from './suggest'

const page = (url: string, title = '', visits = 1, lastVisit = 1): BrowserHistoryEntry => ({
  url,
  title,
  visits,
  lastVisit
})

const urls = (ranked: Array<{ entry: BrowserHistoryEntry }>): string[] => ranked.map(({ entry }) => entry.url)

describe('typedAddress', () => {
  it('drops the scheme, www. and a bare host’s trailing slash, and keeps a path as it is', () => {
    expect(typedAddress('https://www.example.com/')).toBe('example.com')
    expect(typedAddress('http://localhost:3000/')).toBe('localhost:3000')
    expect(typedAddress('http://localhost:3000/app/')).toBe('localhost:3000/app/')
    expect(typedAddress('https://example.com/?q=1')).toBe('example.com/?q=1')
    expect(typedAddress('example.com')).toBe('example.com')
  })
})

describe('rankBrowserHistory', () => {
  it('puts an address that starts with the text first, then a word start in the host or title, then a substring', () => {
    const history = [
      page('https://docs.example.com/react', 'Example docs'),
      page('https://reactjs.org/', 'React'),
      page('https://news.example.com/', 'Why we use React'),
      page('https://example.com/preact', 'Preact')
    ]
    expect(urls(rankBrowserHistory(history, 'react'))).toEqual([
      'https://reactjs.org/',
      'https://news.example.com/',
      'https://docs.example.com/react',
      'https://example.com/preact'
    ])
  })

  it('matches after the scheme and www. on both sides, ignoring case', () => {
    const history = [page('https://www.github.com/'), page('http://localhost:5173/')]
    expect(urls(rankBrowserHistory(history, 'GitH'))).toEqual(['https://www.github.com/'])
    expect(urls(rankBrowserHistory(history, 'www.git'))).toEqual(['https://www.github.com/'])
    expect(urls(rankBrowserHistory(history, 'http://local'))).toEqual(['http://localhost:5173/'])
  })

  it('breaks ties by visits, then by the more recent visit', () => {
    const history = [
      page('https://example.com/a', '', 1, 30),
      page('https://example.com/b', '', 5, 10),
      page('https://example.com/c', '', 5, 20)
    ]
    expect(urls(rankBrowserHistory(history, 'example'))).toEqual([
      'https://example.com/c',
      'https://example.com/b',
      'https://example.com/a'
    ])
  })

  it('matches a word in the title wherever it starts', () => {
    const history = [page('https://a.test/', 'The Rust Book'), page('https://b.test/', 'Trusted types')]
    expect(rankBrowserHistory(history, 'rust').map(({ entry, tier }) => [entry.url, tier])).toEqual([
      ['https://a.test/', 2],
      ['https://b.test/', 3]
    ])
  })

  it('puts the address exactly as typed before a longer one it starts, however often that was visited', () => {
    const history = [page('http://localhost:3000/app', '', 9, 20), page('http://localhost:3000/', '', 1, 10)]
    expect(urls(rankBrowserHistory(history, 'localhost:3000'))).toEqual([
      'http://localhost:3000/',
      'http://localhost:3000/app'
    ])
    expect(suggestBrowserAddresses(history, 'http://localhost:3000/', 'off').completion).toEqual({
      url: 'http://localhost:3000/',
      rest: ''
    })
  })

  it('matches nothing for text that is empty once stripped, or found nowhere', () => {
    const history = [page('https://example.com/')]
    expect(rankBrowserHistory(history, '')).toEqual([])
    expect(rankBrowserHistory(history, 'https://')).toEqual([])
    expect(rankBrowserHistory(history, 'zzz')).toEqual([])
  })
})

describe('inlineCompletion', () => {
  it('is the rest of the address after what was typed', () => {
    expect(inlineCompletion('exa', page('https://www.example.com/'))).toBe('mple.com')
    expect(inlineCompletion('Exa', page('https://example.com/docs'))).toBe('mple.com/docs')
    expect(inlineCompletion('local', page('http://localhost:3000/'))).toBe('host:3000')
  })

  it('is empty for the whole address, so Enter still goes to the page as it was visited', () => {
    expect(inlineCompletion('example.com', page('https://example.com/'))).toBe('')
    expect(inlineCompletion('example.com/', page('https://example.com/'))).toBe('')
  })

  it('lines up with a trailing slash that was typed', () => {
    expect(inlineCompletion('localhost:3000/', page('http://localhost:3000/app'))).toBe('app')
    expect(inlineCompletion('www.example.com/d', page('https://www.example.com/docs'))).toBe('ocs')
  })

  it('is null when the text does not start the address, is a phrase, or names another scheme', () => {
    expect(inlineCompletion('ample', page('https://example.com/'))).toBeNull()
    expect(inlineCompletion('example com', page('https://example.com/'))).toBeNull()
    expect(inlineCompletion('', page('https://example.com/'))).toBeNull()
    expect(inlineCompletion('http://exa', page('https://example.com/'))).toBeNull()
    expect(inlineCompletion('https://exa', page('https://example.com/'))).toBe('mple.com')
  })
})

describe('suggestBrowserAddresses', () => {
  it('offers at most eight rows, best match first', () => {
    const history = Array.from({ length: 12 }, (_, at) => page(`https://example.com/${String(at)}`, '', at))
    const { rows } = suggestBrowserAddresses(history, 'example', 'off')
    expect(rows).toHaveLength(BROWSER_SUGGESTIONS_MAX)
    expect(rows[0]).toEqual({ kind: 'page', url: 'https://example.com/11', title: '' })
  })

  it('offers the pages visited last when nothing is typed', () => {
    const history = [page('https://a.test/', 'A', 9, 1), page('https://b.test/', 'B', 1, 3), page('https://c.test/', 'C', 1, 2)]
    const { rows, completion } = suggestBrowserAddresses(history, '', 'google')
    expect(rows.map((row) => (row.kind === 'page' ? row.url : row.kind))).toEqual([
      'https://b.test/',
      'https://c.test/',
      'https://a.test/'
    ])
    expect(completion).toBeNull()
  })

  it('offers Search <engine> first for a phrase when a search engine is set', () => {
    const history = [page('https://docs.rs/', 'Rust docs')]
    const { rows, completion } = suggestBrowserAddresses(history, 'rust docs', 'duckduckgo')
    expect(rows[0]).toEqual({ kind: 'search', text: 'rust docs', engine: 'DuckDuckGo' })
    expect(rows[1]).toEqual({ kind: 'page', url: 'https://docs.rs/', title: 'Rust docs' })
    expect(completion).toBeNull()
  })

  it('offers no Search row with searching off, or for an address', () => {
    expect(suggestBrowserAddresses([], 'rust docs', 'off').rows).toEqual([])
    expect(suggestBrowserAddresses([], 'example.com', 'google').rows).toEqual([])
    expect(suggestBrowserAddresses([], '3000', 'google').rows).toEqual([])
  })

  it('completes the top match inline and puts its row before the Search row, since Enter opens it', () => {
    const history = [page('https://reactjs.org/', 'React'), page('https://example.com/react', 'React at Example')]
    const { rows, completion } = suggestBrowserAddresses(history, 'react', 'google')
    expect(completion).toEqual({ url: 'https://reactjs.org/', rest: 'js.org' })
    expect(rows.map((row) => row.kind)).toEqual(['page', 'search', 'page'])
  })

  it('does not complete when the top match only matches a word or a substring', () => {
    const history = [page('https://example.com/', 'Docs')]
    expect(suggestBrowserAddresses(history, 'docs', 'off').completion).toBeNull()
  })

  it('answers with the text it was asked about', () => {
    expect(suggestBrowserAddresses([], ' exa', 'off').text).toBe(' exa')
  })
})
