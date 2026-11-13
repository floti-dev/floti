import { app, dialog } from 'electron'
import { mkdirSync, statSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'
import type { AppMode } from '../shared/ipc'
import { finishHelmDataMove, flotiDataDirFor, helmDataDirFor, moveHelmData, userConfigDirFor } from './dataFolder'

/**
 * Where Floti keeps its data.
 *
 * Portable builds put it beside the exe so the whole install travels on a
 * stick; installed builds keep it in Floti's own data folder (design I12,
 * `dataFolder.ts`):
 *
 *   Windows  `%LOCALAPPDATA%\Floti\desktop`
 *   macOS    `~/Library/Application Support/Floti/desktop`
 *   Linux    `${XDG_DATA_HOME:-~/.local/share}/floti/desktop`
 *
 * That is not Electron's own `userData` for the product name `Floti`, so
 * `initDataDir` points `userData` there before anything reads it. Helm kept
 * it in `userData` for the product name `Helm` (`%APPDATA%\Helm` and its
 * macOS and Linux equivalents); the installed app copies that folder here
 * once, at its first start as Floti, and removes it once the copy has opened.
 *
 * electron-builder's portable launcher is what tells us which we are: it sets
 * `PORTABLE_EXECUTABLE_DIR` to the directory the exe was double-clicked from
 * (the exe itself runs from a temp extraction directory, so `process.execPath`
 * would point at the wrong place).
 */

const portableDir = process.env['PORTABLE_EXECUTABLE_DIR']

/**
 * Which of the four this run is.
 *
 * Two questions, not one. `app.isPackaged` says whether this is a build or a
 * checkout, and `PORTABLE_EXECUTABLE_DIR` says whether the data lives somewhere
 * of its own - and the pair matters because the case with no directory of its
 * own is the dangerous one. An unpackaged run with no portable directory uses
 * **the installed app's own** data folder: the same `helm.db`, the same
 * `overlays/` and the same Chromium profile as the Floti somebody is using.
 * That is `dev-live`, it is what `pnpm dev:live` asks for on purpose, and it is
 * named rather than inferred so the status bar can say it.
 *
 * `pnpm dev` and every check go through `scripts/isolate.mjs` instead, which
 * sets `PORTABLE_EXECUTABLE_DIR` to a directory of their own outside it:
 * `%LOCALAPPDATA%\Floti Dev` on Windows, `~/Library/Application Support/Floti Dev`
 * on macOS, `${XDG_DATA_HOME:-~/.local/share}/floti-dev` on Linux.
 */
export const appMode: AppMode = app.isPackaged
  ? portableDir
    ? 'portable'
    : 'installed'
  : portableDir
    ? 'dev'
    : 'dev-live'

/** Whether `path` is a folder. */
function isDir(path: string): boolean {
  try {
    return statSync(path).isDirectory()
  } catch {
    return false
  }
}

/** Floti's data folder for the installed app on this computer. */
const flotiData = (): string => flotiDataDirFor(process.platform, process.env, homedir())

/** The folder Helm kept its data in on this computer. */
const helmData = (): string => helmDataDirFor(process.platform, process.env, homedir())

/**
 * The installed app's data folder: Floti's, which `initDataDir` fills from
 * Helm's at the first start. `dev:live` moves nothing, so until the installed
 * app has made the move it reads Helm's folder where it is.
 */
function liveDataDir(): string {
  const floti = flotiData()
  if (appMode === 'dev-live' && !isDir(floti) && isDir(helmData())) return helmData()
  return floti
}

export const dataDir: string = portableDir ? join(portableDir, 'helm-data') : liveDataDir()

/**
 * Must run before anything reads `app.getPath('userData')`.
 *
 * The installed app first copies Helm's data folder to Floti's, once
 * (`moveHelmData`). A copy that fails changes nothing: the app says why and
 * quits, and the next start tries again with Helm's folder as it was.
 */
export function initDataDir(): void {
  if (appMode === 'installed') {
    const moved = moveHelmData(helmData(), dataDir)
    if (moved.kind === 'moved') console.log(`data: copied ${moved.from} to ${moved.to}`)
    if (moved.kind === 'failed') {
      dialog.showErrorBox(
        'Floti could not move its data',
        `Floti keeps its data in ${moved.to} now, and copying it there from ${moved.from} failed: ` +
          `${moved.reason}\n\nNothing was changed, and ${moved.from} is as it was. ` +
          'Floti tries again when it next starts.'
      )
      app.exit(1)
      return
    }
  }
  mkdirSync(dataDir, { recursive: true })
  app.setPath('userData', dataDir)
}

/**
 * Called once the store in the data folder has opened: removes the folder it
 * was copied from at this start or an earlier one (`finishHelmDataMove`). A
 * folder that cannot be removed yet is tried again at the next start.
 */
export function finishDataMove(): void {
  if (appMode !== 'installed') return
  try {
    const removed = finishHelmDataMove(dataDir)
    if (removed !== null) console.log(`data: removed ${removed}, which Floti's data was copied from`)
  } catch (err) {
    console.warn(`data: the folder Floti's data was copied from is not removed yet: ${(err as Error).message}`)
  }
}

export const dbFile: string = join(dataDir, 'helm.db')

/**
 * Where the user's workspace templates live.
 *
 * The same `PORTABLE_EXECUTABLE_DIR` branch `dataDir` takes, and that is the
 * whole of the mechanism - one rule covering all four run modes rather than a
 * second override invented for this one:
 *
 *   installed, dev:live   `templates` in the user's config folder, on every OS
 *   portable              `helm-data/templates` beside the exe
 *   pnpm dev              its own, under `<isolated base>/dev`
 *   every check           its own, under `<isolated base>/checks/<name>`
 *
 * where the user's config folder is the host's own (`userConfigDirFor`):
 * `~/.config/floti`, or `~/.config/helm` until the host has moved it, and the isolated base is the per-OS folder
 * `isolate.mjs` names (`%LOCALAPPDATA%\Floti Dev` on Windows).
 *
 * The last two come for free: both go through `scripts/isolate.mjs`, which sets
 * the variable. `pnpm dev` needs its own rather than sharing because in-app
 * authoring makes the dev app a *writer* of templates - harmless while it can
 * only read, and somebody's templates directory the moment it cannot.
 * `scripts/run-dev.mjs` seeds it by copying the real one, the way the database
 * is seeded from a `VACUUM INTO` copy.
 *
 * Not under the installed data directory, and that is deliberate: these are
 * files a person writes and edits by hand, likely keeps in git, and has to be
 * able to find. `~/.config/floti` is where somebody looks for that;
 * `%LOCALAPPDATA%` and `~/Library/Application Support` are where an
 * application keeps things about itself.
 */
export const templatesDir: string = portableDir
  ? join(dataDir, 'templates')
  : join(userConfigDirFor(homedir(), isDir), 'templates')

/**
 * Where user themes live: one `*.json` per theme, watched while the app runs.
 *
 * The templates directory's sibling, by the templates directory's rule and for
 * its reasons - a theme is a file a person writes by hand and may keep in git,
 * so an installed Floti keeps them in `~/.config/floti/themes`, and anything
 * carrying `PORTABLE_EXECUTABLE_DIR` (a portable exe, `pnpm dev`, every check)
 * gets its own under its data directory. Unlike templates, `pnpm dev` does not
 * copy the real ones across: a theme is cheap to duplicate into the dev
 * directory, and the dev app writing into somebody's theme folder is the thing
 * the separate directory is for.
 */
export const themesDir: string = portableDir
  ? join(dataDir, 'themes')
  : join(userConfigDirFor(homedir(), isDir), 'themes')

/**
 * Where synthesised overlay plugins go.
 *
 * SPEC 2 sketched these under `%TEMP%`, and they are not there. A shim's
 * subdirectories are links (junctions on Windows, symlinks on macOS and Linux)
 * into the user's real repositories, and every
 * temp-cleaning tool on Windows - Disk Cleanup, Storage Sense, the third-party
 * ones - walks `%TEMP%` and deletes what it finds. One that follows a reparse
 * point instead of unlinking it deletes the repo's `.claude/skills`, which is
 * an unrecoverable outcome for a directory Helm only borrowed.
 *
 * Under the data directory they are ours, they travel with a portable install,
 * and `cleanStaleShims` at startup is the only thing that removes them.
 */
export const shimRoot: string = join(dataDir, 'overlays')

/**
 * Where the per-session `--mcp-config` documents go.
 *
 * Under the data directory for the same reason the shims are, and one reason
 * more: each file carries a **bearer token** for Helm's loopback endpoint, and
 * a token written into `%TEMP%` is a token in a directory every process on the
 * machine enumerates by habit. Here it travels with a portable install, it
 * moves with `PORTABLE_EXECUTABLE_DIR` so a check writes its own, and
 * `cleanStaleMcpConfigs` is the only thing that removes what a crash left.
 *
 * Not `overlays/`, though it could have been: a sweep that has to tell a shim
 * directory from a token file by filename is a sweep with a rule in it, and the
 * rule would be the thing that went wrong.
 */
export const mcpConfigDir: string = join(dataDir, 'mcp')
