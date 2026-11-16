import { useSyncExternalStore } from 'react'
import type { HostNoticeState } from '@helm/ui'
import type { LocalHostStep } from '../../../../shared/ipc'
import type { HostUnusable, HostUsability } from './link'
import type { LocalHostStepStore } from './localHostStep'

/** A step that says this PC's host cannot be used. */
export type UnusableStep = Extract<LocalHostStep, { step: 'refused' | 'unreachable' }>

/**
 * Whether this PC's host can be used, from main's steps (`hostLink:step`):
 *
 * - `unusable`: main's last refused or unreachable step. It stays through the
 *   connects main tries after it (preparing, connecting, starting), so the
 *   window says it once rather than flickering at every retry, and goes once
 *   a connect is ready or main updates the host.
 * - `retrying`: main is trying it again now, after that step.
 * - `updating`: main is restarting it onto this run's floti, until the new
 *   host is ready or the update fails.
 */
export interface LocalHostUsable {
  unusable: UnusableStep | null
  retrying: boolean
  updating: Extract<LocalHostStep, { step: 'updating' }> | null
}

export const USABLE: LocalHostUsable = { unusable: null, retrying: false, updating: null }

/** Where this PC's host stands after `step` began, from where it stood. */
export function nextLocalHostUsable(last: LocalHostUsable, step: LocalHostStep | null): LocalHostUsable {
  if (step === null) return last
  switch (step.step) {
    case 'refused':
    case 'unreachable':
      return { unusable: step, retrying: false, updating: null }
    case 'preparing':
    case 'connecting':
    case 'starting':
      // An update starts the new host itself; it is still the update.
      if (last.unusable === null || last.retrying) return last
      return { ...last, retrying: true }
    case 'updating':
      return { unusable: null, retrying: false, updating: step }
    case 'ready':
      return USABLE
  }
}

export interface LocalHostUsableStore {
  get(): LocalHostUsable
  /** Calls `listener` on every change; returns the unsubscribe. */
  subscribe(listener: () => void): () => void
}

export function createLocalHostUsable(steps: Pick<LocalHostStepStore, 'get' | 'subscribe'>): LocalHostUsableStore {
  let state = nextLocalHostUsable(USABLE, steps.get())
  const listeners = new Set<() => void>()
  steps.subscribe(() => {
    const next = nextLocalHostUsable(state, steps.get())
    if (next === state) return
    state = next
    for (const listener of [...listeners]) listener()
  })
  return {
    get: () => state,
    subscribe(listener) {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    }
  }
}

/** Why the host cannot be used, as the page link reads it: one value per unusable step, so a refusal is told once. */
export function usabilityOf(store: LocalHostUsableStore): HostUsability {
  let from: UnusableStep | null = null
  let why: HostUnusable | null = null
  return {
    get() {
      const { unusable } = store.get()
      if (unusable !== from) {
        from = unusable
        why =
          unusable === null
            ? null
            : unusable.step === 'refused'
              ? { sentence: unusable.sentence, refused: unusable.versions }
              : { sentence: unusable.reason }
      }
      return why
    },
    subscribe: (listener) => store.subscribe(listener)
  }
}

/**
 * The one notice the window shows about this PC's host, or null: updating
 * over everything, then a host that cannot be used, then one that stopped
 * without being asked to (`lost`, from the window's own connection), which
 * says whether Floti starts a new one. A refusal naming this Floti as the
 * older side is fixed by updating Floti; any other, and a host that cannot be
 * reached, by trying again.
 */
export function localHostNotice(usable: LocalHostUsable, lost: { restarts: boolean } | null): HostNoticeState | null {
  if (usable.updating !== null) return { kind: 'updating', to: usable.updating.to }
  const { unusable } = usable
  if (unusable?.step === 'refused') {
    return {
      kind: 'refused',
      sentence: unusable.sentence,
      fix: unusable.versions.update === 'client' ? 'update-floti' : 'try-again',
      retrying: usable.retrying
    }
  }
  if (unusable?.step === 'unreachable') return { kind: 'unreachable', reason: unusable.reason, retrying: usable.retrying }
  if (lost !== null) return { kind: 'lost', restarts: lost.restarts }
  return null
}

/** Where this PC's host stands, re-rendering on every change. */
export function useLocalHostUsable(store: LocalHostUsableStore): LocalHostUsable {
  return useSyncExternalStore(store.subscribe, store.get)
}
