/**
 * A page's icon, for its tab in the Browser strip.
 *
 * Chromium reports a page's icon as candidate addresses (`page-favicon-updated`)
 * and fetches nothing for Floti, so main fetches the one it picks and hands the
 * window a `data:` URL. The window never fetches it: its CSP allows images from
 * itself and `data:` only, and an icon fetched by the renderer would leave from
 * the app's session rather than the page's.
 *
 * Three rules make it safe to fetch an address a page named:
 *
 * - **Never an address the page could not go to.** Every address, the first
 *   and each redirect hop, is checked with the tab's own reach (`allows`)
 *   before it is asked for, so a page on a local dev server cannot make main
 *   fetch something its tab is refused, and an agent's local-only tab cannot
 *   reach the internet through its icon.
 * - **Through the browser partition** (`fetch` is the partition's
 *   `session.fetch`): the request carries the page's own cookies and proxy, as
 *   the page's own request for the icon would, and nothing of the app's.
 * - **Only a small image.** An `image/*` answer up to `FAVICON_BYTES_MAX`, read
 *   no further than the cap, so a page naming a huge file costs one cancelled
 *   read.
 */

/** The largest icon taken, in bytes. A favicon is a few kilobytes; a 512px PNG touch icon fits. */
export const FAVICON_BYTES_MAX = 64 * 1024

/** How many icon addresses are remembered, so a tab switch or a second tab on one site fetches nothing. */
export const FAVICON_CACHE_MAX = 128

/** Redirect hops followed, each checked against the reach rule. */
const FAVICON_REDIRECTS_MAX = 3

/** How long one icon may take. A hanging server must not hold a request open for ever. */
const FAVICON_TIMEOUT_MS = 10_000

/** A `data:` icon a page declared, used as is when it is a small image. */
const DATA_IMAGE = /^data:image\/[a-z0-9.+-]+(;[a-z0-9=._+-]+)*,/i

/** The longest `data:` icon taken: the byte cap once base64 has grown it, plus its header. */
const DATA_ICON_CHARS_MAX = Math.ceil(FAVICON_BYTES_MAX / 3) * 4 + 128

/** A media type an icon may have: an image, by its bare name. */
const IMAGE_TYPE = /^image\/[a-z0-9.+-]+$/

/** The partition's `fetch`, as far as this module uses it. */
export type FaviconFetch = (
  url: string,
  init: { redirect: 'manual'; signal: AbortSignal }
) => Promise<Response>

export interface Favicons {
  /**
   * The icon for a page, as a `data:` URL, or null for none it may have.
   *
   * `candidates` are the addresses Chromium reported, best first; the first
   * that is a `data:` image or an http(s) address `allows` is the one taken. A
   * refused candidate is passed over and never fetched.
   */
  iconFor(candidates: readonly string[], allows: (url: string) => boolean): Promise<string | null>
}

export interface FaviconOptions {
  fetch: FaviconFetch
  bytesMax?: number
  cacheMax?: number
}

/** Why an icon could not be had: `final` answers are remembered, a network failure is not. */
class NoIcon extends Error {
  constructor(readonly final: boolean) {
    super('no icon')
  }
}

export function createFavicons(options: FaviconOptions): Favicons {
  const bytesMax = options.bytesMax ?? FAVICON_BYTES_MAX
  const cacheMax = options.cacheMax ?? FAVICON_CACHE_MAX
  /**
   * By icon address, least recently used first. A promise rather than a value,
   * so two tabs reporting the same icon at once share one request.
   */
  const cache = new Map<string, Promise<string | null>>()

  const remembered = (url: string): Promise<string | null> | undefined => {
    const found = cache.get(url)
    if (found === undefined) return undefined
    cache.delete(url)
    cache.set(url, found)
    return found
  }

  const remember = (url: string, icon: Promise<string | null>): void => {
    cache.set(url, icon)
    while (cache.size > cacheMax) {
      const oldest = cache.keys().next()
      if (oldest.done === true) break
      cache.delete(oldest.value)
    }
  }

  /** The bytes of a response, or null once they pass the cap. Reads no further than it. */
  const readCapped = async (response: Response): Promise<Uint8Array | null> => {
    if (response.body === null) {
      const whole = new Uint8Array(await response.arrayBuffer())
      return whole.byteLength > bytesMax ? null : whole
    }
    const reader = response.body.getReader()
    const chunks: Uint8Array[] = []
    let total = 0
    for (;;) {
      const { done, value } = await reader.read()
      if (done) break
      total += value.byteLength
      if (total > bytesMax) {
        await reader.cancel().catch(() => undefined)
        return null
      }
      chunks.push(value)
    }
    const bytes = new Uint8Array(total)
    let at = 0
    for (const chunk of chunks) {
      bytes.set(chunk, at)
      at += chunk.byteLength
    }
    return bytes
  }

  /** Fetch one icon address, following redirects only to addresses `allows`. */
  const fetchIcon = async (url: string, allows: (url: string) => boolean): Promise<string> => {
    const signal = AbortSignal.timeout(FAVICON_TIMEOUT_MS)
    let at = url
    for (let hop = 0; hop <= FAVICON_REDIRECTS_MAX; hop += 1) {
      if (!isHttp(at) || !allows(at)) throw new NoIcon(true)
      const response = await options.fetch(at, { redirect: 'manual', signal })
      if (response.status >= 300 && response.status < 400) {
        const location = response.headers.get('location')
        if (location === null) throw new NoIcon(true)
        at = resolve(location, at)
        continue
      }
      // A fetch that followed a redirect on its own landed somewhere unchecked.
      // Whatever it brought back is not used.
      if (response.url !== '' && response.url !== at && !allows(response.url)) throw new NoIcon(true)
      if (!response.ok) throw new NoIcon(true)
      const type = imageType(response.headers.get('content-type'))
      if (type === null) throw new NoIcon(true)
      const declared = Number(response.headers.get('content-length') ?? '0')
      if (declared > bytesMax) throw new NoIcon(true)
      const bytes = await readCapped(response)
      if (bytes === null || bytes.byteLength === 0) throw new NoIcon(true)
      return `data:${type};base64,${Buffer.from(bytes).toString('base64')}`
    }
    throw new NoIcon(true)
  }

  return {
    iconFor(candidates, allows) {
      const chosen = candidates.find(
        (candidate) =>
          (DATA_IMAGE.test(candidate) && candidate.length <= DATA_ICON_CHARS_MAX) ||
          (isHttp(candidate) && allows(candidate))
      )
      if (chosen === undefined) return Promise.resolve(null)
      if (chosen.startsWith('data:')) return Promise.resolve(chosen)

      const known = remembered(chosen)
      if (known !== undefined) return known
      const icon: Promise<string | null> = fetchIcon(chosen, allows).catch((err: unknown) => {
        // A refusal, a wrong type or a size over the cap is the answer for that
        // address. A failed connection is not: the next report asks again.
        if (!(err instanceof NoIcon && err.final) && cache.get(chosen) === icon) cache.delete(chosen)
        return null
      })
      remember(chosen, icon)
      return icon
    }
  }
}

function isHttp(url: string): boolean {
  try {
    const { protocol } = new URL(url)
    return protocol === 'http:' || protocol === 'https:'
  } catch {
    return false
  }
}

/** A `Location` header against the address that sent it. Unparseable is "nowhere", which no reach allows. */
function resolve(location: string, base: string): string {
  try {
    return new URL(location, base).href
  } catch {
    return ''
  }
}

/** The image type of a `Content-Type`, without parameters, or null for anything else. */
function imageType(header: string | null): string | null {
  if (header === null) return null
  const type = header.split(';')[0]!.trim().toLowerCase()
  return IMAGE_TYPE.test(type) ? type : null
}
