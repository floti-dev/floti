import { describe, expect, it } from 'vitest'
import { createFavicons, FAVICON_BYTES_MAX, type FaviconFetch } from './favicons'

/**
 * The icon fetcher, against a fetch the test answers. Every address it asks
 * for is recorded, so "never fetched" is a list that does not contain it.
 */

const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 1, 2, 3])
const PNG_URL = `data:image/png;base64,${Buffer.from(PNG).toString('base64')}`

const image = (bytes: Uint8Array = PNG, type = 'image/png'): Response =>
  new Response(bytes, { status: 200, headers: { 'content-type': type } })

const redirect = (location: string): Response =>
  new Response(null, { status: 302, headers: { location } })

/** A fetch answering from a table by address, refusing anything not in it, recording every call. */
function server(answers: Record<string, () => Response | Promise<Response>>): {
  fetch: FaviconFetch
  asked: string[]
} {
  const asked: string[] = []
  return {
    asked,
    fetch: (url, init) => {
      asked.push(url)
      expect(init.redirect).toBe('manual')
      const answer = answers[url]
      return answer === undefined ? Promise.reject(new Error(`no route to ${url}`)) : Promise.resolve(answer())
    }
  }
}

const anywhere = (): boolean => true
const localOnly = (url: string): boolean => new URL(url).hostname === 'localhost'

describe('favicons - which candidate', () => {
  it('takes the first http(s) candidate and turns it into a data URL', async () => {
    const s = server({ 'http://localhost:3000/favicon.ico': () => image(PNG, 'image/x-icon') })
    const favicons = createFavicons({ fetch: s.fetch })
    const icon = await favicons.iconFor(
      ['http://localhost:3000/favicon.ico', 'http://localhost:3000/other.png'],
      anywhere
    )
    expect(icon).toBe(`data:image/x-icon;base64,${Buffer.from(PNG).toString('base64')}`)
    expect(s.asked).toEqual(['http://localhost:3000/favicon.ico'])
  })

  it('uses a data: image the page declared as it is, fetching nothing', async () => {
    const s = server({})
    const favicons = createFavicons({ fetch: s.fetch })
    expect(await favicons.iconFor([PNG_URL], anywhere)).toBe(PNG_URL)
    expect(s.asked).toEqual([])
  })

  it('passes over a data: URL that is not an image, and one past the size cap', async () => {
    const s = server({})
    const favicons = createFavicons({ fetch: s.fetch })
    const huge = `data:image/png;base64,${'A'.repeat(FAVICON_BYTES_MAX * 2)}`
    expect(await favicons.iconFor(['data:text/html,<script>1</script>', huge], anywhere)).toBeNull()
  })

  it('never fetches a candidate the reach refuses, and takes the next one it allows', async () => {
    const s = server({ 'http://localhost:3000/icon.png': () => image() })
    const favicons = createFavicons({ fetch: s.fetch })
    const icon = await favicons.iconFor(
      ['https://cdn.example.com/icon.png', 'http://localhost:3000/icon.png'],
      localOnly
    )
    expect(icon).toBe(PNG_URL)
    expect(s.asked).toEqual(['http://localhost:3000/icon.png'])
  })

  it('answers null with nothing fetched when every candidate is refused or not http(s)', async () => {
    const s = server({})
    const favicons = createFavicons({ fetch: s.fetch })
    expect(
      await favicons.iconFor(['https://cdn.example.com/icon.png', 'file:///C:/icon.png', 'not a url'], localOnly)
    ).toBeNull()
    expect(await favicons.iconFor([], anywhere)).toBeNull()
    expect(s.asked).toEqual([])
  })
})

describe('favicons - what is taken', () => {
  it('takes only an image', async () => {
    const s = server({
      'http://localhost/page.html': () =>
        new Response('<html></html>', { status: 200, headers: { 'content-type': 'text/html' } }),
      'http://localhost/untyped': () => new Response(PNG, { status: 200 })
    })
    const favicons = createFavicons({ fetch: s.fetch })
    expect(await favicons.iconFor(['http://localhost/page.html'], anywhere)).toBeNull()
    expect(await favicons.iconFor(['http://localhost/untyped'], anywhere)).toBeNull()
  })

  it('keeps the bare image type, dropping parameters', async () => {
    const s = server({ 'http://localhost/i.svg': () => image(PNG, 'Image/SVG+XML; charset=utf-8') })
    const favicons = createFavicons({ fetch: s.fetch })
    expect(await favicons.iconFor(['http://localhost/i.svg'], anywhere)).toMatch(/^data:image\/svg\+xml;base64,/)
  })

  it('takes nothing from an answer that is not a success', async () => {
    const s = server({ 'http://localhost/missing.ico': () => new Response(PNG, { status: 404, headers: { 'content-type': 'image/png' } }) })
    const favicons = createFavicons({ fetch: s.fetch })
    expect(await favicons.iconFor(['http://localhost/missing.ico'], anywhere)).toBeNull()
  })

  it('refuses an image past the size cap, whether it says so or not', async () => {
    const big = new Uint8Array(65)
    let pulled = 0
    const s = server({
      'http://localhost/declared.png': () =>
        new Response(big, { status: 200, headers: { 'content-type': 'image/png', 'content-length': '65' } }),
      // No length, in chunks: read only until the cap is passed.
      'http://localhost/streamed.png': () =>
        new Response(
          new ReadableStream<Uint8Array>({
            pull(controller) {
              pulled += 1
              controller.enqueue(new Uint8Array(40))
            }
          }),
          { status: 200, headers: { 'content-type': 'image/png' } }
        )
    })
    const favicons = createFavicons({ fetch: s.fetch, bytesMax: 64 })
    expect(await favicons.iconFor(['http://localhost/declared.png'], anywhere)).toBeNull()
    expect(await favicons.iconFor(['http://localhost/streamed.png'], anywhere)).toBeNull()
    // Two chunks pass 64 bytes; the stream is endless, so reading on would never return.
    expect(pulled).toBeLessThanOrEqual(3)
  })

  it('takes an empty answer as no icon', async () => {
    const s = server({ 'http://localhost/empty.png': () => image(new Uint8Array(0)) })
    const favicons = createFavicons({ fetch: s.fetch })
    expect(await favicons.iconFor(['http://localhost/empty.png'], anywhere)).toBeNull()
  })
})

describe('favicons - redirects', () => {
  it('follows a redirect to an address the reach allows', async () => {
    const s = server({
      'http://localhost/favicon.ico': () => redirect('/static/icon.png'),
      'http://localhost/static/icon.png': () => image()
    })
    const favicons = createFavicons({ fetch: s.fetch })
    expect(await favicons.iconFor(['http://localhost/favicon.ico'], localOnly)).toBe(PNG_URL)
    expect(s.asked).toEqual(['http://localhost/favicon.ico', 'http://localhost/static/icon.png'])
  })

  it('never fetches a redirect hop the reach refuses', async () => {
    const s = server({
      'http://localhost/favicon.ico': () => redirect('https://cdn.example.com/icon.png'),
      'https://cdn.example.com/icon.png': () => image()
    })
    const favicons = createFavicons({ fetch: s.fetch })
    expect(await favicons.iconFor(['http://localhost/favicon.ico'], localOnly)).toBeNull()
    expect(s.asked).toEqual(['http://localhost/favicon.ico'])
  })

  it('gives up on a redirect loop', async () => {
    const s = server({ 'http://localhost/a': () => redirect('/a') })
    const favicons = createFavicons({ fetch: s.fetch })
    expect(await favicons.iconFor(['http://localhost/a'], anywhere)).toBeNull()
    expect(s.asked.length).toBeLessThanOrEqual(4)
  })

  it('drops an answer a fetch reached by following a redirect itself to somewhere refused', async () => {
    const followed = image()
    Object.defineProperty(followed, 'url', { value: 'https://cdn.example.com/icon.png' })
    const s = server({ 'http://localhost/favicon.ico': () => followed })
    const favicons = createFavicons({ fetch: s.fetch })
    expect(await favicons.iconFor(['http://localhost/favicon.ico'], localOnly)).toBeNull()
  })
})

describe('favicons - the cache', () => {
  it('fetches an icon address once, for every tab that reports it, at once or later', async () => {
    const s = server({ 'http://localhost/icon.png': () => image() })
    const favicons = createFavicons({ fetch: s.fetch })
    const [one, two] = await Promise.all([
      favicons.iconFor(['http://localhost/icon.png'], anywhere),
      favicons.iconFor(['http://localhost/icon.png'], anywhere)
    ])
    expect(await favicons.iconFor(['http://localhost/icon.png'], anywhere)).toBe(PNG_URL)
    expect([one, two]).toEqual([PNG_URL, PNG_URL])
    expect(s.asked).toEqual(['http://localhost/icon.png'])
  })

  it('serves a remembered icon only where the reach allows its address', async () => {
    const s = server({ 'https://example.com/icon.png': () => image() })
    const favicons = createFavicons({ fetch: s.fetch })
    expect(await favicons.iconFor(['https://example.com/icon.png'], anywhere)).toBe(PNG_URL)
    expect(await favicons.iconFor(['https://example.com/icon.png'], localOnly)).toBeNull()
  })

  it('remembers an answer, but not a failed connection', async () => {
    let up = false
    const s = server({
      'http://localhost/icon.png': () => (up ? image() : Promise.reject(new Error('ECONNREFUSED'))),
      'http://localhost/page.html': () => new Response('x', { status: 200, headers: { 'content-type': 'text/html' } })
    })
    const favicons = createFavicons({ fetch: s.fetch })
    expect(await favicons.iconFor(['http://localhost/icon.png'], anywhere)).toBeNull()
    up = true
    expect(await favicons.iconFor(['http://localhost/icon.png'], anywhere)).toBe(PNG_URL)

    expect(await favicons.iconFor(['http://localhost/page.html'], anywhere)).toBeNull()
    expect(await favicons.iconFor(['http://localhost/page.html'], anywhere)).toBeNull()
    expect(s.asked.filter((url) => url === 'http://localhost/page.html')).toHaveLength(1)
  })

  it('holds no more addresses than its bound, forgetting the least recently used', async () => {
    const urls = ['http://localhost/1.png', 'http://localhost/2.png', 'http://localhost/3.png']
    const s = server(Object.fromEntries(urls.map((url) => [url, () => image()])))
    const favicons = createFavicons({ fetch: s.fetch, cacheMax: 2 })
    await favicons.iconFor([urls[0]!], anywhere)
    await favicons.iconFor([urls[1]!], anywhere)
    // 1 is used again, so 2 is the oldest when 3 arrives.
    await favicons.iconFor([urls[0]!], anywhere)
    await favicons.iconFor([urls[2]!], anywhere)
    await favicons.iconFor([urls[0]!], anywhere)
    await favicons.iconFor([urls[1]!], anywhere)
    expect(s.asked).toEqual([urls[0], urls[1], urls[2], urls[1]])
  })
})
