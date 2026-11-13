import { compareVersions, parseVersionLine } from '@helm/core/types'
import { carriedFloti, standaloneFloti, type HostPlaces, type LauncherDeps } from './launcher'

/**
 * The installed app's floti is floti installed on its own, in the user's own
 * folder (`standaloneFloti`), where the install scripts put it, so a
 * computer has one floti however Floti came to it and `ssh <it> floti
 * version` finds the one whose host is running.
 *
 * The Windows installer puts it there (`build/installer.nsh`). The dmg and
 * the AppImage have no installer, so the app does it when it starts, on
 * every OS, before it reaches the local host: when the floti there is
 * missing, does not say its version, or is older than this Helm, the app
 * runs `floti install-self` from the floti it carries, which puts that one
 * and its Node there, puts the folder on the user's PATH where the OS keeps
 * it (Windows), and runs `floti setup` from there; a host already running
 * keeps running and is taken over by Restart host to update. A floti there
 * that is as new or newer, such as one `floti update` replaced, is left as
 * it is. Then the app uses it for everything it runs (`useStandalone`).
 * When it cannot be put there, the app says so in its log and runs the floti
 * it carries, as before.
 *
 * Only the installed app does this. The portable exe installs nothing and
 * runs the floti inside it, with its data beside it; a development run and
 * every check run the checkout's build.
 */

/** How a program run ended, with what it wrote. */
export interface Ran {
  code: number | null
  stdout: string
  stderr: string
}

export interface LocalFlotiDeps extends Pick<LauncherDeps, 'platform' | 'env' | 'exists' | 'log'> {
  /** This Helm's version, which the floti it carries shares. */
  appVersion: string
  /** Runs `command` with `args`, windowless, and answers how it ended; rejects when it cannot be run. */
  run: (command: string, args: string[]) => Promise<Ran>
}

/**
 * Makes sure the installed app's floti installed on its own is there and no
 * older than this Helm, and answers whether this run uses it.
 */
export async function ensureLocalFloti(places: HostPlaces, deps: LocalFlotiDeps): Promise<boolean> {
  if (places.appMode !== 'installed') return false
  const standalone = standaloneFloti(deps)
  if (standalone === null) {
    deps.log('floti: no folder of the user names where floti installed on its own goes; using the floti Floti carries')
    return false
  }
  if (await current(standalone, deps)) return true
  const carried = carriedFloti(places, deps)
  if (!deps.exists(carried)) {
    deps.log(`floti: Floti carries no floti at ${carried}, so none was put at ${standalone}`)
    return false
  }
  deps.log(`floti: putting this Floti's floti at ${standalone}`)
  try {
    const ran = await deps.run(carried, ['install-self'])
    for (const line of `${ran.stdout}\n${ran.stderr}`.split(/\r?\n/)) {
      if (line.trim() !== '') deps.log(`floti: ${line.trim()}`)
    }
  } catch (error) {
    deps.log(`floti: could not run ${carried} install-self: ${error instanceof Error ? error.message : String(error)}`)
  }
  if (await current(standalone, deps)) return true
  deps.log(`floti: ${standalone} is not this Floti's floti or newer; using the floti Floti carries`)
  return false
}

/** Whether the floti at `path` is there and says a version no older than this Helm's. */
async function current(path: string, deps: LocalFlotiDeps): Promise<boolean> {
  if (!deps.exists(path)) return false
  try {
    const found = parseVersionLine((await deps.run(path, ['version'])).stdout)
    const order = found === null ? null : compareVersions(found.version, deps.appVersion)
    return order !== null && order >= 0
  } catch {
    return false
  }
}

/**
 * The step settled once per run: everything that runs floti on this
 * computer (the local host's link, each remote host's link and the page
 * bridges that follow it, doctor's record) waits for `ready` before it
 * starts a program, so none runs the carried floti for its whole life when
 * floti installed on its own is about to be used. A bridge left running
 * from the carried floti, inside the install folder, would be found in use
 * by the next install, which would then stop the host.
 */
export interface LocalFloti {
  /** Settles the step, once; every later call waits on the same. */
  ready(): Promise<void>
  /** Whether this run uses floti installed on its own: false until `ready` has settled. */
  useStandalone(): boolean
  /** Runs `start` once `ready` has settled. */
  after<T>(start: () => Promise<T>): Promise<T>
}

export function createLocalFloti(ensure: () => Promise<boolean>): LocalFloti {
  let use = false
  let settled: Promise<void> | null = null
  const ready = (): Promise<void> => {
    settled ??= ensure().then(
      (standalone) => {
        use = standalone
      },
      () => {
        use = false
      }
    )
    return settled
  }
  return {
    ready,
    useStandalone: () => use,
    after: async (start) => {
      await ready()
      return start()
    }
  }
}
