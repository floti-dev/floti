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
import type { FaviconsReport } from './favicons/report'

/**
 * Icons fetched over Electron's own network stack, in real Electron.
 *
 * What a fake cannot show is how Electron's `net.request` behaves at a
 * redirect with `redirect: 'manual'`, and that behaviour is the whole of the
 * reach check on a redirect hop. So the fixture (`favicons/main.fixture.ts`)
 * runs the shipped `main/favicons.ts` on the Electron binary from
 * `node_modules`, hidden, with a user-data folder under the run's root,
 * against loopback servers, and reports what each server was asked for.
 */

const HERE = dirname(fileURLToPath(import.meta.url))
const ELECTRON = createRequire(import.meta.url)('electron') as string

const running = new Set<ChildProcess>()

afterEach(() => {
  for (const child of running) if (child.pid !== undefined) treeKill(child.pid, true)
  running.clear()
})

/** The fixture bundled into an app folder of its own. */
async function fixtureApp(root: string): Promise<string> {
  const app = join(root, 'app')
  mkdirSync(app)
  await build({
    entryPoints: [join(HERE, 'favicons', 'main.fixture.ts')],
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
    JSON.stringify({ name: 'favicons-fixture', productName: 'Favicons Fixture', version: '9.9.9', main: 'main.cjs' })
  )
  return app
}

/** Runs the fixture to its end, owned by this process, and returns its report. */
async function run(app: string, userData: string, root: string): Promise<FaviconsReport> {
  const result = join(root, 'favicons.json')
  const env: NodeJS.ProcessEnv = {
    ...process.env,
    FAVICONS_RESULT: result,
    FAVICONS_OWNER: String(process.pid)
  }
  delete env['ELECTRON_RUN_AS_NODE']
  const child = spawn(ELECTRON, [app, `--user-data-dir=${userData}`], { env, stdio: ['ignore', 'ignore', 'pipe'] })
  running.add(child)
  let stderr = ''
  child.stderr?.on('data', (chunk: Buffer) => (stderr += chunk.toString()))
  const code = await new Promise<number | null>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`the fixture did not exit within ${String(PATIENCE)} ms\n${stderr}`)), PATIENCE)
    child.once('error', (error) => {
      clearTimeout(timer)
      reject(error)
    })
    child.once('exit', (exitCode) => {
      clearTimeout(timer)
      running.delete(child)
      resolve(exitCode)
    })
  })
  const report = JSON.parse(readFileSync(result, 'utf8')) as FaviconsReport | { error?: string; ended?: string }
  if ('error' in report || 'ended' in report) throw new Error(`the fixture did not finish: ${JSON.stringify(report)}`)
  expect(code, "the fixture's exit code").toBe(0)
  return report as FaviconsReport
}

describe('favicons over Electron’s network, in real Electron', () => {
  it(
    'follows a redirect to an address in reach, and never asks for one out of it',
    async () => {
      const root = mkdtempSync(join(tmpdir(), 'favicons '))
      const userData = join(root, 'user data')
      mkdirSync(userData)
      const report = await run(await fixtureApp(root), userData, root)

      expect(report.direct).toBe(report.expected)
      // The hop was checked and followed: the second server was asked.
      expect(report.redirected).toBe(report.expected)
      expect(report.alsoAsked).toEqual(['/icon.png'])
      // The hop out of reach was never asked for.
      expect(report.refused).toBeNull()
      expect(report.farAsked).toEqual([])
      expect(report.notImage).toBeNull()
      expect(report.nearAsked).toEqual(['/icon.png', '/favicon.ico', '/away.ico', '/page.html'])
    },
    // One Electron start, bounded by the patience, and a bundle.
    2 * PATIENCE
  )
})
