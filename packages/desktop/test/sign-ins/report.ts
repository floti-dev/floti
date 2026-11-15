/**
 * What the sign-ins fixture (`main.fixture.ts`) reports to
 * `browser-sign-ins.test.ts`. Cookies are named by role; a report carries no
 * cookie's name or value.
 */

/** The browser pane's partition, as `main/browser.ts` names it. */
export const BROWSER_PARTITION_NAME = 'persist:helm-browser'

/**
 * The cookies the page sets: one kept, one `__Host-` one kept, one the site
 * signs out of, and two of one name on different paths, the deeper of which
 * the fixture removes the way the keeper undoes a rewrite.
 */
export type CookieRole = 'kept' | 'hostPrefixed' | 'signedOut' | 'twinRoot' | 'twinDeep'

/** One change the store announced for the kept cookie, without the cookie. */
export interface ChangeSeen {
  cause: string
  removed: boolean
  session: boolean
}

/** The first run: the user agent the page and its server saw, and how the store announced a rewrite. */
export interface FirstRun {
  appName: string
  /** Whether Electron's own default agent carried its token, so its absence below means something. */
  defaultAgentHadElectron: boolean
  pageAgent: string
  headerAgent: string
  clientHints: string
  brands: Array<{ brand: string; version: string }> | null
  chromeVersion: string
  /** Every change to the kept cookie, from the page setting it to its rewrite. */
  keptChanges: ChangeSeen[]
}

export type CookieFound =
  | { present: false }
  | {
      present: true
      valueMatches: boolean
      session: boolean
      daysLeft: number | null
      hostOnly: boolean
      secure: boolean
      httpOnly: boolean
      sameSite: string
    }

/** The second run: what the partition held after the restart. */
export interface SecondRun {
  cookies: Record<CookieRole, CookieFound>
}

/** A run that ended other than by finishing: the reason, or the error. */
export interface Ended {
  ended?: 'owner'
  error?: string
}
