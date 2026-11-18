import { join, posix, win32 } from 'node:path'

/**
 * Where the desktop keeps its data.
 *
 * Floti keeps what it holds for itself where the OS keeps an application's
 * data (design I12), and the desktop's share of that is a `desktop` folder
 * inside the host's own data folder, so its Chromium profile and its database
 * never mix with the host's files:
 *
 *   Windows  `%LOCALAPPDATA%\Floti\desktop`
 *   macOS    `~/Library/Application Support/Floti/desktop`
 *   Linux    `${XDG_DATA_HOME:-~/.local/share}/floti/desktop`
 *
 * `isolate.mjs` (`realDataDirFor`) states the same folders for the scripts.
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
 * Where the installed app's host keeps its logs on `platform`, for the user
 * whose environment is `env` and whose home is `home`: the host's own rule
 * (`platform/folders` in the host), `FLOTI_LOG_DIR` first when it names an
 * absolute folder, as the host started with this environment reads it.
 *
 *   Windows  `%LOCALAPPDATA%\Floti\logs`
 *   macOS    `~/Library/Application Support/Floti/logs`
 *   Linux    `${XDG_STATE_HOME:-~/.local/state}/floti`
 */
export function flotiLogsDirFor(platform: NodeJS.Platform, env: FolderEnv, home: string): string {
  const path = platform === 'win32' ? win32 : posix
  const moved = absolute(path, env['FLOTI_LOG_DIR'])
  if (moved !== undefined) return moved
  if (platform === 'win32') {
    const local = absolute(win32, env['LOCALAPPDATA']) ?? win32.join(home, 'AppData', 'Local')
    return win32.join(local, 'Floti', 'logs')
  }
  if (platform === 'darwin') return posix.join(home, 'Library', 'Application Support', 'Floti', 'logs')
  const state = absolute(posix, env['XDG_STATE_HOME']) ?? posix.join(home, '.local', 'state')
  return posix.join(state, 'floti')
}

/**
 * The user's own files (templates, themes, plugins they are writing) on every
 * OS: `~/.config/floti`, the host's own rule (`config_folder` in the host).
 */
export function userConfigDirFor(home: string): string {
  return join(home, '.config', 'floti')
}
