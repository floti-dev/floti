import type { FlotiContext, FlotiErrorCode, FlotiTheme } from '@floti-dev/plugin-sdk'

/**
 * The messages between a plugin page's bridge and the desktop page framing it.
 *
 * Two phases. A page announces itself with `HELLO` on `window.parent`, which
 * is the only thing it ever sends that way; Floti checks the message's origin
 * and source against the frames it made, and answers with `CONNECT` and a
 * `MessagePort` of the page's own. Everything after that goes over the port,
 * so no later message has to be told apart from anything else on the window,
 * and a page that is not one Floti framed never gets a port at all.
 */

export const HELLO = 'floti:hello'
export const CONNECT = 'floti:connect'

export interface HelloMessage {
  type: typeof HELLO
}

export interface ConnectMessage {
  type: typeof CONNECT
  context: FlotiContext
  /** Null while the relay has not read one yet; the page keeps the one it booted with. */
  theme: FlotiTheme | null
  visible: boolean
  /**
   * Whether Floti runs on macOS, where Cmd rather than Ctrl is the page's
   * editing modifier. The window answers it once, from the platform helper
   * Floti's own shortcuts use, so a frame never works it out on its own.
   */
  mac?: boolean
}

/** A key the page did not handle, for Floti's shortcuts. */
export interface KeyInit {
  key: string
  code: string
  ctrlKey: boolean
  shiftKey: boolean
  altKey: boolean
  metaKey: boolean
  repeat: boolean
}

export type FrameMessage =
  | { t: 'call'; id: number; method: string; args: unknown[] }
  | { t: 'cancel'; id: number }
  | { t: 'key'; key: KeyInit }
  | { t: 'title'; title: string | null }
  /** The page's surface should stay rather than be replaced as a preview. */
  | { t: 'keep' }

export type FlotiMessage =
  | { t: 'result'; id: number; ok: true; value: unknown }
  | { t: 'result'; id: number; ok: false; code: FlotiErrorCode; message: string }
  | { t: 'event'; name: string; data: unknown }

/** The frame's `name`: how a page knows where it is before it has said anything. */
export const CONTEXT_PREFIX = 'floti:'

export function contextName(context: FlotiContext): string {
  return `${CONTEXT_PREFIX}${JSON.stringify(context)}`
}
