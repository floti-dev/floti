import { statSync } from 'node:fs'
import { homedir } from 'node:os'
import { join, posix, relative, win32 } from 'node:path'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { isolatedBaseFor, isolatedRoot, realDataDirFor } from '../../scripts/isolate.mjs'
import { userConfigDirFor } from './dataFolder'

/**
 * Where Floti's data lives on each OS: the installed app in the desktop's
 * folder of Floti's own data folder, and `pnpm dev` and every check in a
 * folder of their own outside it; templates and themes in the user's config
 * folder wherever the app is installed.
 */

/** Whether `child` is `parent` or anywhere under it, by the rules of `path`. */
function inside(path: typeof posix, parent: string, child: string): boolean {
  const rel = path.relative(parent, child)
  return rel === '' || (!rel.startsWith('..') && !path.isAbsolute(rel))
}

describe('the data directories', () => {
  it('on Windows: installed in %LOCALAPPDATA%\\Floti\\desktop, dev and checks in %LOCALAPPDATA%\\Floti Dev', () => {
    const env = { APPDATA: 'C:\\Users\\me\\AppData\\Roaming', LOCALAPPDATA: 'C:\\Users\\me\\AppData\\Local' }
    const real = realDataDirFor('win32', env, 'C:\\Users\\me')
    const isolated = isolatedBaseFor('win32', env, 'C:\\Users\\me')
    expect(real).toBe('C:\\Users\\me\\AppData\\Local\\Floti\\desktop')
    expect(isolated).toBe('C:\\Users\\me\\AppData\\Local\\Floti Dev')
    expect(inside(win32, real, isolated)).toBe(false)
    expect(inside(win32, 'C:\\Users\\me\\AppData\\Local\\Floti', isolated)).toBe(false)
  })

  it('on macOS: installed in Application Support/Floti/desktop, dev and checks in a folder of their own beside it', () => {
    const real = realDataDirFor('darwin', {}, '/Users/me')
    const isolated = isolatedBaseFor('darwin', {}, '/Users/me')
    expect(real).toBe('/Users/me/Library/Application Support/Floti/desktop')
    expect(isolated).toBe('/Users/me/Library/Application Support/Floti Dev')
    expect(inside(posix, real, isolated)).toBe(false)
    expect(inside(posix, isolated, real)).toBe(false)
  })

  it('on Linux: installed in the data home\'s floti/desktop, dev and checks in floti-dev beside it, each following XDG', () => {
    expect(realDataDirFor('linux', {}, '/home/me')).toBe('/home/me/.local/share/floti/desktop')
    expect(isolatedBaseFor('linux', {}, '/home/me')).toBe('/home/me/.local/share/floti-dev')
    const xdg = { XDG_CONFIG_HOME: '/cfg', XDG_DATA_HOME: '/data' }
    expect(realDataDirFor('linux', xdg, '/home/me')).toBe('/data/floti/desktop')
    expect(isolatedBaseFor('linux', xdg, '/home/me')).toBe('/data/floti-dev')
    expect(inside(posix, '/home/me/.local/share/floti', '/home/me/.local/share/floti-dev')).toBe(false)
  })

  it('puts dev beside the checks under the isolated base, on this OS', () => {
    const base = isolatedBaseFor(process.platform, process.env, homedir())
    expect(isolatedRoot('dev', null)).toBe(join(base, 'dev'))
    expect(isolatedRoot('selftest')).toBe(join(base, 'checks', 'selftest'))
    expect(relative(realDataDirFor(process.platform, process.env, homedir()), base).startsWith('..')).toBe(true)
  })
})

describe('templates and themes', () => {
  afterEach(() => {
    vi.unstubAllEnvs()
    vi.resetModules()
    vi.doUnmock('electron')
  })

  it("stay in the user's config folder for the installed app on every OS, and move with a dev or check run", async () => {
    vi.doMock('electron', async () => (await import('../../test/electron')).electronFake())
    vi.stubEnv('PORTABLE_EXECUTABLE_DIR', undefined)
    const installed = await import('./paths')
    const config = userConfigDirFor(homedir(), (path) => {
      try {
        return statSync(path).isDirectory()
      } catch {
        return false
      }
    })
    expect(installed.templatesDir).toBe(join(config, 'templates'))
    expect(installed.themesDir).toBe(join(config, 'themes'))

    vi.resetModules()
    const run = join(homedir(), 'isolated-run')
    vi.stubEnv('PORTABLE_EXECUTABLE_DIR', run)
    const isolated = await import('./paths')
    expect(isolated.appMode).toBe('dev')
    expect(isolated.templatesDir).toBe(join(run, 'helm-data', 'templates'))
  })
})
