/**
 * The page bridge, typed. Reference it once in a plugin's pages:
 *
 *   /// <reference types="@floti-dev/plugin-sdk/global" />
 *
 * Every page gets `window.floti`. A background module references
 * `@floti-dev/plugin-sdk/background` instead, for the `floti` it runs with.
 */
import type { FlotiBridge } from './types.js'

declare global {
  interface Window {
    /** The bridge in a plugin page. */
    readonly floti: FlotiBridge
  }
  /** The bridge in a plugin page. */
  const floti: FlotiBridge
}

export {}
