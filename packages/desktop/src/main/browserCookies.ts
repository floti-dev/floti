import type { Cookie, Cookies, CookiesSetDetails, Session } from 'electron'

/**
 * Sign-ins that survive a restart, in the browser partition.
 *
 * Most sign-ins are a cookie set without an expiry: a session cookie, which
 * Chromium drops when the browser closes. Chrome keeps them across a restart
 * when "Continue where you left off" is on; Electron has no such switch, so
 * this module does what it does: every session cookie a page sets in the
 * partition is set again, the same in every other respect, with an expiry
 * `SESSION_COOKIE_DAYS` after it was set. A site that sets it again re-arms
 * the days; a site that removes it (signing out) removes it for good.
 *
 * Cookies are handled here and nowhere else. Nothing in this module logs,
 * prints or returns a cookie's name or value, and nothing here reaches the
 * renderer, the host or an agent tool.
 */

/** How long a kept session cookie lives after a page last set it. */
export const SESSION_COOKIE_DAYS = 30

/** How long a flush of the cookie store may hold up a quit. */
export const COOKIE_FLUSH_DEADLINE_MS = 2_000

const DAY_SECONDS = 24 * 60 * 60

/** The part of Electron's `Cookies` this module uses. */
export type CookieJar = Pick<Cookies, 'on' | 'off' | 'get' | 'set' | 'flushStore'>

/** The part of a `Session` `keepSignIns` configures. */
export type SignInSession = Pick<Session, 'getUserAgent' | 'setUserAgent'> & { readonly cookies: CookieJar }

type ChangeListener = Parameters<Cookies['on']>[1]

/**
 * Configures a partition for sign-ins: session cookies kept, and a user agent
 * a sign-in page accepts. Returns what undoes the cookie half.
 */
export function keepSignIns(partition: SignInSession, appName: string): () => void {
  partition.setUserAgent(browserUserAgent(partition.getUserAgent(), appName))
  return keepSessionCookies(partition.cookies)
}

/**
 * Rewrites every session cookie the jar gains into one that expires
 * `SESSION_COOKIE_DAYS` from now. Returns what removes the listener.
 *
 * A rewrite is pending from the moment the cookie arrives until the store has
 * said it still holds that cookie, unchanged and still without an expiry.
 * Any change to the same cookie in that time - a removal, a newer value -
 * cancels it, so a sign-out that lands right after a sign-in is never undone,
 * and an older value never replaces a newer one. The rewrite is seen again
 * here, as the removal of the session cookie (cause `overwrite`) and the
 * addition of one with an expiry; neither is a session cookie being added, so
 * neither starts anything.
 */
export function keepSessionCookies(jar: CookieJar, now: () => number = Date.now): () => void {
  /** The rewrite pending for each cookie, by identity. Gone once it is settled. */
  const pending = new Map<string, symbol>()

  const rewrite = async (cookie: Cookie, key: string, token: symbol): Promise<void> => {
    const url = cookieUrl(cookie)
    const held = await jar.get({ url, name: cookie.name })
    if (pending.get(key) !== token) return
    const current = held.find((found) => cookieKey(found) === key)
    if (current === undefined || current.session !== true || current.value !== cookie.value) {
      pending.delete(key)
      return
    }
    pending.delete(key)
    await jar.set(keptCookie(current, now()))
  }

  const changed: ChangeListener = (_event, cookie, _cause, removed) => {
    const key = cookieKey(cookie)
    // Whatever happened to this cookie, a rewrite still pending for it is of
    // a cookie that is no longer there.
    pending.delete(key)
    if (removed || cookie.session !== true) return
    const token = Symbol('rewrite')
    pending.set(key, token)
    // A cookie that cannot be set again (the store refused it, or the
    // partition is going away) stays the session cookie the page set. There
    // is nothing to say about it that would not name it.
    rewrite(cookie, key, token).catch(() => {
      if (pending.get(key) === token) pending.delete(key)
    })
  }

  jar.on('changed', changed)
  return () => {
    jar.off('changed', changed)
    pending.clear()
  }
}

/**
 * Writes the cookie store to disk now. Chromium otherwise writes it every 30
 * seconds or 512 changes, so a quit or a crash in between loses the newest
 * sign-in. Never rejects, and gives up after `deadlineMs` so a store that
 * does not answer cannot hold a quit.
 */
export function flushCookies(jar: Pick<CookieJar, 'flushStore'>, deadlineMs = COOKIE_FLUSH_DEADLINE_MS): Promise<void> {
  return new Promise<void>((resolve) => {
    const timer = setTimeout(resolve, deadlineMs)
    const done = (): void => {
      clearTimeout(timer)
      resolve()
    }
    let flushing: Promise<void>
    try {
      flushing = jar.flushStore()
    } catch {
      done()
      return
    }
    flushing.then(done, done)
  })
}

/**
 * The user agent the browser partition sends: Electron's own without the
 * `Electron/<version>` token and the app's `<name>/<version>` token, which
 * Google's sign-in refuses. Everything else, the Chrome version included, is
 * Electron's. Electron writes the app's name with its whitespace removed, so
 * the name is matched that way.
 */
export function browserUserAgent(defaultAgent: string, appName: string): string {
  const name = appName.replace(/\s+/g, '')
  const tokens = defaultAgent.split(' ').filter((token) => token !== '')
  const kept = tokens.filter((token) => !isProductToken(token, 'Electron') && !(name !== '' && isProductToken(token, name)))
  return kept.join(' ')
}

/** Whether `token` is `<product>/<version>`. */
function isProductToken(token: string, product: string): boolean {
  return token.startsWith(`${product}/`) && token.length > product.length + 1
}

/** What Chromium tells cookies apart by: name, domain and path. */
function cookieKey(cookie: Pick<Cookie, 'name' | 'domain' | 'path'>): string {
  return `${cookie.domain ?? ''}\u0000${cookie.path ?? '/'}\u0000${cookie.name}`
}

/** The address a cookie is set for: its scheme, its domain without the leading dot, its path. */
export function cookieUrl(cookie: Pick<Cookie, 'secure' | 'domain' | 'path'>): string {
  const host = (cookie.domain ?? '').replace(/^\./, '')
  return `${cookie.secure === true ? 'https' : 'http'}://${host}${cookie.path ?? '/'}`
}

/**
 * `cookie` as it is set again: the same in every respect but its expiry.
 *
 * A host-only cookie is set with no domain, which is what makes it host-only
 * again (and what keeps a `__Host-` cookie valid); a domain cookie keeps its
 * domain. A same-site policy of `unspecified` is left out, and Electron sets
 * the cookie as `lax`, the policy Chromium applies to a cookie that names none.
 */
export function keptCookie(cookie: Cookie, nowMs: number): CookiesSetDetails {
  const details: CookiesSetDetails = {
    url: cookieUrl(cookie),
    name: cookie.name,
    value: cookie.value,
    path: cookie.path ?? '/',
    secure: cookie.secure === true,
    httpOnly: cookie.httpOnly === true,
    expirationDate: Math.floor(nowMs / 1000) + SESSION_COOKIE_DAYS * DAY_SECONDS
  }
  if (cookie.hostOnly !== true && cookie.domain !== undefined) details.domain = cookie.domain
  if (cookie.sameSite !== 'unspecified') details.sameSite = cookie.sameSite
  return details
}
