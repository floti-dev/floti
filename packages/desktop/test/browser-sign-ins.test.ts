import { spawn } from 'node:child_process'
import { mkdirSync, mkdtempSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { afterEach, describe, expect, it } from 'vitest'
import { PATIENCE } from '../../../test/patience'
import { bundleFixture, endFixtures, reportOf, startFixture, track, writtenReport } from './electron-fixture'
import { COOKIE_SECRETS } from './sign-ins/cookies'
import type { FirstRun, SecondRun } from './sign-ins/report'

/**
 * Sign-ins across a restart, in real Electron.
 *
 * The fixture (`sign-ins/main.fixture.ts`) is bundled with the shipped
 * `main/browserCookies.ts` and run by the Electron binary from
 * `node_modules` (`electron-fixture.ts`), twice, on one user-data folder
 * under the run's root: the first run signs in on a loopback page, the second
 * reads the partition back. Its windows are hidden, and every Electron
 * process ends with the test: the fixture exits once the process that owns it
 * is gone, which the last test shows. A failure shows the fixture's stderr
 * with every cookie's name and value masked.
 */

const HERE = dirname(fileURLToPath(import.meta.url))
/** The fixture's app name: one with a space, written without it in the user agent. */
const APP_NAME = 'Floti Fixture'

afterEach(endFixtures)

const fixtureApp = (root: string): Promise<string> =>
  bundleFixture(root, join(HERE, 'sign-ins', 'main.fixture.ts'), 'sign-ins-fixture', APP_NAME)

type Mode = 'first' | 'second' | 'linger'

/**
 * Starts the fixture, owned by `owner` (this process unless a test says
 * otherwise): the fixture ends once its owner is gone, however it ends, and
 * ends itself after the patience in any case.
 */
const start = (app: string, userData: string, mode: Mode, result: string, owner: number = process.pid) =>
  startFixture(
    app,
    userData,
    { SIGN_INS_MODE: mode, SIGN_INS_RESULT: result, SIGN_INS_OWNER: String(owner) },
    `${mode} run`,
    COOKIE_SECRETS
  )

/** Runs the fixture to its end and returns its report. */
function run<T>(app: string, userData: string, mode: 'first' | 'second', root: string): Promise<T> {
  const result = join(root, `${mode}.json`)
  return reportOf<T>(start(app, userData, mode, result), result)
}

describe('sign-ins in the browser partition, in real Electron', () => {
  it(
    'keeps a session cookie across a restart, 30 days out, and a signed-out one stays gone',
    async () => {
      const root = mkdtempSync(join(tmpdir(), 'sign ins '))
      const userData = join(root, 'user data')
      mkdirSync(userData)
      const app = await fixtureApp(root)

      const signedIn = await run<FirstRun>(app, userData, 'first', root)
      const restarted = await run<SecondRun>(app, userData, 'second', root)

      for (const role of ['kept', 'hostPrefixed'] as const) {
        const found = restarted.cookies[role]
        expect(found, role).toMatchObject({ present: true, valueMatches: true, session: false })
        if (!found.present || found.daysLeft === null) throw new Error(`${role} has no expiry`)
        expect(found.daysLeft, role).toBeGreaterThan(29.9)
        expect(found.daysLeft, role).toBeLessThanOrEqual(30)
      }
      expect(restarted.cookies.kept).toMatchObject({ httpOnly: true, sameSite: 'lax', hostOnly: true })
      // Set with no same-site policy, which Electron's `cookies.set` keeps as `lax`.
      expect(restarted.cookies.hostPrefixed).toMatchObject({ secure: true, hostOnly: true, sameSite: 'lax' })
      expect(restarted.cookies.signedOut).toEqual({ present: false })
      // Removed as the keeper undoes a rewrite: exactly that cookie, and not
      // the one of the same name on the parent path.
      expect(restarted.cookies.twinDeep).toEqual({ present: false })
      expect(restarted.cookies.twinRoot).toMatchObject({ present: true, valueMatches: true, session: false })

      // How the store announced the page's cookie and its rewrite, which is
      // what the keeper checks a rewrite against.
      expect(signedIn.keptChanges).toEqual([
        { cause: 'inserted', removed: false, session: true },
        { cause: 'overwrite', removed: true, session: true },
        { cause: 'inserted-no-value-change-overwrite', removed: false, session: false }
      ])

      // The user agent, as the page and its server saw it.
      expect(signedIn.appName).toBe(APP_NAME)
      expect(signedIn.defaultAgentHadElectron).toBe(true)
      expect(signedIn.headerAgent).toBe(signedIn.pageAgent)
      expect(signedIn.pageAgent).not.toMatch(/Electron\//)
      expect(signedIn.pageAgent).not.toContain('FlotiFixture')
      expect(signedIn.pageAgent).toContain(`Chrome/${signedIn.chromeVersion}`)
    },
    // Two Electron starts, each bounded by the patience, and a bundle.
    3 * PATIENCE
  )

  it(
    'ends the fixture once the process that owns it is gone',
    async () => {
      const root = mkdtempSync(join(tmpdir(), 'sign ins '))
      const userData = join(root, 'user data')
      mkdirSync(userData)
      const app = await fixtureApp(root)
      const result = join(root, 'linger.json')
      // A stand-in for the test's worker: a process that runs until it is ended.
      const owner = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1 << 30)'], { stdio: 'ignore' })
      track(owner)
      if (owner.pid === undefined) throw new Error('the stand-in owner did not start')
      const fixture = start(app, userData, 'linger', result, owner.pid)
      // Lingering, the fixture writes nothing until it ends.
      owner.kill()
      expect(await writtenReport(fixture, result, 1)).toEqual({ ended: 'owner' })
    },
    2 * PATIENCE
  )
})
