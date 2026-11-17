/**
 * `@helm/core` - what Floti's clients share, and `desktop.db`.
 *
 * The local host runs sessions and reads and writes the workspace; this
 * package keeps what the window and main need beside it: the shapes the host
 * answers in (`types.ts`, the one entry point a browser bundle imports values
 * from), `desktop.db` for what stays per device and the copy of an older
 * install's data into the host (`store/`).
 *
 * The one discipline (SPEC 5): nothing in this package may import Electron.
 * It is enforced by ESLint, not by convention - see `eslint.config.js`.
 */

export * from './types'
export type { ArchiveRole, ArchiveRow, ArchiveTail } from './archive/transcript'
export type { HistoryLine, HistoryTail, TranscriptFile } from './discovery/history'
export { isInsideRoot } from './files/inside'
export * from './store'
export type { UsageRow, UsageTail } from './usage/transcripts'
export * from './theme/load'
