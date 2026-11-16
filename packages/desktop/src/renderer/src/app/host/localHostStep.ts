import { useSyncExternalStore } from 'react'
import type { HelmBridge, LocalHostStep } from '../../../../shared/ipc'

/**
 * What main's link to this PC's host is doing (`LocalHostStep`), as the
 * window shows it: the one store every view that says so reads, from
 * `hostLink:step` as each step begins. The window can load after the first
 * steps were sent, so it asks for the step under way once
 * (`hostLink:currentStep`); an answer that comes after a step was heard is
 * older than it and is dropped. Null until main has begun a connect, or
 * while that answer has not come.
 */
export interface LocalHostStepStore {
  get(): LocalHostStep | null
  /** Calls `listener` on every change; returns the unsubscribe. */
  subscribe(listener: () => void): () => void
}

export function createLocalHostStep(
  bridge: Pick<HelmBridge, 'invoke' | 'on'>,
  log: (line: string) => void
): LocalHostStepStore {
  let step: LocalHostStep | null = null
  /** Whether a step was heard on the event, which makes the asked one stale. */
  let heard = false
  const listeners = new Set<() => void>()
  const set = (next: LocalHostStep): void => {
    step = next
    for (const listener of listeners) listener()
  }
  bridge.on('hostLink:step', (next) => {
    heard = true
    set(next)
  })
  bridge.invoke('hostLink:currentStep').then(
    (current) => {
      if (!heard && current !== null) set(current)
    },
    (error: unknown) => {
      log(`host: the local host's step was not read (${(error as Error).message})`)
    }
  )
  return {
    get: () => step,
    subscribe(listener) {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    }
  }
}

/** The local host's step under way, re-rendering on every change. */
export function useLocalHostStep(store: LocalHostStepStore): LocalHostStep | null {
  return useSyncExternalStore(store.subscribe, store.get)
}
