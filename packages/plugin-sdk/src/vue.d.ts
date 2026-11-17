import type { MaybeRefOrGetter, Ref } from 'vue'
import type { FlotiEvents, FlotiTheme, SecretState, SettingValue } from './types.js'

/** Calls `listener` on every `event` until the effect scope it was called in ends. */
export function useFlotiEvent<K extends keyof FlotiEvents>(event: K, listener: (data: FlotiEvents[K]) => void): void

/** Floti's theme, kept current. */
export function useFlotiTheme(): Readonly<Ref<FlotiTheme>>

/** Whether the surface is on screen. */
export function useFlotiVisible(): Readonly<Ref<boolean>>

/** The plugin's settings, or null until they have been read. */
export function useFlotiSettings(): Readonly<Ref<Record<string, SettingValue> | null>>

/**
 * Whether a declared secret is ready (null until read), and a function that
 * asks the user to add it. `key` may be a ref or a getter.
 */
export function useSecret(key: MaybeRefOrGetter<string>): [Readonly<Ref<SecretState | null>>, () => Promise<void>]
