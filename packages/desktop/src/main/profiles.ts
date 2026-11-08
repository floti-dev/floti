import { type BrowserWindow, dialog } from 'electron'
import type { Profile } from '@helm/core'
import type {
  EventChannel,
  EventPayload,
  ExportProfileResult,
  ImportProfileResult,
  SaveProfileRequest,
  SaveProfileResult
} from '../shared/ipc'

/**
 * The shape of the profiles as main reaches them, and the dialogs around them.
 *
 * The profiles live in the local host (`host/profiles.ts`), and every reader
 * of profiles in main reads that `ProfileService`. The save and open pickers
 * are Helm's native dialogs; the question before a delete is the window's own
 * (`DeleteProfileDialog`), asked before anything reaches main or the host.
 */

export interface ProfileService {
  /** The profiles as they are now. */
  list(): Promise<Profile[]>
  /** The same, as last known, for readers that cannot wait: the config console's and Files view's folders. */
  current(): Profile[]
  save(req: SaveProfileRequest): Promise<SaveProfileResult>
  /** Deletes a profile the host lists; false, with nothing sent, for one it does not. */
  remove(id: number): Promise<boolean>
  pin(ids: number[]): Promise<Profile[]>
  /** Asks where, then writes the profile there. */
  exportProfile(id: number): Promise<ExportProfileResult>
  /** Asks which file, then adds the profile it describes. */
  importProfile(): Promise<ImportProfileResult>
}

/** The file pickers a profile export and import open, wherever the profiles live. */
export interface ProfileDialogs {
  /** Where to export to, starting from `suggested`; null when cancelled. */
  chooseExportFile(suggested: string): Promise<string | null>
  /** Which file to import; null when cancelled. */
  chooseImportFile(): Promise<string | null>
}

export type EmitToWindow = <K extends EventChannel>(channel: K, payload: EventPayload<K>) => void

const PROFILE_FILE_FILTERS = [
  { name: 'Helm profile', extensions: ['yaml', 'yml'] },
  { name: 'All files', extensions: ['*'] }
]

/** The native file dialogs, on `window` when it is there. */
export function nativeProfileDialogs(window: () => BrowserWindow | null): ProfileDialogs {
  const shown = (): BrowserWindow | null => {
    const win = window()
    return win && !win.isDestroyed() ? win : null
  }
  return {
    async chooseExportFile(suggested) {
      const options: Electron.SaveDialogOptions = {
        title: 'Export profile',
        defaultPath: suggested,
        filters: PROFILE_FILE_FILTERS
      }
      const win = shown()
      const result = win ? await dialog.showSaveDialog(win, options) : await dialog.showSaveDialog(options)
      return result.canceled || !result.filePath ? null : result.filePath
    },

    async chooseImportFile() {
      const options: Electron.OpenDialogOptions = {
        title: 'Import profile',
        properties: ['openFile'],
        filters: PROFILE_FILE_FILTERS
      }
      const win = shown()
      const result = win ? await dialog.showOpenDialog(win, options) : await dialog.showOpenDialog(options)
      const file = result.filePaths[0]
      return result.canceled || file === undefined ? null : file
    }
  }
}
