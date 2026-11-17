import type { FlotiEvents, FlotiTheme, PluginBridge, SecretState, SettingValue } from '../src/types'

/**
 * A page bridge the framework helpers' tests drive: `window.floti`, as a
 * plugin page has it. Events fire when the
 * test says, and every read waits until the test answers it, so the order of
 * a read and an event is the test's to choose. Property and event behave as
 * the real bridge does: the property changes first, then the event fires.
 */

export interface Pending<T> {
  /** Resolves the call, then waits for whatever was waiting on it. */
  answer(value: T): Promise<void>
}

interface Call<T> extends Pending<T> {
  key: string
}

export interface FakeBridge {
  readonly settingsReads: Array<Pending<Record<string, SettingValue>>>
  readonly stateReads: Array<Call<SecretState>>
  readonly requests: Array<Call<SecretState>>
  emit<K extends keyof FlotiEvents>(event: K, data: FlotiEvents[K]): void
  /** How many listeners `event` has. */
  listeners(event: keyof FlotiEvents): number
}

export const DARK: FlotiTheme = { kind: 'dark', tokens: {} as FlotiTheme['tokens'], radius: 6, density: 'comfortable' }
export const LIGHT: FlotiTheme = { kind: 'light', tokens: {} as FlotiTheme['tokens'], radius: 6, density: 'compact' }

function pending<T>(): { promise: Promise<T>; call: Pending<T> } {
  let resolve: (value: T) => void = () => undefined
  const promise = new Promise<T>((done) => {
    resolve = done
  })
  return {
    promise,
    call: {
      async answer(value) {
        resolve(value)
        await promise
      }
    }
  }
}

/** Puts a fake bridge on the global object as `floti`, in place of any before it. */
export function installBridge(): FakeBridge {
  removeBridge()
  let theme = DARK
  let visible = false
  const listeners = new Map<keyof FlotiEvents, Set<(data: never) => void>>()
  const settingsReads: Array<Pending<Record<string, SettingValue>>> = []
  const stateReads: Array<Call<SecretState>> = []
  const requests: Array<Call<SecretState>> = []

  const keyed = (into: Array<Call<SecretState>>, key: string): Promise<SecretState> => {
    const { promise, call } = pending<SecretState>()
    into.push({ ...call, key })
    return promise
  }

  const floti = {
    get theme() {
      return theme
    },
    get visible() {
      return visible
    },
    onHostComputer: true,
    settings: {
      get() {
        const { promise, call } = pending<Record<string, SettingValue>>()
        settingsReads.push(call)
        return promise
      }
    },
    secrets: {
      state: (key: string) => keyed(stateReads, key),
      request: (key: string) => keyed(requests, key)
    },
    on<K extends keyof FlotiEvents>(event: K, listener: (data: FlotiEvents[K]) => void) {
      const set = listeners.get(event) ?? new Set()
      set.add(listener as (data: never) => void)
      listeners.set(event, set)
      return () => {
        set.delete(listener as (data: never) => void)
      }
    }
  }
  ;(globalThis as { floti?: PluginBridge }).floti = floti as unknown as PluginBridge

  return {
    settingsReads,
    stateReads,
    requests,
    emit(event, data) {
      if (event === 'theme') theme = data as FlotiTheme
      if (event === 'visibility') visible = data as boolean
      for (const listener of [...(listeners.get(event) ?? [])]) (listener as (value: typeof data) => void)(data)
    },
    listeners: (event) => listeners.get(event)?.size ?? 0
  }
}

export function removeBridge(): void {
  delete (globalThis as { floti?: unknown }).floti
}
