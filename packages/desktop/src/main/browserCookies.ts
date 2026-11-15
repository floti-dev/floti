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

/** How long a flush of the cookie store may hold up a quit, the rewrites it waits for included. */
export const COOKIE_FLUSH_DEADLINE_MS = 2_000

const DAY_SECONDS = 24 * 60 * 60

/** An expiry long past, one second into 1970: a cookie set with it removes the one it matches. */
const FORGOTTEN_EXPIRY = 1

/** The part of Electron's `Cookies` this module uses. */
export type CookieJar = Pick<Cookies, 'on' | 'off' | 'get' | 'set' | 'flushStore'>

/** The part of a `Session` `keepSignIns` configures. */
export type SignInSession = Pick<Session, 'getUserAgent' | 'setUserAgent'> & { readonly cookies: CookieJar }

type ChangeListener = Parameters<Cookies['on']>[1]
type ChangeCause = Parameters<ChangeListener>[2]

/** What `keepSessionCookies` installed. */
export interface CookieKeeper {
  /** Removes the listener and stops waiting on rewrites already sent. */
  stop(): void
  /** Resolves once every rewrite started so far has landed, and been undone where it had to be. */
  settled(): Promise<void>
}

/**
 * Configures a partition for sign-ins: session cookies kept, and a user agent
 * a sign-in page accepts.
 */
export function keepSignIns(partition: SignInSession, appName: string): CookieKeeper {
  partition.setUserAgent(browserUserAgent(partition.getUserAgent(), appName))
  return keepSessionCookies(partition.cookies)
}

/** One change the store announced for a cookie. */
interface Change {
  cookie: Cookie
  cause: ChangeCause
  removed: boolean
}

/** A rewrite sent to the store whose own addition has not been seen yet. */
interface Flight {
  value: string
  expiry: number
  /** Every change to the cookie announced since the rewrite was sent. */
  seen: Change[]
  /** Ends the rewrite's wait: its addition was seen, and whatever it replaced by mistake is back. */
  landed: () => void
}

/**
 * Rewrites every session cookie the jar gains into one that expires
 * `SESSION_COOKIE_DAYS` from now.
 *
 * A rewrite is pending from the moment the cookie arrives until the store has
 * said it still holds that cookie, unchanged and still without an expiry; any
 * change to the cookie in that time cancels it.
 *
 * The store can still change the cookie after it answered and before the
 * rewrite reaches it, and `set` cannot be made conditional. So a rewrite is
 * checked again against the store's announcements, which come in the order it
 * applied the changes. A rewrite that replaced the session cookie it was for
 * is announced as exactly that cookie's removal (cause `overwrite`) followed
 * by the rewrite's addition. Anything else between sending the rewrite and
 * seeing its addition means it landed on something else, and it is undone:
 * when it replaced another cookie (a newer value the site set), that cookie
 * is put back as it was; when it replaced nothing (the site had removed the
 * cookie: a sign-out), it is removed again. So a sign-out is never undone and
 * an older value never replaces a newer one.
 *
 * The rewrite's own announcements start nothing: its removal of the session
 * cookie is a removal, and its addition has an expiry.
 */
export function keepSessionCookies(jar: CookieJar, now: () => number = Date.now): CookieKeeper {
  /** The rewrite pending for each cookie, by identity. Gone once it is sent or cancelled. */
  const pending = new Map<string, symbol>()
  /** The rewrite sent for each cookie, until its addition is seen. */
  const inflight = new Map<string, Flight>()
  /** Every rewrite and undo still running, for `settled`. */
  const work = new Set<Promise<void>>()

  const track = (job: Promise<void>): void => {
    // A cookie that cannot be set again (the store refused it, or the
    // partition is going away) stays as the page left it. There is nothing to
    // say about it that would not name it.
    const quiet = job.catch(() => undefined)
    work.add(quiet)
    void quiet.then(() => work.delete(quiet))
  }

  const rewrite = async (cookie: Cookie, key: string, token: symbol): Promise<void> => {
    const held = await jar.get({ url: cookieUrl(cookie), name: cookie.name })
    if (pending.get(key) !== token) return
    pending.delete(key)
    const current = held.find((found) => cookieKey(found) === key)
    if (current === undefined || current.session !== true || current.value !== cookie.value) return
    const details = keptCookie(current, now())
    let landed: () => void = () => undefined
    const landing = new Promise<void>((resolve) => (landed = resolve))
    inflight.get(key)?.landed()
    inflight.set(key, { value: current.value, expiry: details.expirationDate ?? 0, seen: [], landed })
    try {
      await jar.set(details)
    } catch (err) {
      if (inflight.get(key)?.landed === landed) inflight.delete(key)
      landed()
      throw err
    }
    await landing
  }

  /** Undoes a rewrite that landed on something other than the cookie it was for. */
  const undo = (flight: Flight, ours: Cookie): Promise<void> => {
    const last = flight.seen.at(-1)
    const replaced = last !== undefined && last.removed && last.cause === 'overwrite' ? last.cookie : null
    return jar.set(replaced === null ? forgottenCookie(ours) : restoredCookie(replaced))
  }

  const changed: ChangeListener = (_event, cookie, cause, removed) => {
    const key = cookieKey(cookie)
    // Whatever happened to this cookie, a rewrite still pending for it is of
    // a cookie that is no longer there.
    pending.delete(key)
    const flight = inflight.get(key)
    if (flight !== undefined) {
      if (!removed && isRewriteOf(cookie, flight)) {
        inflight.delete(key)
        if (landedCleanly(flight)) {
          flight.landed()
        } else {
          const undoing = undo(flight, cookie)
          track(undoing)
          void undoing.then(flight.landed, flight.landed)
        }
        return
      }
      flight.seen.push({ cookie, cause, removed })
    }
    if (removed || cookie.session !== true) return
    const token = Symbol('rewrite')
    pending.set(key, token)
    track(rewrite(cookie, key, token))
  }

  jar.on('changed', changed)
  return {
    stop: () => {
      jar.off('changed', changed)
      pending.clear()
      for (const flight of inflight.values()) flight.landed()
      inflight.clear()
    },
    settled: () => Promise.all([...work]).then(() => undefined)
  }
}

/** Whether `cookie` is the addition a rewrite in flight makes. */
function isRewriteOf(cookie: Cookie, flight: Flight): boolean {
  return (
    cookie.session !== true &&
    cookie.value === flight.value &&
    Math.abs((cookie.expirationDate ?? 0) - flight.expiry) < 1
  )
}

/** Whether the one change since the rewrite was sent is its own removal of the session cookie it was for. */
function landedCleanly(flight: Flight): boolean {
  const [only, ...more] = flight.seen
  return (
    only !== undefined &&
    more.length === 0 &&
    only.removed &&
    only.cause === 'overwrite' &&
    only.cookie.session === true &&
    only.cookie.value === flight.value
  )
}

/**
 * Writes the cookie store to disk, once `after` (the rewrites still running)
 * has settled. Chromium otherwise writes it every 30 seconds or 512 changes,
 * so a quit or a crash in between loses the newest sign-in. Never rejects,
 * and gives up after `deadlineMs` in all, so a store that does not answer
 * cannot hold a quit.
 */
export function flushCookies(
  jar: Pick<CookieJar, 'flushStore'>,
  after: Promise<unknown> = Promise.resolve(),
  deadlineMs = COOKIE_FLUSH_DEADLINE_MS
): Promise<void> {
  return new Promise<void>((resolve) => {
    const timer = setTimeout(resolve, deadlineMs)
    const done = (): void => {
      clearTimeout(timer)
      resolve()
    }
    const flush = (): void => {
      let flushing: Promise<void>
      try {
        flushing = jar.flushStore()
      } catch {
        done()
        return
      }
      flushing.then(done, done)
    }
    after.then(flush, flush)
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
 * `cookie` as it is set again: the same in every respect but its expiry
 * (none, for a session cookie).
 *
 * A host-only cookie is set with no domain, which is what makes it host-only
 * again (and what keeps a `__Host-` cookie valid); a domain cookie keeps its
 * domain. A same-site policy of `unspecified` is left out, and Electron sets
 * the cookie as `lax`, the policy Chromium applies to a cookie that names none.
 */
function cookieDetails(cookie: Cookie, expirationDate: number | undefined): CookiesSetDetails {
  const details: CookiesSetDetails = {
    url: cookieUrl(cookie),
    name: cookie.name,
    value: cookie.value,
    path: cookie.path ?? '/',
    secure: cookie.secure === true,
    httpOnly: cookie.httpOnly === true
  }
  if (expirationDate !== undefined) details.expirationDate = expirationDate
  if (cookie.hostOnly !== true && cookie.domain !== undefined) details.domain = cookie.domain
  if (cookie.sameSite !== 'unspecified') details.sameSite = cookie.sameSite
  return details
}

/** A session cookie as it is kept: expiring `SESSION_COOKIE_DAYS` after `nowMs`. */
export function keptCookie(cookie: Cookie, nowMs: number): CookiesSetDetails {
  return cookieDetails(cookie, Math.floor(nowMs / 1000) + SESSION_COOKIE_DAYS * DAY_SECONDS)
}

/** A cookie put back exactly as it was, its expiry or its lack of one included. */
export function restoredCookie(cookie: Cookie): CookiesSetDetails {
  return cookieDetails(cookie, cookie.session === true ? undefined : cookie.expirationDate)
}

/**
 * What removes exactly `cookie` and nothing else: the same name, domain and
 * path, already expired. `cookies.remove(url, name)` would also remove every
 * other cookie of that name sent to that address, on a parent path or domain.
 */
export function forgottenCookie(cookie: Cookie): CookiesSetDetails {
  return { ...cookieDetails(cookie, FORGOTTEN_EXPIRY), value: '' }
}
