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
 * A test calls `endFixtures()` after each test: every process started here, a
 * fixture, its display server or a stand-in, ends with the test that started
 * it.
 *
 * On Linux with no display (a CI runner), each fixture gets a private Xvfb,
 * which picks a free display number itself (`-displayfd`), so fixtures running
 * side by side never race for one; it ends when its fixture does. Chromium's
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
 * without one a private Xvfb, ready once it has written its display number to
 * fd 3. Rejects naming Xvfb, with its stderr, if it cannot start.
 */
function displayFor(label: string): Promise<Display> {
  if (hasDisplay()) return Promise.resolve({ env: {}, server: null })
  return new Promise((resolve, reject) => {
    const server = spawn('Xvfb', ['-displayfd', '3', '-nolisten', 'tcp', '-screen', '0', '1280x800x24'], {
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
    const failed = (error: Error): void => fail(`did not start (${error.message}; it is in the xvfb package)`)
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
      child.once('error', (error) => {
        clearTimeout(timer)
        display.server?.kill()
        reject(error)
      })
      child.once('exit', (code, signal) => {
        clearTimeout(timer)
        // Its display ends with it; SIGTERM lets Xvfb remove its lock file.
        display.server?.kill()
        resolve({ code, signal })
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
