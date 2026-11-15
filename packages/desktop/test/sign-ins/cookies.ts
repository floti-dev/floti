import type { CookieRole } from './report'

/**
 * The cookies the sign-ins fixture's page sets, by role. Only the fixture
 * (`main.fixture.ts`) uses them; `browser-sign-ins.test.ts` imports them only
 * to mask every name and value in what a failure shows. Each value is one no
 * log line would carry by chance, so masking it hides nothing else.
 */
export const COOKIES: Record<CookieRole, { name: string; value: string; path: string; attributes: string }> = {
  kept: { name: 'probe-session', value: 'probe-signed-in', path: '/', attributes: 'HttpOnly; SameSite=Lax' },
  hostPrefixed: { name: '__Host-probe', value: 'probe-host-only', path: '/', attributes: 'Secure' },
  signedOut: { name: 'probe-signed-out', value: 'probe-until-sign-out', path: '/', attributes: '' },
  twinRoot: { name: 'probe-twin', value: 'probe-twin-root', path: '/', attributes: '' },
  twinDeep: { name: 'probe-twin', value: 'probe-twin-deep', path: '/deep', attributes: '' }
}

/** Every cookie name and value above. */
export const COOKIE_SECRETS: readonly string[] = Object.values(COOKIES).flatMap(({ name, value }) => [name, value])
