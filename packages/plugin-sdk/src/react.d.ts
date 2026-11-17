import type { FlotiEvents, FlotiTheme, SecretState, SettingValue } from './types.js'

/** Calls `listener` on every `event`, always the latest `listener` passed. */
export function useFlotiEvent<K extends keyof FlotiEvents>(event: K, listener: (data: FlotiEvents[K]) => void): void

/** Floti's theme, re-rendering when it changes. */
export function useFlotiTheme(): FlotiTheme

/** Whether the surface is on screen. */
export function useFlotiVisible(): boolean

/** The plugin's settings, or null until they have been read. */
export function useFlotiSettings(): Record<string, SettingValue> | null

/** Whether a declared secret is ready (null until read), and a function that asks the user to add it. */
export function useSecret(key: string): [SecretState | null, () => Promise<void>]
