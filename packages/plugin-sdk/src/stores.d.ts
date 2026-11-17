import type { FlotiTheme, SecretState, SettingValue } from './types.js'

/**
 * A value Floti keeps current. `subscribe` calls `run` with the value straight
 * away and again whenever it changes, and returns the function that
 * unsubscribes. That is Svelte's store contract, so `$store` works in a Svelte
 * component as it is.
 */
export interface FlotiStore<T> {
  /** The value now. */
  readonly current: T
  subscribe(this: void, run: (value: T) => void): () => void
}

/** A declared secret's state: null until it has been read. */
export interface SecretStore extends FlotiStore<SecretState | null> {
  /** Opens Floti's dialog for adding the key, scoped to this plugin. Resolves with the state once it closes, which the store takes too. */
  request(): Promise<SecretState>
}

/** Floti's theme. The tokens are on the page as CSS variables already; this is for code that needs the values. */
export const theme: FlotiStore<FlotiTheme>

/** Whether the page is on screen. */
export const visible: FlotiStore<boolean>

/** The plugin's settings: null until they have been read, then kept current. */
export const settings: FlotiStore<Record<string, SettingValue> | null>

/** A declared secret's state. The same key is always the same store. */
export function secret(key: string): SecretStore
