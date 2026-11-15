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
 * - **Through the browser partition**: the request (`request`, which is
 *   `net.request` on the partition's session) carries the page's own cookies
 *   and proxy, as the page's own request for the icon would, and nothing of
 *   the app's.
 * - **Only a small image.** An `image/*` answer up to `FAVICON_BYTES_MAX`, read
 *   no further than the cap, so a page naming a huge file costs one cancelled
 *   read.
 *
 * Redirects are why this is `net.request` and not `session.fetch`. The hop has
 * to be checked before it is followed, which takes `redirect: 'manual'`; and
 * there `session.fetch` never hands back the 3xx - its request cancels the
 * redirect and the fetch rejects - so a check written against a 3xx answer
 * never runs and every redirected icon is lost. `net.request` raises
 * `redirect` with the address it is going to, and goes there only if
 * `followRedirect()` is called inside that event.
 */

/** The largest icon taken, in bytes. A favicon is a few kilobytes; a 512px PNG touch icon fits. */
export const FAVICON_BYTES_MAX = 64 * 1024

/** How many icon addresses are remembered, so a tab switch or a second tab on one site fetches nothing. */
export const FAVICON_CACHE_MAX = 128

/** Redirect hops followed, each checked against the reach rule. */
export const FAVICON_REDIRECTS_MAX = 3

/** How long one icon may take. A hanging server must not hold a request open for ever. */
export const FAVICON_TIMEOUT_MS = 10_000

/** A `data:` icon a page declared, used as is when it is a small image. */
const DATA_IMAGE = /^data:image\/[a-z0-9.+-]+(;[a-z0-9=._+-]+)*,/i

/** The longest `data:` icon taken: the byte cap once base64 has grown it, plus its header. */
const DATA_ICON_CHARS_MAX = Math.ceil(FAVICON_BYTES_MAX / 3) * 4 + 128

/** A media type an icon may have: an image, by its bare name. */
const IMAGE_TYPE = /^image\/[a-z0-9.+-]+$/

/** The part of Electron's `IncomingMessage` read here. */
export interface IconResponse {
  statusCode: number
  headers: Record<string, string | string[]>
  on(event: 'data', listener: (chunk: Buffer) => void): this
  on(event: 'end', listener: () => void): this
  on(event: 'error', listener: (error: Error) => void): this
  on(event: 'aborted', listener: () => void): this
}

/** The part of Electron's `ClientRequest` used here. */
export interface IconRequest {
  on(
    event: 'redirect',
    listener: (statusCode: number, method: string, redirectUrl: string, responseHeaders: Record<string, string[]>) => void
  ): this
  on(event: 'response', listener: (response: IconResponse) => void): this
  on(event: 'error', listener: (error: Error) => void): this
  on(event: 'abort', listener: () => void): this
  followRedirect(): void
  abort(): void
  end(): void
}

/**
 * Starts a GET for `url` that stops at every redirect: `net.request` with
 * `redirect: 'manual'` on the browser partition's session.
 */
export type IconRequester = (url: string) => IconRequest

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
  request: IconRequester
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

  /** Fetch one icon address, following redirects only to addresses `allows`. */
  const fetchIcon = (url: string, allows: (url: string) => boolean): Promise<string> =>
    new Promise<string>((resolve, reject) => {
      if (!isHttp(url) || !allows(url)) {
        reject(new NoIcon(true))
        return
      }
      const request = options.request(url)
      let settled = false
      const settle = (then: () => void): void => {
        if (settled) return
        settled = true
        clearTimeout(timer)
        then()
      }
      /** No icon. The request is abandoned, so nothing more is read or followed. */
      const fail = (final: boolean): void => {
        if (settled) return
        settle(() => reject(new NoIcon(final)))
        request.abort()
      }
      const timer = setTimeout(() => fail(false), FAVICON_TIMEOUT_MS)

      let hops = 0
      // Followed only from inside this event, and only to an address the
      // tab could go to: not following is how the hop is never asked for.
      request.on('redirect', (_statusCode, _method, redirectUrl) => {
        hops += 1
        if (settled || hops > FAVICON_REDIRECTS_MAX || !isHttp(redirectUrl) || !allows(redirectUrl)) {
          fail(true)
          return
        }
        request.followRedirect()
      })
      request.on('response', (response) => {
        if (settled) return
        if (response.statusCode < 200 || response.statusCode >= 300) {
          fail(true)
          return
        }
        const type = imageType(header(response.headers, 'content-type'))
        const declared = Number(header(response.headers, 'content-length') ?? '0')
        if (type === null || declared > bytesMax) {
          fail(true)
          return
        }
        const chunks: Buffer[] = []
        let total = 0
        response.on('data', (chunk) => {
          if (settled) return
          total += chunk.byteLength
          if (total > bytesMax) {
            fail(true)
            return
          }
          chunks.push(chunk)
        })
        response.on('end', () => {
          if (total === 0) {
            fail(true)
            return
          }
          settle(() => resolve(`data:${type};base64,${Buffer.concat(chunks, total).toString('base64')}`))
        })
        response.on('error', () => fail(false))
        response.on('aborted', () => fail(false))
      })
      request.on('error', () => fail(false))
      request.on('abort', () => fail(false))
      request.end()
    })

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
        // address. A failed connection or a timeout is not: the next report
        // asks again.
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

/** One response header, by its lower-case name, joined if it came more than once. */
function header(headers: Record<string, string | string[]>, name: string): string | null {
  const value = headers[name]
  if (value === undefined) return null
  return Array.isArray(value) ? value.join(', ') : value
}

/** The image type of a `Content-Type`, without parameters, or null for anything else. */
function imageType(value: string | null): string | null {
  if (value === null) return null
  const type = value.split(';')[0]!.trim().toLowerCase()
  return IMAGE_TYPE.test(type) ? type : null
}
