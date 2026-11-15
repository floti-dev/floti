import { afterEach, describe, expect, it, vi } from 'vitest'
import { FakeNet, type FakeRoute } from '../../test/browser-electron'
import { createFavicons, FAVICON_BYTES_MAX, FAVICON_TIMEOUT_MS, type Favicons } from './favicons'

/**
 * The icon fetcher, against a fake `net.request` that behaves as Electron's
 * does with `redirect: 'manual'` (`FakeNetRequest`). Every address the network
 * is asked for is recorded, redirect hops included, so "never fetched" is a
 * list that does not contain it.
 */

const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 1, 2, 3])
const PNG_URL = `data:image/png;base64,${Buffer.from(PNG).toString('base64')}`

const image = (chunks: Uint8Array[] = [PNG], type = 'image/png'): FakeRoute => ({
  headers: { 'content-type': type },
  chunks
})

/** A fetcher over a fake network answering `routes`. */
function server(routes: Record<string, FakeRoute>, options: { bytesMax?: number; cacheMax?: number } = {}): {
  favicons: Favicons
  net: FakeNet
} {
  const net = new FakeNet()
  for (const [url, route] of Object.entries(routes)) net.routes.set(url, route)
  const favicons = createFavicons({
    request: (url) => net.request({ url, redirect: 'manual' }),
    ...options
  })
  return { favicons, net }
}

const anywhere = (): boolean => true
const localOnly = (url: string): boolean => new URL(url).hostname === 'localhost'

afterEach(() => {
  vi.useRealTimers()
})

describe('favicons - which candidate', () => {
  it('takes the first http(s) candidate and turns it into a data URL', async () => {
    const s = server({ 'http://localhost:3000/favicon.ico': image([PNG], 'image/x-icon') })
    const icon = await s.favicons.iconFor(
      ['http://localhost:3000/favicon.ico', 'http://localhost:3000/other.png'],
      anywhere
    )
    expect(icon).toBe(`data:image/x-icon;base64,${Buffer.from(PNG).toString('base64')}`)
    expect(s.net.asked).toEqual(['http://localhost:3000/favicon.ico'])
    expect(s.net.requests[0]!.options.redirect).toBe('manual')
  })

  it('uses a data: image the page declared as it is, fetching nothing', async () => {
    const s = server({})
    expect(await s.favicons.iconFor([PNG_URL], anywhere)).toBe(PNG_URL)
    expect(s.net.asked).toEqual([])
  })

  it('passes over a data: URL that is not an image, and one past the size cap', async () => {
    const s = server({})
    const huge = `data:image/png;base64,${'A'.repeat(FAVICON_BYTES_MAX * 2)}`
    expect(await s.favicons.iconFor(['data:text/html,<script>1</script>', huge], anywhere)).toBeNull()
  })

  it('never fetches a candidate the reach refuses, and takes the next one it allows', async () => {
    const s = server({ 'http://localhost:3000/icon.png': image() })
    const icon = await s.favicons.iconFor(
      ['https://cdn.example.com/icon.png', 'http://localhost:3000/icon.png'],
      localOnly
    )
    expect(icon).toBe(PNG_URL)
    expect(s.net.asked).toEqual(['http://localhost:3000/icon.png'])
  })

  it('answers null with nothing fetched when every candidate is refused or not http(s)', async () => {
    const s = server({})
    expect(
      await s.favicons.iconFor(['https://cdn.example.com/icon.png', 'file:///C:/icon.png', 'not a url'], localOnly)
    ).toBeNull()
    expect(await s.favicons.iconFor([], anywhere)).toBeNull()
    expect(s.net.asked).toEqual([])
  })
})

describe('favicons - what is taken', () => {
  it('takes only an image', async () => {
    const s = server({
      'http://localhost/page.html': { headers: { 'content-type': 'text/html' }, chunks: [PNG] },
      'http://localhost/untyped': { chunks: [PNG] }
    })
    expect(await s.favicons.iconFor(['http://localhost/page.html'], anywhere)).toBeNull()
    expect(await s.favicons.iconFor(['http://localhost/untyped'], anywhere)).toBeNull()
  })

  it('keeps the bare image type, dropping parameters', async () => {
    const s = server({ 'http://localhost/i.svg': image([PNG], 'Image/SVG+XML; charset=utf-8') })
    expect(await s.favicons.iconFor(['http://localhost/i.svg'], anywhere)).toMatch(/^data:image\/svg\+xml;base64,/)
  })

  it('joins a body that arrives in pieces', async () => {
    const s = server({ 'http://localhost/icon.png': image([PNG.slice(0, 3), PNG.slice(3)]) })
    expect(await s.favicons.iconFor(['http://localhost/icon.png'], anywhere)).toBe(PNG_URL)
  })

  it('takes nothing from an answer that is not a success', async () => {
    const s = server({ 'http://localhost/missing.ico': { ...image(), status: 404 } })
    expect(await s.favicons.iconFor(['http://localhost/missing.ico'], anywhere)).toBeNull()
  })

  it('refuses an image past the size cap, whether it says so or not, reading no further', async () => {
    const s = server(
      {
        'http://localhost/declared.png': {
          headers: { 'content-type': 'image/png', 'content-length': '65' },
          chunks: [new Uint8Array(65)]
        },
        // No length, and a body that never ends: reading stops once it passes the cap.
        'http://localhost/streamed.png': { headers: { 'content-type': 'image/png' }, endless: 40 }
      },
      { bytesMax: 64 }
    )
    expect(await s.favicons.iconFor(['http://localhost/declared.png'], anywhere)).toBeNull()
    expect(s.net.requests[0]!.delivered).toBe(0)
    expect(await s.favicons.iconFor(['http://localhost/streamed.png'], anywhere)).toBeNull()
    expect(s.net.requests[1]!.delivered).toBe(2)
    expect(s.net.requests[1]!.aborted).toBe(true)
  })

  it('takes an empty answer as no icon', async () => {
    const s = server({ 'http://localhost/empty.png': image([]) })
    expect(await s.favicons.iconFor(['http://localhost/empty.png'], anywhere)).toBeNull()
  })

  it('gives up on an answer that never comes, after the timeout', async () => {
    vi.useFakeTimers()
    const s = server({ 'http://localhost/slow.png': { hang: true } })
    const icon = s.favicons.iconFor(['http://localhost/slow.png'], anywhere)
    await vi.advanceTimersByTimeAsync(FAVICON_TIMEOUT_MS)
    expect(await icon).toBeNull()
    expect(s.net.requests[0]!.aborted).toBe(true)
  })
})

describe('favicons - redirects', () => {
  it('follows a redirect to an address the reach allows', async () => {
    const s = server({
      'http://localhost/favicon.ico': { redirect: '/static/icon.png' },
      'http://localhost/static/icon.png': image()
    })
    expect(await s.favicons.iconFor(['http://localhost/favicon.ico'], localOnly)).toBe(PNG_URL)
    expect(s.net.asked).toEqual(['http://localhost/favicon.ico', 'http://localhost/static/icon.png'])
  })

  it('never fetches a redirect hop the reach refuses', async () => {
    const s = server({
      'http://localhost/favicon.ico': { redirect: 'https://cdn.example.com/icon.png' },
      'https://cdn.example.com/icon.png': image()
    })
    expect(await s.favicons.iconFor(['http://localhost/favicon.ico'], localOnly)).toBeNull()
    expect(s.net.asked).toEqual(['http://localhost/favicon.ico'])
    expect(s.net.requests[0]!.aborted).toBe(true)
  })

  it('never follows a redirect off http(s)', async () => {
    const s = server({ 'http://localhost/favicon.ico': { redirect: 'file:///C:/icon.png' } })
    expect(await s.favicons.iconFor(['http://localhost/favicon.ico'], anywhere)).toBeNull()
    expect(s.net.asked).toEqual(['http://localhost/favicon.ico'])
  })

  it('gives up after three hops', async () => {
    const s = server({
      'http://localhost/0': { redirect: '/1' },
      'http://localhost/1': { redirect: '/2' },
      'http://localhost/2': { redirect: '/3' },
      'http://localhost/3': { redirect: '/4' },
      'http://localhost/4': image()
    })
    expect(await s.favicons.iconFor(['http://localhost/0'], anywhere)).toBeNull()
    expect(s.net.asked).toEqual(['http://localhost/0', 'http://localhost/1', 'http://localhost/2', 'http://localhost/3'])
  })

  it('remembers a refused hop as the answer, and asks again after a failed connection', async () => {
    const s = server({
      'http://localhost/favicon.ico': { redirect: 'https://cdn.example.com/icon.png' },
      'http://localhost/down.png': { refuse: true }
    })
    await s.favicons.iconFor(['http://localhost/favicon.ico'], localOnly)
    await s.favicons.iconFor(['http://localhost/favicon.ico'], localOnly)
    expect(s.net.asked).toEqual(['http://localhost/favicon.ico'])

    expect(await s.favicons.iconFor(['http://localhost/down.png'], anywhere)).toBeNull()
    s.net.routes.set('http://localhost/down.png', image())
    expect(await s.favicons.iconFor(['http://localhost/down.png'], anywhere)).toBe(PNG_URL)
  })
})

describe('favicons - the cache', () => {
  it('fetches an icon address once, for every tab that reports it, at once or later', async () => {
    const s = server({ 'http://localhost/icon.png': image() })
    const [one, two] = await Promise.all([
      s.favicons.iconFor(['http://localhost/icon.png'], anywhere),
      s.favicons.iconFor(['http://localhost/icon.png'], anywhere)
    ])
    expect(await s.favicons.iconFor(['http://localhost/icon.png'], anywhere)).toBe(PNG_URL)
    expect([one, two]).toEqual([PNG_URL, PNG_URL])
    expect(s.net.asked).toEqual(['http://localhost/icon.png'])
  })

  it('serves a remembered icon only where the reach allows its address', async () => {
    const s = server({ 'https://example.com/icon.png': image() })
    expect(await s.favicons.iconFor(['https://example.com/icon.png'], anywhere)).toBe(PNG_URL)
    expect(await s.favicons.iconFor(['https://example.com/icon.png'], localOnly)).toBeNull()
  })

  it('remembers an answer that is not an image', async () => {
    const s = server({ 'http://localhost/page.html': { headers: { 'content-type': 'text/html' }, chunks: [PNG] } })
    expect(await s.favicons.iconFor(['http://localhost/page.html'], anywhere)).toBeNull()
    expect(await s.favicons.iconFor(['http://localhost/page.html'], anywhere)).toBeNull()
    expect(s.net.asked).toEqual(['http://localhost/page.html'])
  })

  it('holds no more addresses than its bound, forgetting the least recently used', async () => {
    const urls = ['http://localhost/1.png', 'http://localhost/2.png', 'http://localhost/3.png']
    const s = server(Object.fromEntries(urls.map((url) => [url, image()])), { cacheMax: 2 })
    await s.favicons.iconFor([urls[0]!], anywhere)
    await s.favicons.iconFor([urls[1]!], anywhere)
    // 1 is used again, so 2 is the oldest when 3 arrives.
    await s.favicons.iconFor([urls[0]!], anywhere)
    await s.favicons.iconFor([urls[2]!], anywhere)
    await s.favicons.iconFor([urls[0]!], anywhere)
    await s.favicons.iconFor([urls[1]!], anywhere)
    expect(s.net.asked).toEqual([urls[0], urls[1], urls[2], urls[1]])
  })
})
