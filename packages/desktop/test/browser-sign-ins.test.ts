import { spawn, type ChildProcess } from 'node:child_process'
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { build } from 'esbuild'
import { afterEach, describe, expect, it } from 'vitest'
import { PATIENCE } from '../../../test/patience'
import { treeKill } from '../src/main/treekill'
import type { FirstRun, SecondRun } from './sign-ins/report'

/**
 * Sign-ins across a restart, in real Electron.
 *
 * The fixture (`sign-ins/main.fixture.ts`) is bundled with the shipped
 * `main/browserCookies.ts` and run by the Electron binary from
 * `node_modules`, twice, on one user-data folder under the run's root: the
 * first run signs in on a loopback page, the second reads the partition back.
 * Its windows are hidden, and every Electron process ends with the test.
 */

const HERE = dirname(fileURLToPath(import.meta.url))
const ELECTRON = createRequire(import.meta.url)('electron') as string
/** The fixture's app name: one with a space, written without it in the user agent. */
const APP_NAME = 'Floti Fixture'

const running = new Set<ChildProcess>()

afterEach(() => {
  for (const child of running) if (child.pid !== undefined) treeKill(child.pid, true)
  running.clear()
})

/** The fixture bundled into an app folder of its own, with the app name above. */
async function fixtureApp(root: string): Promise<string> {
  const app = join(root, 'app')
  mkdirSync(app)
  await build({
    entryPoints: [join(HERE, 'sign-ins', 'main.fixture.ts')],
    outfile: join(app, 'main.cjs'),
    bundle: true,
    platform: 'node',
    format: 'cjs',
    target: 'node22',
    external: ['electron'],
    logLevel: 'silent'
  })
  writeFileSync(
    join(app, 'package.json'),
    JSON.stringify({ name: 'sign-ins-fixture', productName: APP_NAME, version: '9.9.9', main: 'main.cjs' })
  )
  return app
}

/** Runs the fixture once and returns its report, bounded by the shared patience. */
async function run<T>(app: string, userData: string, mode: 'first' | 'second', root: string): Promise<T> {
  const result = join(root, `${mode}.json`)
  const env: NodeJS.ProcessEnv = { ...process.env, SIGN_INS_MODE: mode, SIGN_INS_RESULT: result }
  delete env['ELECTRON_RUN_AS_NODE']
  const child = spawn(ELECTRON, [app, `--user-data-dir=${userData}`], { env, stdio: ['ignore', 'ignore', 'pipe'] })
  running.add(child)
  let stderr = ''
  child.stderr?.on('data', (chunk: Buffer) => (stderr += chunk.toString()))
  const code = await new Promise<number | null>((resolve, reject) => {
    const timer = setTimeout(() => {
      reject(new Error(`the ${mode} run did not exit within ${String(PATIENCE)} ms\n${stderr}`))
    }, PATIENCE)
    child.once('error', (error) => {
      clearTimeout(timer)
      reject(error)
    })
    child.once('exit', (exitCode) => {
      clearTimeout(timer)
      resolve(exitCode)
    })
  })
  running.delete(child)
  const report = JSON.parse(readFileSync(result, 'utf8')) as T | { error: string }
  if (typeof report === 'object' && report !== null && 'error' in report) {
    throw new Error(`the ${mode} run failed: ${report.error}\n${stderr}`)
  }
  expect(code, `the ${mode} run's exit code`).toBe(0)
  return report as T
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
})
