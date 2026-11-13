import { spawnSync } from 'node:child_process'
import {
  chmodSync,
  copyFileSync,
  constants as fsConstants,
  existsSync,
  lstatSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  renameSync,
  rmdirSync,
  unlinkSync,
  writeFileSync
} from 'node:fs'
import { basename, dirname, join, posix, win32 } from 'node:path'
import { probeProcess, type OwnerLiveness } from '@helm/core'

/**
 * Where the desktop keeps its data, and the one move from where it kept it
 * while it was called Helm.
 *
 * Floti keeps what it holds for itself where the OS keeps an application's
 * data (design I12), and the desktop's share of that is a `desktop` folder
 * inside the host's own data folder, so its Chromium profile and `helm.db`
 * never mix with the host's files:
 *
 *   Windows  `%LOCALAPPDATA%\Floti\desktop`
 *   macOS    `~/Library/Application Support/Floti/desktop`
 *   Linux    `${XDG_DATA_HOME:-~/.local/share}/floti/desktop`
 *
 * Helm kept it in Electron's `userData` for the product name `Helm`:
 * `%APPDATA%\Helm`, `~/Library/Application Support/Helm` and
 * `${XDG_CONFIG_HOME:-~/.config}/Helm`. The installed app copies that folder
 * here once, at its first start as Floti ([`moveHelmData`]), and removes it
 * only once the copy has opened ([`finishHelmDataMove`]). `isolate.mjs`
 * (`realDataDirFor`) states the same folders for the scripts.
 */

/** The environment the folder rules read: only the variables named below. */
export type FolderEnv = Readonly<Record<string, string | undefined>>

/** The desktop's folder inside Floti's data folder. */
export const DESKTOP_FOLDER = 'desktop'

/** An absolute path a variable names, by the rules of `path`, or `undefined`. */
function absolute(path: typeof posix, value: string | undefined): string | undefined {
  return value !== undefined && value !== '' && path.isAbsolute(value) ? value : undefined
}

/**
 * Where the installed desktop keeps its data on `platform`, for the user
 * whose environment is `env` and whose home is `home`. The host's own data
 * folder (`platform/folders` in the host) by the same rules, then `desktop`.
 */
export function flotiDataDirFor(platform: NodeJS.Platform, env: FolderEnv, home: string): string {
  if (platform === 'win32') {
    const local = absolute(win32, env['LOCALAPPDATA']) ?? win32.join(home, 'AppData', 'Local')
    return win32.join(local, 'Floti', DESKTOP_FOLDER)
  }
  if (platform === 'darwin') return posix.join(home, 'Library', 'Application Support', 'Floti', DESKTOP_FOLDER)
  const share = absolute(posix, env['XDG_DATA_HOME']) ?? posix.join(home, '.local', 'share')
  return posix.join(share, 'floti', DESKTOP_FOLDER)
}

/**
 * Where Helm kept its data on `platform`: Electron's `userData` for an app
 * whose product name was `Helm`.
 */
export function helmDataDirFor(platform: NodeJS.Platform, env: FolderEnv, home: string): string {
  if (platform === 'win32') return win32.join(env['APPDATA'] ?? win32.join(home, 'AppData', 'Roaming'), 'Helm')
  if (platform === 'darwin') return posix.join(home, 'Library', 'Application Support', 'Helm')
  return posix.join(env['XDG_CONFIG_HOME'] || posix.join(home, '.config'), 'Helm')
}

/**
 * The user's own files (templates, themes, plugins they are writing) on every
 * OS: `~/.config/helm` while Helm's folder is there, else `~/.config/floti`.
 * The host's own rule (`config_folder` in the host): the first host to start
 * as Floti moves Helm's folder to Floti's once, and that move is what
 * switches a computer over.
 */
export function userConfigDirFor(home: string, isDir: (path: string) => boolean): string {
  const helm = join(home, '.config', 'helm')
  return isDir(helm) ? helm : join(home, '.config', 'floti')
}

/**
 * The file in the moved copy that names the folder it was copied from, until
 * that folder is removed. Its being there is what says the old folder is
 * still to go.
 */
export const MOVED_FROM_FILE = 'moved-from-helm.json'

/** The file whose presence says a folder holds Helm's data. */
const STORE_FILE = 'helm.db'

/** What a copy in progress is called beside the folder it becomes. */
const COPYING = '.copying-'

/**
 * Chromium's caches, which the copy leaves out at any depth: each is made
 * again as it is needed, and a large one would hold up the first start as
 * Floti before any window opens. Compared ignoring case.
 */
export const CACHE_FOLDERS: ReadonlySet<string> = new Set(
  [
    'Cache',
    'Code Cache',
    'GPUCache',
    'DawnCache',
    'DawnGraphiteCache',
    'DawnWebGPUCache',
    'GrShaderCache',
    'GraphiteDawnCache',
    'ShaderCache'
  ].map((name) => name.toLowerCase())
)

/**
 * The folders whose files are keys: this PC's host's client folder
 * (`host-client`) and each remote host's (`remote-hosts/<id>`), each holding
 * a device key and the host key it pinned. The host writes them owner-only
 * (`platform/private_files`); a copy carries no access list on Windows, so
 * each copied file there is made owner-only again before the copy is in
 * place.
 */
export const KEY_FOLDERS: readonly string[] = ['host-client', 'remote-hosts']

/** What starting found, and did, about Helm's data. */
export type DataMove =
  /** The data is in Floti's folder already, or there was none to move. */
  | { kind: 'in-place' }
  /** Helm's folder was copied to Floti's, and is removed once the copy opens. */
  | { kind: 'moved'; from: string; to: string }
  /** The copy failed. Helm's folder is as it was, and nothing is in Floti's. */
  | { kind: 'failed'; from: string; to: string; reason: string }

/** What a move may ask about the machine, passed in so a test can answer. */
export interface MoveWorld {
  /** Whether the process with this id is still there. */
  probe: (pid: number) => OwnerLiveness
  /** This process's id, which names its copy in progress. */
  pid: number
  /** Makes the file at a path readable and writable by the current user alone. */
  ownerOnly: (path: string) => void
}

const REAL_WORLD: MoveWorld = { probe: probeProcess, pid: process.pid, ownerOnly: makeOwnerOnly }

/**
 * Makes the file at `path` the current user's alone, as the host's
 * `platform/private_files` makes a key: `0600` on macOS and Linux; on
 * Windows a protected access list with one entry, full access for the
 * current user, set by `icacls` (inheritance removed, the user's grant
 * replacing any other). Throws when it could not.
 */
export function makeOwnerOnly(path: string): void {
  if (process.platform !== 'win32') {
    chmodSync(path, 0o600)
    return
  }
  const icacls = spawnSync(
    systemProgram('icacls.exe'),
    [path, '/inheritance:r', '/grant:r', `*${currentUserSid()}:F`],
    { windowsHide: true, encoding: 'utf8' }
  )
  if (icacls.status !== 0) {
    throw new Error(`icacls could not make ${path} owner-only: ${(icacls.stderr || icacls.stdout || String(icacls.error)).trim()}`)
  }
}

/** A program in Windows' System32, never one found on `PATH`. */
function systemProgram(name: string): string {
  return win32.join(process.env['SystemRoot'] ?? 'C:\\Windows', 'System32', name)
}

let userSid: string | undefined

/** The SID of the user this process runs as, from `whoami /user`. */
function currentUserSid(): string {
  if (userSid !== undefined) return userSid
  const whoami = spawnSync(systemProgram('whoami.exe'), ['/user', '/fo', 'csv', '/nh'], {
    windowsHide: true,
    encoding: 'utf8'
  })
  const sid = /"(S-1-[0-9-]+)"\s*$/m.exec(whoami.stdout ?? '')?.[1]
  if (whoami.status !== 0 || sid === undefined) {
    throw new Error(`whoami did not say who this user is: ${(whoami.stderr || String(whoami.error)).trim()}`)
  }
  userSid = sid
  return sid
}

/**
 * Copies Helm's data folder `from` to Floti's `to`, once, before anything
 * opens either.
 *
 * Nothing happens when `to` is there already, or `from` holds no `helm.db`.
 * Otherwise `from` is copied, file by file, to a folder beside `to` that this
 * process alone names, which is then renamed to `to`, so a copy cut off
 * halfway is never taken for the data. Links are not copied and never walked
 * into: Helm's folder holds junctions (symlinks on macOS and Linux) into the
 * user's repositories under `overlays/`, which the start-up sweep makes again
 * as sessions need them. Chromium's caches ([`CACHE_FOLDERS`]) are left out,
 * and the keys in [`KEY_FOLDERS`] are made owner-only again. The copy records
 * where it came from ([`MOVED_FROM_FILE`]); `from` itself is left exactly as
 * it was.
 *
 * A copy left beside `to` by a start that ended before renaming it is removed
 * first, when the process that made it is gone.
 */
export function moveHelmData(from: string, to: string, world: MoveWorld = REAL_WORLD): DataMove {
  if (exists(to)) return { kind: 'in-place' }
  sweepCopies(to, world)
  if (!existsSync(join(from, STORE_FILE))) return { kind: 'in-place' }
  const copy = join(dirname(to), `.${basename(to)}${COPYING}${String(world.pid)}`)
  try {
    mkdirSync(dirname(to), { recursive: true })
    copyTree(from, copy, '', world)
    writeFileSync(join(copy, MOVED_FROM_FILE), `${JSON.stringify({ from }, null, 2)}\n`)
    renameSync(copy, to)
    return { kind: 'moved', from, to }
  } catch (err) {
    try {
      removeTree(copy)
    } catch {
      // A copy left behind is swept at the next start; the error that
      // matters is the copy's own.
    }
    return { kind: 'failed', from, to, reason: (err as Error).message }
  }
}

/**
 * Removes the folder the data in `to` was copied from, once that copy has
 * opened, and then the record of it ([`MOVED_FROM_FILE`]). Nothing when there
 * is no record. A folder that cannot be removed whole stays recorded and is
 * tried again at the next start; links in it are unlinked, never walked into.
 * Answers the folder it removed, or `null`.
 */
export function finishHelmDataMove(to: string): string | null {
  const record = join(to, MOVED_FROM_FILE)
  let from: unknown
  try {
    from = (JSON.parse(readFileSync(record, 'utf8')) as { from?: unknown }).from
  } catch {
    return null
  }
  if (typeof from !== 'string' || from === '' || samePath(from, to)) {
    unlinkSync(record)
    return null
  }
  removeTree(from)
  unlinkSync(record)
  return from
}

/** Whether `a` and `b` name one folder, ignoring case where the OS does. */
function samePath(a: string, b: string): boolean {
  return process.platform === 'win32' || process.platform === 'darwin'
    ? a.toLowerCase() === b.toLowerCase()
    : a === b
}

/** Whether anything, a link included, is at `path`. */
function exists(path: string): boolean {
  try {
    lstatSync(path)
    return true
  } catch {
    return false
  }
}

/** Removes copies beside `to` whose process is gone. */
function sweepCopies(to: string, world: MoveWorld): void {
  const prefix = `.${basename(to)}${COPYING}`
  let names: string[]
  try {
    names = readdirSync(dirname(to))
  } catch {
    return
  }
  for (const name of names) {
    if (!name.startsWith(prefix)) continue
    const pid = Number(name.slice(prefix.length))
    if (!Number.isInteger(pid) || pid <= 0 || pid === world.pid) continue
    if (world.probe(pid) !== 'gone') continue
    try {
      removeTree(join(dirname(to), name))
    } catch {
      // Left for the next start.
    }
  }
}

/**
 * Copies the folder `from` to `to`, which must not exist: folders and files,
 * no links, no caches ([`CACHE_FOLDERS`]) and nothing else (sockets and the
 * like). `top` is the first folder under the data folder this one is in, or
 * `''` for the data folder itself; a file under one of [`KEY_FOLDERS`] is
 * made owner-only once copied.
 */
function copyTree(from: string, to: string, top: string, world: MoveWorld): void {
  mkdirSync(to)
  for (const name of readdirSync(from)) {
    const source = join(from, name)
    const target = join(to, name)
    const stat = lstatSync(source)
    if (stat.isSymbolicLink()) continue
    if (stat.isDirectory()) {
      if (CACHE_FOLDERS.has(name.toLowerCase())) continue
      copyTree(source, target, top === '' ? name : top, world)
    } else if (stat.isFile()) {
      copyFileSync(source, target, fsConstants.COPYFILE_EXCL)
      if (KEY_FOLDERS.includes(top)) world.ownerOnly(target)
    }
  }
}

/**
 * Removes `path` and everything under it without walking into a link: a link
 * is unlinked where it is found (`unlink`, else `rmdir`, which is what
 * Windows removes a directory junction with), files are unlinked, and each
 * folder is removed once it is empty. Nothing here is a recursive delete
 * that could follow a junction into a repository.
 */
export function removeTree(path: string): void {
  let stat
  try {
    stat = lstatSync(path)
  } catch {
    return
  }
  if (stat.isSymbolicLink()) {
    try {
      unlinkSync(path)
    } catch {
      rmdirSync(path)
    }
    return
  }
  if (!stat.isDirectory()) {
    unlinkSync(path)
    return
  }
  for (const name of readdirSync(path)) removeTree(join(path, name))
  rmdirSync(path)
}
