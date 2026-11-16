import { spawn, type ChildProcess } from 'node:child_process'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { join } from 'node:path'
import type { Readable } from 'node:stream'
import { build } from 'esbuild'
import { PATIENCE } from '../../../test/patience'
import { treeKill } from '../src/main/treekill'

/**
 * Runs a fixture main script on the real Electron binary from `node_modules`,
 * for the desktop tests that need what only Chromium can show
 * (`browser-sign-ins.test.ts`, `browser-favicons.test.ts`).
 *
 * The fixture is bundled with the shipped modules it imports into an app
 * folder under the run's root and started with a user-data folder there too.
 * It writes a JSON report and exits; when it does not, the error names its
 * exit code or signal and ends with its stderr, so a failure on a runner says
 * why Electron did not get as far as the report.
 *
 * Every process started here ends with the test that started it, even when
 * the test's worker dies: a fixture watches its owner, and a fixture's display
 * server is watched by a keeper of its own (below). A test also calls
 * `endFixtures()` after each test, which ends whatever is still running.
 *
 * On Linux with no display (a CI runner), each fixture gets a private Xvfb,
 * which picks a free display number itself (`-displayfd`), so fixtures running
 * side by side never race for one. It runs with `-terminate`, so it exits
 * once its last client, the fixture, disconnects, and under `XVFB_KEEPER`,
 * which ends it once the test's worker is gone, as the fixture ends with its
 * owner: that covers the moment between Xvfb being ready and the fixture
 * connecting, and a fixture that dies before it connects. Chromium's
 * headless Ozone platform is no substitute: Electron 43 crashes on it once a
 * window loads a page. The OS sandbox stays on, as Floti's does: where the OS
 * refuses it, Electron aborts at start and the error shows Chromium's own line
 * saying so (docs/TESTING.md has the fix; the Linux CI job applies it).
 */

const ELECTRON = createRequire(import.meta.url)('electron') as string

/** How much of a fixture's stderr a failure shows: its end, where a fatal error is. */
const STDERR_TAIL = 16_384

const running = new Set<ChildProcess>()

/** Keeps `child` to be ended by `endFixtures`, until it exits. */
export function track(child: ChildProcess): void {
  running.add(child)
  child.once('exit', () => running.delete(child))
}

/** Ends every process started here that is still running, with what it started. */
export function endFixtures(): void {
  for (const child of running) if (child.pid !== undefined) treeKill(child.pid, true)
  running.clear()
}

/** `entry` bundled into `<root>/app`, an app folder Electron can start, named `productName`. */
export async function bundleFixture(root: string, entry: string, name: string, productName: string): Promise<string> {
  const app = join(root, 'app')
  mkdirSync(app)
  await build({
    entryPoints: [entry],
    outfile: join(app, 'main.cjs'),
    bundle: true,
    platform: 'node',
    format: 'cjs',
    target: 'node22',
    external: ['electron'],
    logLevel: 'silent'
  })
  writeFileSync(join(app, 'package.json'), JSON.stringify({ name, productName, version: '9.9.9', main: 'main.cjs' }))
  return app
}

/** Appends `chunk` to `text`, keeping the last `STDERR_TAIL` characters. */
const tailOf = (text: string, chunk: Buffer): string => (text + chunk.toString()).slice(-STDERR_TAIL)

/**
 * Runs Xvfb with the arguments after the owner's pid, passing fd 3 through for
 * `-displayfd`, and exits when Xvfb does. It ends Xvfb with SIGTERM, which
 * lets Xvfb remove its lock file, when it is sent SIGTERM itself or once the
 * owner is gone, polled as the fixtures poll theirs (signal 0 sends nothing;
 * EPERM means the owner runs as somebody else).
 */
const XVFB_KEEPER = `
const { spawn } = require('node:child_process')
const rest = process.argv.slice(1)
if (rest[0] === '--') rest.shift()
const [owner, ...args] = rest
const server = spawn('Xvfb', args, { stdio: ['ignore', 'ignore', 'inherit', 3] })
server.on('error', (error) => {
  console.error(String(error) + ' (Xvfb is in the xvfb package)')
  process.exit(127)
})
server.on('exit', (code, signal) => process.exit(code ?? (signal === null ? 0 : 1)))
const end = () => server.kill('SIGTERM')
process.on('SIGTERM', end)
setInterval(() => {
  try {
    process.kill(Number(owner), 0)
  } catch (error) {
    if (error.code !== 'EPERM') end()
  }
}, 250)
`

/** Settles once `child` has exited, at once if it already has, or after the patience. */
function exitOf(child: ChildProcess): Promise<void> {
  if (child.exitCode !== null || child.signalCode !== null) return Promise.resolve()
  return new Promise((resolve) => {
    const timer = setTimeout(resolve, PATIENCE)
    child.once('exit', () => {
      clearTimeout(timer)
      resolve()
    })
  })
}

/** What the fixture's environment needs for a display, and the server started for it, if any. */
interface Display {
  env: Record<string, string>
  server: ChildProcess | null
}

/** Whether Electron on this computer has a display to connect to. */
const hasDisplay = (): boolean =>
  process.platform !== 'linux' || Boolean(process.env['DISPLAY']) || Boolean(process.env['WAYLAND_DISPLAY'])

/**
 * A display for one fixture: nothing where there is one already, and on Linux
 * without one a private Xvfb under its keeper, ready once Xvfb has written its
 * display number to fd 3. Rejects naming Xvfb, with its stderr, if it cannot
 * start.
 */
function displayFor(label: string): Promise<Display> {
  if (hasDisplay()) return Promise.resolve({ env: {}, server: null })
  return new Promise((resolve, reject) => {
    const xvfb = ['-displayfd', '3', '-terminate', '-nolisten', 'tcp', '-screen', '0', '1280x800x24']
    const server = spawn(process.execPath, ['-e', XVFB_KEEPER, '--', String(process.pid), ...xvfb], {
      stdio: ['ignore', 'ignore', 'pipe', 'pipe']
    })
    track(server)
    let stderr = ''
    server.stderr?.on('data', (chunk: Buffer) => (stderr = tailOf(stderr, chunk)))
    const fail = (why: string): void => {
      clearTimeout(timer)
      if (server.pid !== undefined) treeKill(server.pid, true)
      reject(new Error(`the ${label} has no display, and Xvfb ${why}\n--- its stderr ---\n${stderr.trimEnd()}`))
    }
    const timer = setTimeout(() => fail(`gave no display within ${String(PATIENCE)} ms`), PATIENCE)
    const failed = (error: Error): void => fail(`did not start (${error.message})`)
    const exited = (code: number | null, signal: NodeJS.Signals | null): void =>
      fail(`exited (${signal ?? `exit code ${String(code)}`})`)
    server.once('error', failed)
    server.once('exit', exited)
    let number = ''
    ;(server.stdio[3] as Readable).on('data', (chunk: Buffer) => {
      number += chunk.toString()
      if (!number.includes('\n')) return
      clearTimeout(timer)
      server.off('error', failed)
      server.off('exit', exited)
      resolve({ env: { DISPLAY: `:${number.trim()}` }, server })
    })
  })
}

export interface FixtureExit {
  code: number | null
  signal: NodeJS.Signals | null
}

export interface Fixture {
  /** Settles when the fixture exits; rejects if it could not start or has not exited within the patience. */
  readonly exited: Promise<FixtureExit>
  /**
   * An error saying `what`, how the fixture ended and the end of its stderr,
   * with every string in `mask` replaced throughout. Waits for the stderr to
   * close, up to the patience.
   */
  failure(what: string, exit?: FixtureExit): Promise<Error>
}

/**
 * Starts the fixture in `app` on `userData` with `env` added to this
 * process's environment, once it has a display. `label` names it in errors.
 * Every string in `mask` (a cookie's name or value, say) is replaced in every
 * error it gives.
 */
export function startFixture(
  app: string,
  userData: string,
  env: Record<string, string>,
  label: string,
  mask: readonly string[] = []
): Fixture {
  let stderr = ''
  let closed: Promise<void> = Promise.resolve()
  // Longest first, so a secret containing another is masked whole.
  const secrets = [...mask].filter((secret) => secret !== '').sort((a, b) => b.length - a.length)
  const masked = (text: string): string => secrets.reduce((out, secret) => out.split(secret).join('<masked>'), text)

  const describe = (exit: FixtureExit | undefined): string => {
    const how =
      exit === undefined
        ? 'still running'
        : exit.signal !== null
          ? `killed by ${exit.signal}`
          : `exit code ${String(exit.code)}`
    const tail = stderr.trimEnd()
    const cut = stderr.length >= STDERR_TAIL ? ` (last ${String(STDERR_TAIL)} characters)` : ''
    return `${how}\n--- its stderr${cut} ---\n${tail === '' ? '(empty)' : tail}`
  }

  const failure = async (what: string, exit?: FixtureExit): Promise<Error> => {
    let timer: NodeJS.Timeout | undefined
    await Promise.race([closed, new Promise<void>((resolve) => (timer = setTimeout(resolve, PATIENCE)))])
    clearTimeout(timer)
    return new Error(masked(`the ${label} ${what}: ${describe(exit)}`))
  }

  const run = (display: Display): Promise<FixtureExit> =>
    new Promise((resolve, reject) => {
      const environment: NodeJS.ProcessEnv = { ...process.env, ...env, ...display.env }
      delete environment['ELECTRON_RUN_AS_NODE']
      const child = spawn(ELECTRON, [app, `--user-data-dir=${userData}`], {
        env: environment,
        stdio: ['ignore', 'ignore', 'pipe']
      })
      track(child)
      child.stderr?.on('data', (chunk: Buffer) => (stderr = tailOf(stderr, chunk)))
      closed = new Promise<void>((settle) => child.once('close', () => settle()))
      const timer = setTimeout(() => {
        void failure(`did not exit within ${String(PATIENCE)} ms`).then(reject)
      }, PATIENCE)
      // Its display ends with it, before it counts as ended, so `endFixtures`
      // finds nothing to kill and Xvfb has removed its lock file.
      const endDisplay = (): Promise<void> => {
        if (display.server === null) return Promise.resolve()
        display.server.kill('SIGTERM')
        return exitOf(display.server)
      }
      child.once('error', (error) => {
        clearTimeout(timer)
        void endDisplay().then(() => reject(error))
      })
      child.once('exit', (code, signal) => {
        clearTimeout(timer)
        void endDisplay().then(() => resolve({ code, signal }))
      })
    })

  return { exited: displayFor(label).then(run), failure }
}

/**
 * What the fixture wrote at `path`, once it has exited with `expectedCode`. A
 * missing or unreadable report, or another exit code, throws the fixture's
 * `failure`.
 */
export async function writtenReport(fixture: Fixture, path: string, expectedCode: number): Promise<unknown> {
  const exit = await fixture.exited
  let report: unknown
  try {
    report = JSON.parse(readFileSync(path, 'utf8'))
  } catch (error) {
    throw await fixture.failure(`wrote no report (${error instanceof Error ? error.message : String(error)})`, exit)
  }
  if (exit.code !== expectedCode) {
    throw await fixture.failure(`exited without ${String(expectedCode)}, reporting ${JSON.stringify(report)}`, exit)
  }
  return report
}

/**
 * The fixture's report at `path` once it has exited with 0: anything
 * `writtenReport` refuses, or a report of `{ error }` or `{ ended }`, throws
 * the fixture's `failure`.
 */
export async function reportOf<T>(fixture: Fixture, path: string): Promise<T> {
  const report = await writtenReport(fixture, path, 0)
  if (typeof report === 'object' && report !== null && ('error' in report || 'ended' in report)) {
    throw await fixture.failure(`did not finish: ${JSON.stringify(report)}`, await fixture.exited)
  }
  return report as T
}
