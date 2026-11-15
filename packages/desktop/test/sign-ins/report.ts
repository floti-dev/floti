/**
 * What the sign-ins fixture (`main.fixture.ts`) reports to
 * `browser-sign-ins.test.ts`. Cookies are named by role; a report carries no
 * cookie's name or value.
 */

/** The browser pane's partition, as `main/browser.ts` names it. */
export const BROWSER_PARTITION_NAME = 'persist:helm-browser'

export type CookieRole = 'kept' | 'hostPrefixed' | 'signedOut'

/** The first run: the user agent the page and its server saw. */
export interface FirstRun {
  appName: string
  /** Whether Electron's own default agent carried its token, so its absence below means something. */
  defaultAgentHadElectron: boolean
  pageAgent: string
  headerAgent: string
  clientHints: string
  brands: Array<{ brand: string; version: string }> | null
  chromeVersion: string
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
