import { spawn, spawnSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { mkdtemp } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { afterEach, describe, expect, inject, it } from 'vitest'
import { PATIENCE } from '../../../../../test/patience'
import { RUN_ROOT } from '../../../../../test/run-root'
import {
  FLOTI,
  FLOTI_PLACES,
  endStartedProcesses,
  flotiAvailable,
  startedProcesses,
  stopFlotiHost
} from '../../../test/floti-host'
import { hostEnvironment, startLocalHost, type HostPlaces, type LauncherDeps } from './launcher'

/**
 * The host this Floti starts on Windows has no console, so no window opens
 * for it and none can be closed to stop it: `startLocalHost`'s own spawn
 * options, starting the real `floti` this checkout built in this test's own
 * folders.
 *
 * Whether a process has a console is asked from outside it by
 * `floti-fake-gui --console-of=<pid>`, built beside `floti`: a program with
 * no console of its own, which asks Windows to attach it to that process's
 * console. It exits 0 for a console, hidden or not, 1 for none. A host
 * started with `windowsHide` alone (`CREATE_NO_WINDOW`) has a hidden one,
 * which the probe is shown to find first.
 */

const PROBE = join(dirname(FLOTI), 'floti-fake-gui.exe')
const runs = process.platform === 'win32' && flotiAvailable && existsSync(PROBE)

afterEach(endStartedProcesses)

/** Whether process `pid` has a console, as the probe says. */
function hasConsole(pid: number): boolean {
  const probed = spawnSync(PROBE, [`--console-of=${pid}`], { windowsHide: true, timeout: PATIENCE })
  if (probed.status === 0) return true
  if (probed.status === 1) return false
  throw new Error(`the probe could not tell whether process ${pid} has a console: ${probed.stderr.toString()}`)
}

async function hostWorld(label: string) {
  const root = await mkdtemp(join(inject(RUN_ROOT), `${label} `))
  const places: HostPlaces = {
    ...FLOTI_PLACES,
    dataDir: join(root, 'helm-data'),
    templatesDir: join(root, 'config', 'templates')
  }
  const programs = { platform: process.platform, env: process.env, tmpdir, exists: existsSync }
  /** Each host Helm's start path started, with when it exits. */
  const hosts: Array<{ pid: number | undefined; exited: Promise<void> }> = []
  const deps: LauncherDeps = {
    ...programs,
    spawn: (command, args, options) => {
      const child = spawn(command, args, options)
      startedProcesses.push(child)
      if (args[0] === 'host') {
        hosts.push({ pid: child.pid, exited: new Promise((done) => child.once('exit', () => done())) })
      }
      return child
    },
    appVersion: '0.0.0',
    log: () => undefined,
    setTimeout: (run, ms) => setTimeout(run, ms),
    clearTimeout: (timer) => clearTimeout(timer as NodeJS.Timeout)
  }
  return { places, env: hostEnvironment(places, programs), deps, hosts }
}

describe.runIf(runs)('the host this Floti starts on Windows', { timeout: PATIENCE * 3 }, () => {
  it('has no console', async () => {
    const world = await hostWorld('no console')
    const local = await startLocalHost(world.places, world.deps)
    expect(local.startedHost).toBe(true)
    expect(world.hosts).toHaveLength(1)
    const host = world.hosts[0]!
    expect(hasConsole(host.pid!)).toBe(false)

    // A deliberate stop ends the keeper before the host exits, so this test
    // leaves nothing running.
    await local.connection.request('host:stop', { reason: 'stop' })
    await host.exited
    await local.close()
  })

  it('would have a hidden console if it were started with its window hidden alone, which the probe finds', async () => {
    const world = await hostWorld('hidden console')
    const child = spawn(FLOTI, ['host'], { env: world.env, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true })
    startedProcesses.push(child)
    const exited = new Promise<void>((done) => child.once('exit', () => done()))
    let said = ''
    await new Promise<void>((done, fail) => {
      child.stdout.on('data', (chunk: Buffer) => {
        said += chunk.toString()
        if (said.includes('Floti host is listening at ')) done()
      })
      child.once('error', fail)
      void exited.then(() => fail(new Error(`floti host exited before listening: ${said}`)))
    })
    expect(hasConsole(child.pid!)).toBe(true)
    await stopFlotiHost({ exited }, world.env)
  })
})
