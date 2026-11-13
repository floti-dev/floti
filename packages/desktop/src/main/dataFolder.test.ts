import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { beforeEach, describe, expect, it } from 'vitest'
import { helmDataDirFor as helmDataDirForScripts, realDataDirFor } from '../../scripts/isolate.mjs'
import {
  MOVED_FROM_FILE,
  finishHelmDataMove,
  flotiDataDirFor,
  helmDataDirFor,
  moveHelmData,
  removeTree,
  userConfigDirFor,
  type MoveWorld
} from './dataFolder'

/**
 * The desktop's data folder after the rename (design I12), and the one copy
 * from where Helm kept it, against folders under the test run's root only.
 */

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

  it("names Helm's folder where Electron put it for the product name Helm", () => {
    expect(helmDataDirFor('win32', WIN_ENV, 'C:\\Users\\Some One')).toBe('C:\\Users\\Some One\\AppData\\Roaming\\Helm')
    expect(helmDataDirFor('darwin', {}, '/Users/me')).toBe('/Users/me/Library/Application Support/Helm')
    expect(helmDataDirFor('linux', {}, '/home/me')).toBe('/home/me/.config/Helm')
    expect(helmDataDirFor('linux', { XDG_CONFIG_HOME: '/cfg' }, '/home/me')).toBe('/cfg/Helm')
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
      expect(helmDataDirForScripts(platform, env, home)).toBe(helmDataDirFor(platform, env, home))
    }
  })
})

describe("the user's config folder", () => {
  const home = '/home/some one'
  const floti = join(home, '.config', 'floti')
  const helm = join(home, '.config', 'helm')

  it("is Helm's while it is there, as the host reads it, else ~/.config/floti", () => {
    expect(userConfigDirFor(home, () => false)).toBe(floti)
    expect(userConfigDirFor(home, (path) => path === floti)).toBe(floti)
    expect(userConfigDirFor(home, (path) => path === helm)).toBe(helm)
    expect(userConfigDirFor(home, () => true)).toBe(helm)
  })
})

describe('moving Helm\'s data to Floti\'s folder', () => {
  let root: string
  let from: string
  let to: string
  let repo: string
  const world: MoveWorld = { probe: () => 'gone', pid: 4242 }

  beforeEach(() => {
    root = mkdtempSync(join(tmpdir(), 'data move '))
    from = join(root, 'Roaming', 'Helm')
    to = join(root, 'Local', 'Floti', 'desktop')
    repo = join(root, 'a repo', '.claude', 'skills')
    mkdirSync(repo, { recursive: true })
    writeFileSync(join(repo, 'SKILL.md'), 'the repository\'s own')
    mkdirSync(join(from, 'Partitions', 'helm-browser', 'Network'), { recursive: true })
    writeFileSync(join(from, 'helm.db'), 'the store')
    writeFileSync(join(from, 'helm.db-wal'), 'its log')
    writeFileSync(join(from, 'Partitions', 'helm-browser', 'Network', 'Cookies'), 'sign-ins')
    mkdirSync(join(from, 'overlays', 'shim one'), { recursive: true })
    symlinkSync(repo, join(from, 'overlays', 'shim one', 'skills'), 'junction')
  })

  it("copies the folder, links left out, and leaves Helm's as it was", () => {
    expect(moveHelmData(from, to, world)).toEqual({ kind: 'moved', from, to })

    expect(readFileSync(join(to, 'helm.db'), 'utf8')).toBe('the store')
    expect(readFileSync(join(to, 'helm.db-wal'), 'utf8')).toBe('its log')
    expect(readFileSync(join(to, 'Partitions', 'helm-browser', 'Network', 'Cookies'), 'utf8')).toBe('sign-ins')
    expect(readdirSync(join(to, 'overlays', 'shim one'))).toEqual([])
    expect(JSON.parse(readFileSync(join(to, MOVED_FROM_FILE), 'utf8'))).toEqual({ from })

    expect(readFileSync(join(from, 'helm.db'), 'utf8')).toBe('the store')
    expect(readFileSync(join(from, 'overlays', 'shim one', 'skills', 'SKILL.md'), 'utf8')).toBe('the repository\'s own')
    expect(readdirSync(join(root, 'Local', 'Floti'))).toEqual(['desktop'])
  })

  it('moves nothing once Floti\'s folder is there', () => {
    mkdirSync(to, { recursive: true })
    writeFileSync(join(to, 'helm.db'), 'already here')

    expect(moveHelmData(from, to, world)).toEqual({ kind: 'in-place' })
    expect(readFileSync(join(to, 'helm.db'), 'utf8')).toBe('already here')
    expect(existsSync(join(to, MOVED_FROM_FILE))).toBe(false)
  })

  it('moves nothing when Helm\'s folder holds no store', () => {
    const empty = join(root, 'no helm')
    mkdirSync(empty)
    writeFileSync(join(empty, 'Preferences'), '{}')

    expect(moveHelmData(empty, to, world)).toEqual({ kind: 'in-place' })
    expect(moveHelmData(join(root, 'nowhere'), to, world)).toEqual({ kind: 'in-place' })
    expect(existsSync(to)).toBe(false)
  })

  it('leaves nothing behind when the copy fails, and Helm\'s folder as it was', () => {
    // A file where the copy's folder has to go: making it fails.
    mkdirSync(join(root, 'Local'), { recursive: true })
    writeFileSync(join(root, 'Local', 'Floti'), 'in the way')

    const moved = moveHelmData(from, to, world)

    expect(moved.kind).toBe('failed')
    expect(existsSync(to)).toBe(false)
    expect(readFileSync(join(from, 'helm.db'), 'utf8')).toBe('the store')
  })

  it('sweeps a copy a start that ended left behind, and keeps one whose process is alive', () => {
    const parent = join(root, 'Local', 'Floti')
    mkdirSync(join(parent, '.desktop.copying-77', 'Partitions'), { recursive: true })
    mkdirSync(join(parent, '.desktop.copying-78'), { recursive: true })

    moveHelmData(from, to, { probe: (pid) => (pid === 78 ? 'alive' : 'gone'), pid: 4242 })

    expect(readdirSync(parent).sort()).toEqual(['.desktop.copying-78', 'desktop'])
  })

  it("removes Helm's folder once the copy has opened, unlinking its links, and only then", () => {
    moveHelmData(from, to, world)
    expect(existsSync(from)).toBe(true)

    expect(finishHelmDataMove(to)).toBe(from)

    expect(existsSync(from)).toBe(false)
    expect(existsSync(join(to, MOVED_FROM_FILE))).toBe(false)
    expect(readFileSync(join(repo, 'SKILL.md'), 'utf8')).toBe('the repository\'s own')
    expect(readFileSync(join(to, 'helm.db'), 'utf8')).toBe('the store')
    expect(finishHelmDataMove(to)).toBeNull()
  })

  it('removes nothing when nothing was moved', () => {
    mkdirSync(to, { recursive: true })
    expect(finishHelmDataMove(to)).toBeNull()
    expect(existsSync(from)).toBe(true)
  })
})

describe('removing a tree', () => {
  it('unlinks a link where it finds one and never walks into it', () => {
    const root = mkdtempSync(join(tmpdir(), 'remove tree '))
    const target = join(root, 'kept folder')
    mkdirSync(target)
    writeFileSync(join(target, 'file'), 'kept')
    const tree = join(root, 'tree')
    mkdirSync(join(tree, 'inner'), { recursive: true })
    symlinkSync(target, join(tree, 'inner', 'link'), 'junction')
    writeFileSync(join(tree, 'inner', 'file'), 'gone')

    removeTree(tree)

    expect(existsSync(tree)).toBe(false)
    expect(readFileSync(join(target, 'file'), 'utf8')).toBe('kept')
  })
})
