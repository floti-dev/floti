import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { realDataDirFor } from '../../scripts/isolate.mjs'
import { flotiDataDirFor, flotiLogsDirFor, userConfigDirFor } from './dataFolder'

/** The desktop's data folder on each OS (design I12), and the user's config folder. */

const WIN_ENV = { APPDATA: 'C:\\Users\\Some One\\AppData\\Roaming', LOCALAPPDATA: 'C:\\Users\\Some One\\AppData\\Local' }

describe("Floti's data folder on each OS", () => {
  it("is the desktop's folder in Floti's own data folder, not Electron's roaming userData", () => {
    expect(flotiDataDirFor('win32', WIN_ENV, 'C:\\Users\\Some One')).toBe('C:\\Users\\Some One\\AppData\\Local\\Floti\\desktop')
    expect(flotiDataDirFor('win32', {}, 'C:\\Users\\Some One')).toBe('C:\\Users\\Some One\\AppData\\Local\\Floti\\desktop')
    expect(flotiDataDirFor('darwin', {}, '/Users/some one')).toBe('/Users/some one/Library/Application Support/Floti/desktop')
    expect(flotiDataDirFor('linux', {}, '/home/some one')).toBe('/home/some one/.local/share/floti/desktop')
    expect(flotiDataDirFor('linux', { XDG_DATA_HOME: '/data home' }, '/home/me')).toBe('/data home/floti/desktop')
  })

  it('ignores a relative XDG_DATA_HOME or LOCALAPPDATA, as the host does', () => {
    expect(flotiDataDirFor('linux', { XDG_DATA_HOME: 'relative' }, '/home/me')).toBe('/home/me/.local/share/floti/desktop')
    expect(flotiDataDirFor('win32', { LOCALAPPDATA: 'relative' }, 'C:\\Users\\me')).toBe('C:\\Users\\me\\AppData\\Local\\Floti\\desktop')
  })

  it("puts the installed host's logs where the host's own rule does, FLOTI_LOG_DIR first", () => {
    expect(flotiLogsDirFor('win32', WIN_ENV, 'C:\\Users\\Some One')).toBe('C:\\Users\\Some One\\AppData\\Local\\Floti\\logs')
    expect(flotiLogsDirFor('darwin', {}, '/Users/some one')).toBe('/Users/some one/Library/Application Support/Floti/logs')
    expect(flotiLogsDirFor('linux', {}, '/home/some one')).toBe('/home/some one/.local/state/floti')
    expect(flotiLogsDirFor('linux', { XDG_STATE_HOME: '/state home' }, '/home/me')).toBe('/state home/floti')
    expect(flotiLogsDirFor('linux', { XDG_STATE_HOME: 'relative' }, '/home/me')).toBe('/home/me/.local/state/floti')
    expect(flotiLogsDirFor('linux', { FLOTI_LOG_DIR: '/moved logs' }, '/home/me')).toBe('/moved logs')
    expect(flotiLogsDirFor('win32', { FLOTI_LOG_DIR: 'relative' }, 'C:\\Users\\me')).toBe('C:\\Users\\me\\AppData\\Local\\Floti\\logs')
  })

  it('is the same folder the scripts name', () => {
    const cases: [NodeJS.Platform, Record<string, string>, string][] = [
      ['win32', WIN_ENV, 'C:\\Users\\Some One'],
      ['win32', {}, 'C:\\Users\\me'],
      ['darwin', {}, '/Users/me'],
      ['linux', {}, '/home/me'],
      ['linux', { XDG_DATA_HOME: '/data', XDG_CONFIG_HOME: '/cfg' }, '/home/me'],
      ['linux', { XDG_DATA_HOME: 'relative' }, '/home/me']
    ]
    for (const [platform, env, home] of cases) {
      expect(realDataDirFor(platform, env, home)).toBe(flotiDataDirFor(platform, env, home))
    }
  })
})

describe("the user's config folder", () => {
  it('is ~/.config/floti, as the host reads it', () => {
    expect(userConfigDirFor('/home/some one')).toBe(join('/home/some one', '.config', 'floti'))
  })
})
