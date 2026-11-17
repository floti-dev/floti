import { describe, expect, it, vi } from 'vitest'
import { createReopen, type QuitPhase } from './reopen'

/**
 * The desktop's reopen as index.ts wires it: `phase` is `hostStop`, `windows` the
 * open windows, and `finish()` the held `before-quit` ending, which calls
 * `quitFinished()` before `app.quit()`.
 */
function desktopWith(start: { phase?: QuitPhase; windows?: number } = {}) {
  let phase: QuitPhase = start.phase ?? 'running'
  let windows = start.windows ?? 0
  const openWindow = vi.fn(() => {
    windows += 1
  })
  const relaunch = vi.fn()
  const reopen = createReopen({
    quitPhase: () => phase,
    windowCount: () => windows,
    openWindow,
    relaunch
  })
  return {
    reopen,
    openWindow,
    relaunch,
    /** The window closed and the quit began: `hostStop` is 'stopping'. */
    startQuit() {
      windows = 0
      phase = 'stopping'
    },
    /** The held quit ended: `hostStop` is 'stopped', then `quitFinished()`. */
    finish() {
      phase = 'stopped'
      reopen.quitFinished()
    }
  }
}

describe('reopen while Floti quits', () => {
  it('opens no window in the quitting process', () => {
    const desktop = desktopWith()
    desktop.startQuit()

    desktop.reopen.activate()

    expect(desktop.openWindow).not.toHaveBeenCalled()
    expect(desktop.relaunch).not.toHaveBeenCalled()
  })

  it('starts a fresh Floti once the quit has finished', () => {
    const desktop = desktopWith()
    desktop.startQuit()
    desktop.reopen.activate()

    desktop.finish()

    expect(desktop.relaunch).toHaveBeenCalledTimes(1)
    expect(desktop.openWindow).not.toHaveBeenCalled()
  })

  it('starts one fresh Floti however many times it was reopened', () => {
    const desktop = desktopWith()
    desktop.startQuit()
    desktop.reopen.activate()
    desktop.reopen.activate()

    desktop.finish()
    desktop.reopen.activate()

    expect(desktop.relaunch).toHaveBeenCalledTimes(1)
  })

  it('honours a reopen that comes after the quit finished, before the process exits', () => {
    const desktop = desktopWith()
    desktop.startQuit()
    desktop.finish()
    expect(desktop.relaunch).not.toHaveBeenCalled()

    desktop.reopen.activate()

    expect(desktop.relaunch).toHaveBeenCalledTimes(1)
    expect(desktop.openWindow).not.toHaveBeenCalled()
  })

  it('relaunches nothing when nobody reopened during the quit', () => {
    const desktop = desktopWith()
    desktop.startQuit()

    desktop.finish()

    expect(desktop.relaunch).not.toHaveBeenCalled()
  })
})

describe('reopen with no quit underway', () => {
  it('opens the window when none is open', () => {
    const desktop = desktopWith({ windows: 0 })

    desktop.reopen.activate()

    expect(desktop.openWindow).toHaveBeenCalledTimes(1)
    expect(desktop.relaunch).not.toHaveBeenCalled()
  })

  it('opens no second window while one is open', () => {
    const desktop = desktopWith({ windows: 1 })

    desktop.reopen.activate()

    expect(desktop.openWindow).not.toHaveBeenCalled()
  })

  it('relaunches nothing when a quit that nobody reopened during finishes later', () => {
    const desktop = desktopWith({ windows: 0 })
    desktop.reopen.activate()

    desktop.startQuit()
    desktop.finish()

    expect(desktop.relaunch).not.toHaveBeenCalled()
  })
})
