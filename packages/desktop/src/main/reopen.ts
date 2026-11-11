/**
 * Reopening Helm while it quits.
 *
 * The first `before-quit` is held while every service shuts down and each host
 * is told what becomes of its sessions (index.ts), and by then the window has
 * gone. A reopen in that wait - the Dock icon clicked, which Electron reports
 * as `activate` - must not open a window in the quitting process: its links
 * refuse everything and its plugins answer that Helm is shutting down. So the
 * reopen is kept instead, and honoured once the quit has finished: Helm calls
 * `app.relaunch()` before it exits, and a fresh Helm starts once the old one
 * is gone. With no quit underway a reopen opens the window, as it always has.
 */

/** Where the quit stands: the `hostStop` index.ts holds the first `before-quit` with. */
export type QuitPhase = 'running' | 'stopping' | 'stopped'

export interface ReopenHost {
  /** Where the quit stands now. */
  quitPhase: () => QuitPhase
  /** How many windows are open: `BrowserWindow.getAllWindows().length`. */
  windowCount: () => number
  /** Opens Helm's window. */
  openWindow: () => void
  /** Has a fresh Helm start once this one exits: `app.relaunch()`. */
  relaunch: () => void
}

export interface Reopen {
  /** The app's `activate`. */
  activate(): void
  /** The quit has finished, just before `app.quit()` lets the app exit. */
  quitFinished(): void
}

export function createReopen(host: ReopenHost): Reopen {
  let wanted = false
  let relaunched = false

  /**
   * Electron starts one more instance for every `app.relaunch()` call, so a
   * reopen asked for twice still starts a single Helm.
   */
  const relaunchOnce = (): void => {
    if (relaunched) return
    relaunched = true
    host.relaunch()
  }

  return {
    activate() {
      const phase = host.quitPhase()
      if (phase === 'running') {
        if (host.windowCount() === 0) host.openWindow()
        return
      }
      wanted = true
      // A reopen after the quit finished, in the moment before the process
      // exits, is still honoured: the relaunch takes effect at the exit.
      if (phase === 'stopped') relaunchOnce()
    },

    quitFinished() {
      if (wanted) relaunchOnce()
    }
  }
}
