import { LOCAL_HOST_KEY, type PaneRef } from '@floti/core/types'
import { SETTINGS_SECTIONS, type SettingsSectionId } from '@floti/ui'
import type * as P from '../../../shared/host/protocol'
import type { DesktopCall, DesktopToolResult } from './host/desktopTools'

/**
 * `desktop_open`, as the window carries it out: what a session's agent asked
 * to show the user, beside that session's tab, the keyboard left where it was.
 *
 * The host has already resolved and checked every path: it sends a
 * `DesktopOpen` naming the known folder holding a path and the path. So the
 * window only places what it is told. A file, a project's page and a Settings
 * tab open beside the caller's tab (`openBeside` in `panes.ts`), or beside the
 * focused pane when the caller's tab is not in this window; Files is the
 * sidebar, pointed at the folder; History is the window's palette, which opens
 * over everything and takes the keyboard until it is closed.
 *
 * The answer is the sentence the agent reads, then the same as JSON for
 * `floti desktop open --json`.
 */

/** What the window does for `desktop_open`, given by the app. */
export interface DesktopOpenWindow {
  /**
   * The tab showing `session` of `host` in this window: its own tab, or the
   * tab of the nearest session above it, which shows it among its children.
   * Null when neither is here.
   */
  anchorOf(host: string, session: number): string | null
  /** Opens `ref` beside tab `anchor`, the focused pane standing in for null; the focus stays where it is. */
  openBeside(ref: PaneRef, anchor: string | null): void
  /** The file at `path` of `host` opens scrolled to `line`. */
  revealLine(host: string, path: string, line: number): void
  /** Points the Files view at `root` of `host` with the folder `path` opened. */
  showFolder(host: string, root: string, path: string): void
  /** Opens the History palette on `host` with `search` typed in. */
  showHistory(host: string, search: string): void
  /** Settings shows `section` from now on. */
  showSettingsSection(section: SettingsSectionId): void
}

/** Where a tab went, as the answer says it. */
type Beside = 'yourTab' | 'focusedPane'

const text = (sentence: string, structured: Record<string, unknown>): DesktopToolResult => ({
  content: [
    { type: 'text', text: sentence },
    { type: 'text', text: JSON.stringify(structured) }
  ]
})

const refused = (sentence: string): DesktopToolResult => ({ content: [{ type: 'text', text: sentence }], isError: true })

const onHost = (host: string): { host?: string } => (host === LOCAL_HOST_KEY ? {} : { host })

/** The last part of a path, as a tab names it. */
const baseName = (path: string): string => path.split(/[\\/]/).filter((part) => part !== '').at(-1) ?? path

const besideWords = (beside: Beside): string =>
  beside === 'yourTab' ? 'beside your tab' : 'beside the pane in front, because your tab is not in this window'

const SECTION_IDS: ReadonlySet<string> = new Set(SETTINGS_SECTIONS.map((section) => section.id))

/** What the host sent, if it is a `DesktopOpen` the window knows how to show. */
function targetOf(value: unknown): P.DesktopOpen | null {
  if (typeof value !== 'object' || value === null) return null
  const target = value as Record<string, unknown>
  const string = (key: string): boolean => typeof target[key] === 'string'
  switch (target.view) {
    case 'file':
      return string('root') && string('path') && (target.line === undefined || typeof target.line === 'number')
        ? (target as P.DesktopOpen)
        : null
    case 'folder':
      return string('root') && string('path') ? (target as P.DesktopOpen) : null
    case 'project':
      return string('path') ? (target as P.DesktopOpen) : null
    case 'history':
      return string('search') ? (target as P.DesktopOpen) : null
    case 'settings':
      return target.section === undefined || (string('section') && SECTION_IDS.has(target.section as string))
        ? (target as P.DesktopOpen)
        : null
    default:
      return null
  }
}

/** Carries out one `desktop_open` call in the window, and answers what it did. */
export function openForAgent(call: DesktopCall, window: DesktopOpenWindow): Promise<DesktopToolResult> {
  return Promise.resolve().then(() => {
    const target = targetOf(call.arguments)
    if (target === null) return refused('Floti’s window could not read what to open. Nothing opened.')
    const anchor = window.anchorOf(call.host, call.session)
    const beside: Beside = anchor === null ? 'focusedPane' : 'yourTab'
    switch (target.view) {
      case 'file': {
        const ref: PaneRef = { kind: 'file', root: target.root, path: target.path, ...onHost(call.host) }
        if (target.line !== undefined) window.revealLine(call.host, target.path, target.line)
        window.openBeside(ref, anchor)
        const at = target.line === undefined ? '' : `, at line ${String(target.line)}`
        return text(`Opened ${baseName(target.path)} ${besideWords(beside)}${at}.`, {
          opened: 'file',
          path: target.path,
          ...(target.line === undefined ? {} : { line: target.line }),
          beside
        })
      }
      case 'folder':
        window.showFolder(call.host, target.root, target.path)
        return text(
          `Showed ${baseName(target.path)} in the Files view, on ${target.root}, with its changed files marked.`,
          { opened: 'folder', root: target.root, path: target.path }
        )
      case 'project':
        window.openBeside({ kind: 'project', path: target.path, ...onHost(call.host) }, anchor)
        return text(`Opened the page of ${baseName(target.path)} ${besideWords(beside)}.`, {
          opened: 'project',
          path: target.path,
          beside
        })
      case 'history':
        window.showHistory(call.host, target.search)
        return text(
          `Opened History${target.search === '' ? '' : ` searching for “${target.search}”`}. It takes the keyboard until the user closes it.`,
          { opened: 'history', search: target.search }
        )
      case 'settings': {
        const section = target.section as SettingsSectionId | undefined
        if (section !== undefined) window.showSettingsSection(section)
        window.openBeside({ kind: 'settings' }, anchor)
        const label = SETTINGS_SECTIONS.find((each) => each.id === section)?.label
        return text(`Opened Settings${label === undefined ? '' : ` on ${label}`} ${besideWords(beside)}.`, {
          opened: 'settings',
          ...(section === undefined ? {} : { section }),
          beside
        })
      }
    }
  })
}
