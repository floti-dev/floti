import { describe, expect, it } from 'vitest'
import type { HostPlaces } from './launcher'
import { ensureLocalFloti, type LocalFlotiDeps, type Ran } from './local-floti'

const CARRIED = '/Applications/Helm.app/Contents/Resources/floti'
const OWN = '/Users/me/.local/bin/floti'

const PLACES: HostPlaces = {
  appMode: 'installed',
  dataDir: '/Users/me/Library/Application Support/Helm',
  packaged: true,
  resourcesPath: '/Applications/Helm.app/Contents/Resources',
  repoRoot: '/src/helm',
  templatesDir: '/Users/me/.config/helm/templates',
  execPath: '/Applications/Helm.app/Contents/MacOS/Helm',
  appPath: '/Applications/Helm.app/Contents/Resources/app.asar'
}

/** A computer with floti at the paths `files` names, each saying its version; install-self puts the carried one in place. */
function computer(files: Record<string, string>, installs: (files: Record<string, string>) => Ran | Error = put) {
  const runs: string[] = []
  const log: string[] = []
  const deps: LocalFlotiDeps = {
    platform: 'darwin',
    env: { HOME: '/Users/me' },
    exists: (path) => path in files,
    log: (line) => log.push(line),
    appVersion: '3.1.0',
    run: async (command, args) => {
      runs.push([command, ...args].join(' '))
      if (args[0] === 'version') {
        const version = files[command]
        return version === undefined
          ? Promise.reject(new Error('ENOENT'))
          : { code: 0, stdout: `floti ${version} protocol 3.0 macos arm64\n`, stderr: '' }
      }
      const ran = installs(files)
      if (ran instanceof Error) throw ran
      return ran
    }
  }
  return { deps, runs, log }
}

function put(files: Record<string, string>): Ran {
  files[OWN] = files[CARRIED] ?? ''
  return { code: 0, stdout: `floti is at ${OWN}.\nFloti host is running.\n`, stderr: '' }
}

describe('the installed app’s floti installed on its own', () => {
  it('is put there from the floti the app carries when it is missing, then used', async () => {
    const { deps, runs, log } = computer({ [CARRIED]: '3.1.0' })
    expect(await ensureLocalFloti(PLACES, deps)).toBe(true)
    expect(runs).toEqual([`${CARRIED} install-self`, `${OWN} version`])
    expect(log).toContain(`floti: floti is at ${OWN}.`)
  })

  it('is put there when the one there is older, or says no version', async () => {
    for (const there of ['3.0.2', 'not a version']) {
      const { deps, runs } = computer({ [CARRIED]: '3.1.0', [OWN]: there })
      expect(await ensureLocalFloti(PLACES, deps)).toBe(true)
      expect(runs).toEqual([`${OWN} version`, `${CARRIED} install-self`, `${OWN} version`])
    }
  })

  it('is used as it is when it is this Helm’s version or newer, running nothing more', async () => {
    for (const there of ['3.1.0', '3.2.0-preview.1']) {
      const { deps, runs } = computer({ [CARRIED]: '3.1.0', [OWN]: there })
      expect(await ensureLocalFloti(PLACES, deps)).toBe(true)
      expect(runs).toEqual([`${OWN} version`])
    }
  })

  it('is not used when it could not be put there, and the log says why', async () => {
    const refused = computer({ [CARRIED]: '3.1.0', [OWN]: '3.0.0' }, () => ({
      code: 1,
      stdout: '',
      stderr: 'floti install-self: could not make /Users/me/.local/bin: permission denied\n'
    }))
    expect(await ensureLocalFloti(PLACES, refused.deps)).toBe(false)
    expect(refused.log).toContain('floti: floti install-self: could not make /Users/me/.local/bin: permission denied')

    const unrunnable = computer({ [CARRIED]: '3.1.0' }, () => new Error('EACCES'))
    expect(await ensureLocalFloti(PLACES, unrunnable.deps)).toBe(false)
    expect(unrunnable.log.some((line) => line.includes('EACCES'))).toBe(true)
  })

  it('is never touched by the portable app, a development run or a check', async () => {
    for (const appMode of ['portable', 'dev', 'dev-live'] as const) {
      const { deps, runs } = computer({ [CARRIED]: '3.1.0' })
      expect(await ensureLocalFloti({ ...PLACES, appMode }, deps)).toBe(false)
      expect(runs).toEqual([])
    }
  })

  it('is not looked for where no folder of the user’s is named', async () => {
    const { deps, runs } = computer({ [CARRIED]: '3.1.0' })
    expect(await ensureLocalFloti(PLACES, { ...deps, env: {} })).toBe(false)
    expect(runs).toEqual([])
  })
})
