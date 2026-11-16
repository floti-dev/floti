import type { LocalHostStep } from '../../shared/ipc'
import type { LocalHostStepSaid } from './launcher'

/**
 * What main's link to this PC's host is doing (`LocalHostStep`): the one
 * place that knows, kept for `hostLink:currentStep` and told to every
 * listener (the window, on `hostLink:step`) as each step begins.
 *
 * An update lasts until the host it starts is reached or the update fails:
 * the `preparing`, `connecting` and `starting` steps that come while it is
 * under way are part of it and are not told, so the window keeps saying the
 * host is updating rather than flickering through the steps of its restart.
 */
export interface LocalHostSteps {
  /** The step under way, or null before the first connect began. */
  current(): LocalHostStep | null
  /** A step began now. */
  begin(step: LocalHostStepSaid): void
  /**
   * Puts `step` back as it was, its `since` kept: for a restart that failed
   * before it changed anything, which leaves the link where it stood. Null
   * leaves the current step.
   */
  restore(step: LocalHostStep | null): void
  /** Calls `listener` with every step from now on; returns the unsubscribe. */
  onStep(listener: (step: LocalHostStep) => void): () => void
}

/** The steps of an update's own restart, which the update stands for until it ends. */
const PART_OF_AN_UPDATE: ReadonlySet<LocalHostStep['step']> = new Set(['preparing', 'connecting', 'starting'])

export function createLocalHostSteps(now: () => number): LocalHostSteps {
  let current: LocalHostStep | null = null
  const listeners = new Set<(step: LocalHostStep) => void>()
  const set = (step: LocalHostStep): void => {
    current = step
    for (const listener of listeners) listener(step)
  }
  return {
    current: () => current,
    begin(said) {
      if (current?.step === 'updating' && PART_OF_AN_UPDATE.has(said.step)) return
      set({ ...said, since: now() } as LocalHostStep)
    },
    restore(step) {
      if (step !== null) set(step)
    },
    onStep(listener) {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    }
  }
}
