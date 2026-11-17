import { app } from 'electron'
import { mkdirSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'
import type { AppMode } from '../shared/ipc'
import { flotiDataDirFor, userConfigDirFor } from './dataFolder'

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
 * `initDataDir` points `userData` there before anything reads it.
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

export const dataDir: string = portableDir
  ? join(portableDir, 'helm-data')
  : flotiDataDirFor(process.platform, process.env, homedir())

/** Must run before anything reads `app.getPath('userData')`. */
export function initDataDir(): void {
  mkdirSync(dataDir, { recursive: true })
  app.setPath('userData', dataDir)
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
 * where the user's config folder is the host's own (`userConfigDirFor`),
 * `~/.config/floti`, and the isolated base is the per-OS folder `isolate.mjs`
 * names (`%LOCALAPPDATA%\Floti Dev` on Windows).
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
  : join(userConfigDirFor(homedir()), 'templates')

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
  : join(userConfigDirFor(homedir()), 'themes')
