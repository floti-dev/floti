import { describe, expect, it, vi } from 'vitest'
import { createReopen, type QuitPhase } from './reopen'

/**
 * Helm's reopen as index.ts wires it: `phase` is `hostStop`, `windows` the
 * open windows, and `finish()` the held `before-quit` ending, which calls
 * `quitFinished()` before `app.quit()`.
 */
function helmWith(start: { phase?: QuitPhase; windows?: number } = {}) {
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

describe('reopen while Helm quits', () => {
  it('opens no window in the quitting process', () => {
    const helm = helmWith()
    helm.startQuit()

    helm.reopen.activate()

    expect(helm.openWindow).not.toHaveBeenCalled()
    expect(helm.relaunch).not.toHaveBeenCalled()
  })

  it('starts a fresh Helm once the quit has finished', () => {
    const helm = helmWith()
    helm.startQuit()
    helm.reopen.activate()

    helm.finish()

    expect(helm.relaunch).toHaveBeenCalledTimes(1)
    expect(helm.openWindow).not.toHaveBeenCalled()
  })

  it('starts one fresh Helm however many times it was reopened', () => {
    const helm = helmWith()
    helm.startQuit()
    helm.reopen.activate()
    helm.reopen.activate()

    helm.finish()
    helm.reopen.activate()

    expect(helm.relaunch).toHaveBeenCalledTimes(1)
  })

  it('honours a reopen that comes after the quit finished, before the process exits', () => {
    const helm = helmWith()
    helm.startQuit()
    helm.finish()
    expect(helm.relaunch).not.toHaveBeenCalled()

    helm.reopen.activate()

    expect(helm.relaunch).toHaveBeenCalledTimes(1)
    expect(helm.openWindow).not.toHaveBeenCalled()
  })

  it('relaunches nothing when nobody reopened during the quit', () => {
    const helm = helmWith()
    helm.startQuit()

    helm.finish()

    expect(helm.relaunch).not.toHaveBeenCalled()
  })
})

describe('reopen with no quit underway', () => {
  it('opens the window when none is open', () => {
    const helm = helmWith({ windows: 0 })

    helm.reopen.activate()

    expect(helm.openWindow).toHaveBeenCalledTimes(1)
    expect(helm.relaunch).not.toHaveBeenCalled()
  })

  it('opens no second window while one is open', () => {
    const helm = helmWith({ windows: 1 })

    helm.reopen.activate()

    expect(helm.openWindow).not.toHaveBeenCalled()
  })

  it('relaunches nothing when a quit that nobody reopened during finishes later', () => {
    const helm = helmWith({ windows: 0 })
    helm.reopen.activate()

    helm.startQuit()
    helm.finish()

    expect(helm.relaunch).not.toHaveBeenCalled()
  })
})
