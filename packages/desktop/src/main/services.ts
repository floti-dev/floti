import {
  openStore,
  readSettings,
  reconcileRunningSessions,
  settleWalkthrough,
  writeSettings,
  type AppSettings,
  type LostSession,
  type SavedPaneLayout,
  type Store
} from '@floti/core'
import { dbFile } from './paths'

/**
 * The main process's stateful bits, in one object rather than module-level
 * singletons: the spike modes never open a database, and a module that opened
 * one on import would create a file the fidelity harness has no use for.
 */

export interface Services {
  store: Store
  settings: AppSettings
  /**
   * What the previous run left behind when it stopped without shutting down -
   * a crash, or a kill from Task Manager: the sessions still claiming to be
   * running, reconciled to `lost` on the way in, and the panes as that run
   * last wrote them.
   *
   * Read together and before the window exists, because the window rewrites
   * `paneLayout` within a second of opening, without the sessions it can no
   * longer see - and that layout is the only record of where they were.
   */
  lost: { sessions: LostSession[]; layout: SavedPaneLayout | null }
}

export function createServices(): Services {
  const store = openStore({ file: dbFile })
  // The open that made the database is a first start: the walkthrough begins.
  settleWalkthrough(store)
  const settings = readSettings(store)
  return {
    store,
    settings,
    lost: { sessions: reconcileRunningSessions(store), layout: settings.paneLayout }
  }
}

export function updateSettings(services: Services, patch: Partial<AppSettings>): AppSettings {
  services.settings = writeSettings(services.store, patch)
  return services.settings
}

/**
 * An install that predates the setup pane, brought up to date.
 *
 * It has roots and no stamp, which would read as "never set up" and answer
 * with a setup pane over a working launcher; the stamp goes to the local host
 * with the roots on its first connect (`host/workspace.ts`). Done here rather than
 * during the first scan because the window is created immediately afterwards:
 * a stamp written a second later would show that pane and then take it away.
 */
export function adoptExistingProfile(services: Services): boolean {
  if (services.settings.firstRunCompletedAt !== null) return false
  if (services.settings.scanRoots.length === 0) return false
  updateSettings(services, { firstRunCompletedAt: new Date().toISOString() })
  return true
}
