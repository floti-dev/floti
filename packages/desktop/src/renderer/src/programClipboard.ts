import type { Terminal } from '@xterm/xterm'
import type { ClipboardWriteOutcome } from '../../shared/ipc'
import { copyToClipboard } from './clipboardNotice'

/**
 * A program's own copy, OSC 52, reaching this client's clipboard.
 *
 * A TUI that captures the mouse makes its own selection and copies it with
 * `ESC ] 52 ; <selection> ; <base64> BEL` (or `ST`). The program runs on the
 * host, which may be another computer, so only the terminal drawing it here
 * can put the text on the clipboard of the person using it. The host passes
 * the bytes through untouched; this is the client's xterm answering them.
 *
 * Only a write is answered. A read (`?`) gets no reply, so no program can
 * read the client's clipboard, and anything that is not a clean write - an
 * unknown selection, base64 that does not decode, bytes that are not UTF-8,
 * more than `PROGRAM_CLIPBOARD_MAX_BYTES` - writes nothing. Every OSC 52 is
 * consumed either way, so xterm draws none of it.
 *
 * A write lands only while this window has focus. Every client attached to a
 * session sees the same bytes, and a window nobody is using must not have its
 * clipboard replaced by a copy made at another.
 *
 * The host's redraw for a tab that attaches is written out from its parsed
 * screen (`crates/host/src/session/redraw.rs`), which keeps cells and modes
 * and no clipboard writes, so attaching never replays an old copy.
 */

/** OSC 52's number. */
const OSC_CLIPBOARD = 52

/** The most a program may put on the clipboard at once, decoded. */
export const PROGRAM_CLIPBOARD_MAX_BYTES = 1024 * 1024

/**
 * The selections that mean the clipboard: `c` (clipboard), `p` (primary), `s`
 * (selection) and none, which xterm and most terminals read as `s` `0`. Cut
 * buffers (`0`-`7`) and `q` are not this client's clipboard.
 */
const CLIPBOARD_SELECTION = /^[cps]*$/

/** Base64 as RFC 4648 spells it, padded or not; no whitespace. */
const BASE64 = /^[A-Za-z0-9+/]+={0,2}$/

const utf8 = new TextDecoder('utf-8', { fatal: true })

/**
 * The text an OSC 52 body asks to put on the clipboard, or null when it is a
 * read or anything else that must write nothing. `body` is what follows
 * `52;`: the selection, `;`, then the payload.
 */
export function clipboardWriteOf(body: string): string | null {
  const split = body.indexOf(';')
  if (split < 0) return null
  const selection = body.slice(0, split)
  const payload = body.slice(split + 1)
  if (!CLIPBOARD_SELECTION.test(selection)) return null
  if (!BASE64.test(payload)) return null
  const digits = payload.replace(/=+$/, '').length
  // Padded base64 is whole quads; unpadded can end short by one or two, never three.
  if (digits < payload.length ? payload.length % 4 !== 0 : digits % 4 === 1) return null
  // Each digit is six bits. Refused before decoding, so an oversized write
  // costs no allocation.
  if (Math.floor((digits * 6) / 8) > PROGRAM_CLIPBOARD_MAX_BYTES) return null

  let binary: string
  try {
    binary = atob(payload)
  } catch {
    return null
  }
  const bytes = Uint8Array.from(binary, (char) => char.charCodeAt(0))
  try {
    const text = utf8.decode(bytes)
    return text === '' ? null : text
  } catch {
    return null
  }
}

/**
 * Answers OSC 52 in `term`: a write goes through `write`, the hook the
 * terminal's own copy uses, and a refusal is said in `container` as that
 * copy's is. Returns what removes the handler.
 */
export function attachProgramClipboard(
  term: Terminal,
  container: HTMLElement,
  write: (text: string) => Promise<ClipboardWriteOutcome>
): { dispose: () => void } {
  return term.parser.registerOscHandler(OSC_CLIPBOARD, (body) => {
    const text = clipboardWriteOf(body)
    if (text !== null && document.hasFocus()) void copyToClipboard(container, text, write)
    return true
  })
}
