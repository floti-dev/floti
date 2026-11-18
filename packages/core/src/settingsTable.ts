import { BROWSER_SEARCH_ENGINES, browserSearchName, type BrowserSearch } from './browser/reach'
import { THEME_ID_MAX_LENGTH, THEME_ID_PATTERN } from './theme/themes'
import {
  AGENT_CHILDREN_PER_PARENT,
  AGENT_COMMANDS_PER_PARENT,
  AGENT_DEPTH,
  AGENT_SESSIONS_PER_HOST,
  AGENT_STARTS_PER_PARENT,
  CORNER_RADIUS,
  DEFAULT_SETTINGS,
  PANE_GAP,
  PINNED_PROJECTS_MAX,
  PROJECT_SHELL_HEIGHT_PCT,
  quitChoice,
  TERMINAL_FONT_SIZE,
  TERMINAL_SCROLLBACK,
  TRANSCRIPT_ARCHIVE_BYTES,
  type AppSettings
} from './types'

/**
 * Every setting the Settings page shows, described once: its label, what it
 * does, the section it sits in, the values it takes, its default, whether it
 * is kept per host, and what a session may do to it.
 *
 * Settings draws its labels and hints from here, and a session reads and
 * changes settings by it (`desktop_settings`, `desktop_set`), so the page and
 * the tools cannot drift apart. A test ties each entry's values to its
 * validator in `store/settings.ts`, which the window cannot import (it reaches
 * node), so `settingValueProblem` here checks a value as the validator would
 * before anything is written.
 *
 * What Floti remembers rather than what anyone chose (`STATE_KEYS`: the pane
 * layout, the window's size, first-run records and the like) is not a setting
 * and is not listed. Nothing here holds a secret: the plugin secret store and
 * the device and host keys are not settings.
 *
 * Built on first use rather than at load: `types.ts` re-exports this module and
 * this one reads `types.ts`'s constants, so a table built at load would read
 * them before they exist.
 */

/** The Settings sections a setting sits in, by the ids `SettingsSections` gives them. */
export type SettingSection =
  | 'general'
  | 'appearance'
  | 'terminal'
  | 'sessions'
  | 'files'
  | 'browser'
  | 'programs'
  | 'tools'
  | 'agents'
  | 'workspace'
  | 'archive'
  | 'updates'

export type SettingKey = keyof AppSettings

/** One value a choice offers, and what the page calls it. */
export interface SettingOption {
  value: string
  label: string
}

/** The values a setting takes. */
export type SettingValues =
  | { kind: 'boolean' }
  | { kind: 'choice'; options: readonly SettingOption[] }
  | { kind: 'integer'; min: number; max: number }
  /** A theme's id: lower-case letters, digits and dashes. */
  | { kind: 'theme' }
  /** Null for the theme's own, or `#rrggbb` in lower case. */
  | { kind: 'accent' }
  /** Null for the built-in stack, or one family's name. */
  | { kind: 'font' }
  /** Null to have Floti find it, or a program's absolute path. */
  | { kind: 'program' }
  /** A list of absolute paths, at most `most` of them, and each once where `unique` says so. */
  | { kind: 'folders'; most: number | null; unique: boolean }

/**
 * What a session may do to a setting: change it to any value it takes, only
 * narrow it (`narrows`) where it decides what agents may reach or do or what
 * Floti or its sessions do on their own, or nothing (only the user changes
 * it, in Settings).
 */
export type SessionMay = 'change' | 'narrow' | 'nothing'

/** Where a setting is kept: once on this device, or once for each host. */
export type SettingScope = 'device' | 'host'

export interface SettingEntry {
  key: SettingKey
  /** What the page calls it. */
  label: string
  /** What it does, in a sentence or two: what a session reads, and the page's hint unless `hint` says otherwise. */
  description: string
  /** What the page writes under the label when that is not the description: nothing, or a sentence for each value. */
  hint?: 'none' | Readonly<Record<string, string>>
  section: SettingSection
  values: SettingValues
  scope: SettingScope
  session: SessionMay
  /** For a narrow-only setting: whether going from `from` to `to` narrows it or leaves it as it was. */
  narrows?: (from: unknown, to: unknown) => boolean
  /** For one a session may not change: why only the user changes it. */
  why?: string
  /** Its value on `host`, out of the settings read with that host's own keys laid over. */
  read: (settings: AppSettings, host: string) => unknown
  /** The patch that gives it `value` on `host`, from `settings` as they stand. */
  patch: (settings: AppSettings, host: string, value: unknown) => Partial<AppSettings>
  /** Its default on `host`. */
  defaultOn: (host: string) => unknown
}

/**
 * Keys of `AppSettings` that are state rather than settings, each with why the
 * page does not show it. Every validated key is a setting or one of these.
 */
export const STATE_KEYS: Readonly<Partial<Record<SettingKey, string>>> = {
  windowBounds: 'the window’s size and place, remembered',
  paneLayout: 'the panes and tabs, remembered',
  firstRunCompletedAt: 'when setup was finished',
  walkthrough: 'where the guided first run stands',
  railHidden: 'what a right-click took off the rail',
  browserProjectUrls: 'the last address each project’s browser was on',
  hosts: 'the hosts on other computers, added and removed in Settings > Hosts with their own dialog',
  profilesCopiedToHostAt: 'when profiles were copied into the host',
  pluginsCopiedToHostAt: 'when plugins were copied into the host',
  noticesSeen: 'the newest notice shown from each host'
}

const offOnly = (from: unknown, to: unknown): boolean => to === false || to === from
const onOnly = (from: unknown, to: unknown): boolean => to === true || to === from
const noHigher = (from: unknown, to: unknown): boolean =>
  typeof from === 'number' && typeof to === 'number' && to <= from
/**
 * Folders narrow when every one kept was already there, spelt exactly as it
 * was: a session may stop Floti scanning one, never add one. Exactly, on every
 * OS: on macOS and Linux a path that differs only in case is another folder,
 * and on Windows a change of case alone is refused, which costs nothing.
 */
const fewerFolders = (from: unknown, to: unknown): boolean =>
  Array.isArray(from) && Array.isArray(to) && to.every((folder) => typeof folder === 'string' && from.includes(folder))

/**
 * What quitting leaves running, least first: end < ask < keep. A move narrows
 * when it goes toward end or stays; any move toward keep widens. `ask` is not
 * a value `quitSessions` takes yet (`QUIT_SESSIONS`); it has its place here so
 * the rule holds if it becomes one.
 */
const QUIT_ORDER = ['end', 'ask', 'keep'] as const
const quitNarrows = (from: unknown, to: unknown): boolean => {
  const was = QUIT_ORDER.indexOf(from as (typeof QUIT_ORDER)[number])
  const now = QUIT_ORDER.indexOf(to as (typeof QUIT_ORDER)[number])
  return was !== -1 && now !== -1 && now <= was
}

const options = (values: readonly string[], label: (value: string) => string): SettingOption[] =>
  values.map((value) => ({ value, label: label(value) }))

const integer = (bounds: { min: number; max: number }): SettingValues => ({
  kind: 'integer',
  min: bounds.min,
  max: bounds.max
})

/** An entry kept as one value on the device or the host, the common case. */
function plain(
  entry: Omit<SettingEntry, 'read' | 'patch' | 'defaultOn' | 'scope' | 'session'> &
    Partial<Pick<SettingEntry, 'scope' | 'session'>>
): SettingEntry {
  const key = entry.key
  return {
    scope: 'device',
    session: entry.narrows === undefined ? 'change' : 'narrow',
    ...entry,
    read: (settings) => settings[key],
    patch: (_settings, _host, value) => ({ [key]: value }) as Partial<AppSettings>,
    defaultOn: () => DEFAULT_SETTINGS[key]
  }
}

/** A host's switch for what its sessions' agents may reach: a session may only turn it off. */
const NARROW_AGENTS = { narrows: offOnly, scope: 'host' } as const

function build(): readonly SettingEntry[] {
  const searchHint = Object.fromEntries(
    BROWSER_SEARCH_ENGINES.map((search: BrowserSearch) => {
      const engine = browserSearchName(search)
      return [
        search,
        engine === null
          ? 'Off. Something that is not an address gets a sentence saying so, and nothing is fetched.'
          : `Something that is not an address goes to ${engine} when you press Enter. Nothing is sent while you type.`
      ]
    })
  )
  return [
    plain({
      key: 'claudePath',
      label: 'Claude CLI',
      description: 'The claude executable Floti runs. Found on PATH unless you set one.',
      section: 'programs',
      values: { kind: 'program' },
      session: 'nothing',
      why: 'it names the program every session runs, and Floti checks a program before it keeps it',
      scope: 'host'
    }),

    plain({
      key: 'scanRoots',
      label: 'Folders Floti scans',
      description:
        'The folders Floti looks in for workspaces and projects. Removing a folder only stops Floti scanning it.',
      section: 'workspace',
      values: { kind: 'folders', most: null, unique: false },
      scope: 'host',
      narrows: fewerFolders
    }),
    plain({
      key: 'pinnedProjects',
      label: 'Pinned projects',
      description: 'Shown first in the sidebar. Pin one with the star on its row.',
      section: 'workspace',
      values: { kind: 'folders', most: PINNED_PROJECTS_MAX, unique: true },
      scope: 'host'
    }),

    plain({
      key: 'theme',
      label: 'Theme',
      description: 'Which theme is on screen: the dark one, the light one, or whichever the system is in.',
      section: 'appearance',
      values: {
        kind: 'choice',
        options: [
          { value: 'system', label: 'Follow the system' },
          { value: 'light', label: 'Light' },
          { value: 'dark', label: 'Dark' }
        ]
      }
    }),
    plain({
      key: 'themeDark',
      label: 'Dark theme',
      description: 'The theme shown when the dark one is: a built-in’s id, such as nocturne, or one of your own.',
      section: 'appearance',
      values: { kind: 'theme' }
    }),
    plain({
      key: 'themeLight',
      label: 'Light theme',
      description: 'The theme shown when the light one is: a built-in’s id, such as daylight, or one of your own.',
      section: 'appearance',
      values: { kind: 'theme' }
    }),
    plain({
      key: 'paneGap',
      label: 'Space between panes',
      description: `Pixels of canvas between islands, ${String(PANE_GAP.min)} to ${String(PANE_GAP.max)}.`,
      section: 'appearance',
      values: integer(PANE_GAP)
    }),
    plain({
      key: 'cornerRadius',
      label: 'Corner radius',
      description: 'Panels take it; buttons, fields and popups are one pixel rounder.',
      section: 'appearance',
      values: integer(CORNER_RADIUS)
    }),
    plain({
      key: 'density',
      label: 'Density',
      description: 'How tightly lists and the tab strip pack. Text keeps its size.',
      section: 'appearance',
      values: {
        kind: 'choice',
        options: [
          { value: 'comfortable', label: 'Comfortable' },
          { value: 'compact', label: 'Compact' }
        ]
      }
    }),
    plain({
      key: 'accentColor',
      label: 'Accent',
      description: "The theme's own, or one of these, fitted to stay readable on it.",
      section: 'appearance',
      values: { kind: 'accent' }
    }),

    plain({
      key: 'terminalFontFamily',
      label: 'Font',
      description:
        'One family name. It goes in front of the built-in stack - Cascadia Mono, SF Mono, Menlo, Consolas - rather than replacing it, so anything it lacks still draws.',
      section: 'terminal',
      values: { kind: 'font' }
    }),
    plain({
      key: 'terminalFontSize',
      label: 'Size',
      description: `Point size, ${String(TERMINAL_FONT_SIZE.min)} to ${String(TERMINAL_FONT_SIZE.max)}.`,
      section: 'terminal',
      values: integer(TERMINAL_FONT_SIZE)
    }),
    plain({
      key: 'terminalCursorStyle',
      label: 'Cursor',
      description: 'The shape of the cursor in every terminal.',
      hint: 'none',
      section: 'terminal',
      values: {
        kind: 'choice',
        options: [
          { value: 'block', label: 'Block' },
          { value: 'underline', label: 'Underline' },
          { value: 'bar', label: 'Bar' }
        ]
      }
    }),
    plain({
      key: 'terminalCursorBlink',
      label: 'Blink the cursor',
      description: 'Whether the cursor blinks in every terminal.',
      hint: 'none',
      section: 'terminal',
      values: { kind: 'boolean' }
    }),
    plain({
      key: 'terminalScrollback',
      label: 'Scrollback',
      description: 'Lines of history each terminal keeps. Shrinking it discards what is already past that point.',
      section: 'terminal',
      values: integer(TERMINAL_SCROLLBACK)
    }),
    plain({
      key: 'terminalShell',
      label: 'Shell for project panes',
      description: 'Claude sessions are unaffected. A project page can override it.',
      section: 'programs',
      values: { kind: 'program' },
      session: 'nothing',
      why: 'it names the program every project shell runs',
      scope: 'host'
    }),
    plain({
      key: 'projectShellHeightPct',
      label: 'Shell height',
      description: 'Share of a project page the shell takes, up to half. Dragging its handle changes it too.',
      section: 'terminal',
      values: integer(PROJECT_SHELL_HEIGHT_PCT)
    }),

    plain({
      key: 'sessionMcp',
      label: 'Let Claude see the other sessions',
      description:
        'Sessions Floti hosts can list every Claude Code session running here - its name, its directory, whether it is busy or waiting on you - and, for the ones Floti started, what they are holding: child processes and listening ports. Not their conversations, and not the arguments they were launched with.',
      hint: {
        true: 'Sessions Floti hosts can list every Claude Code session running here - its name, its directory, whether it is busy or waiting on you - and, for the ones Floti started, what they are holding: child processes and listening ports. Each can also give itself a title saying what it is doing, shown beside its name here and in History. Not their conversations, and not the arguments they were launched with.',
        false: 'Off. Floti serves the tools to nobody, sessions are started without them and give themselves no titles.'
      },
      section: 'tools',
      values: { kind: 'boolean' },
      ...NARROW_AGENTS
    }),
    plain({
      key: 'restoreWithoutAsking',
      label: 'Resume after a crash without asking',
      description:
        'When Floti stops without shutting down, the next start reopens every session it was running, in the tab it had. Off, it lists them and asks which to reopen.',
      hint: {
        true: 'When Floti stops without shutting down, the next start reopens every session it was running, in the tab it had, and says what it could not.',
        false: 'When Floti stops without shutting down, the next start lists the sessions it was running and asks which to reopen.'
      },
      section: 'sessions',
      values: { kind: 'boolean' },
      // On relaunches agents with no person there: a session may only turn it off.
      narrows: offOnly
    }),
    {
      key: 'quitSessions',
      label: 'When you quit Floti',
      description:
        'What quitting Floti does to the sessions it started on a host: end them, after asking when any are running, or keep them running there. Kept on this device, for each host.',
      hint: {
        end: 'For this PC’s host. Quitting ends the sessions Floti started there, after asking when any are running. Kept on this device.',
        keep: 'For this PC’s host. Quitting leaves the sessions Floti started running there, and they are back in their tabs when Floti opens again. Kept on this device.'
      },
      section: 'sessions',
      values: {
        kind: 'choice',
        options: [
          { value: 'end', label: 'End the sessions it started' },
          { value: 'keep', label: 'Keep them running' }
        ]
      },
      scope: 'host',
      // Ordered by how much keeps running after quit: a session may move it only toward end.
      session: 'narrow',
      narrows: quitNarrows,
      read: (settings, host) => quitChoice(settings, host),
      patch: (settings, host, value) =>
        ({ quitSessions: { ...settings.quitSessions, [host]: value } }) as Partial<AppSettings>,
      defaultOn: (host) => quitChoice(DEFAULT_SETTINGS, host)
    },

    plain({
      key: 'sessionAgents',
      label: 'Sessions can start agents',
      description:
        'Every session you start gets the floti-agents tools and floti agent. A profile can turn it off for its own sessions.',
      section: 'tools',
      values: { kind: 'boolean' },
      ...NARROW_AGENTS
    }),
    plain({
      key: 'desktopMcp',
      label: "Sessions can use Floti's window",
      description:
        "Every session you start gets the floti-desktop tools and floti desktop: asked to show you something, it opens a file, Files, a project's page, History or Settings beside its tab, and the keyboard stays in its terminal. It can read these settings, and change them or narrow what agents may do, with a toast you can undo.",
      section: 'tools',
      values: { kind: 'boolean' },
      ...NARROW_AGENTS
    }),
    plain({
      key: 'showChildrenInParentTab',
      label: "Show children in their parent's tab",
      description:
        "On this device. A parent's tab shows its running children beside its terminal while its pane has room, and never takes the focus from it. Off, the parent shows alone, with its children as chips in its crumb and rows in the tree.",
      section: 'sessions',
      values: { kind: 'boolean' }
    }),
    plain({
      key: 'agentChildrenPerParent',
      label: 'Children per parent',
      description: 'Sessions one session may have running at once that it started. A start past it is refused.',
      section: 'agents',
      values: integer(AGENT_CHILDREN_PER_PARENT),
      narrows: noHigher,
      scope: 'host'
    }),
    plain({
      key: 'agentCommandsPerParent',
      label: 'Command children per parent',
      description:
        'Commands one session may have running at once that it started with floti agent run. Counted apart from the agent limits: a command takes none of their places, and an agent takes none of this one. A run past it is refused.',
      section: 'agents',
      values: integer(AGENT_COMMANDS_PER_PARENT),
      narrows: noHigher,
      scope: 'host'
    }),
    plain({
      key: 'agentSessionsPerHost',
      label: 'Agent sessions per host',
      description:
        'Sessions started by agents that may run on one host at once. Sessions you start yourself count toward none of these limits.',
      section: 'agents',
      values: integer(AGENT_SESSIONS_PER_HOST),
      narrows: noHigher,
      scope: 'host'
    }),
    plain({
      key: 'agentDepth',
      label: 'Depth',
      description:
        'How many generations a family may reach. 2 is a session, the sessions it starts and the ones they start, and no further.',
      section: 'agents',
      values: integer(AGENT_DEPTH),
      narrows: noHigher,
      scope: 'host'
    }),
    plain({
      key: 'agentStartsPerParent',
      label: 'Starts per parent',
      description: 'Sessions one session may start in its life, whether or not they are still running.',
      section: 'agents',
      values: integer(AGENT_STARTS_PER_PARENT),
      narrows: noHigher,
      scope: 'host'
    }),

    plain({
      key: 'filesWrap',
      label: 'Wrap long lines',
      description: 'Notes always wrap while you edit them.',
      section: 'files',
      values: { kind: 'boolean' }
    }),

    plain({
      key: 'browserReach',
      label: 'Where the pane may go',
      description:
        'Anywhere you navigate to, or this machine only: anything that is not localhost, 127.0.0.1 or ::1 is then refused with a sentence and nothing is fetched.',
      hint: {
        local:
          'This machine only. Anything that is not localhost, 127.0.0.1 or ::1 is refused with a sentence and nothing is fetched.',
        web: 'Anywhere. Floti still opens no page you did not ask for - the pane fetches the address you navigate to and nothing else.'
      },
      section: 'browser',
      values: {
        kind: 'choice',
        options: [
          { value: 'web', label: 'Anywhere I navigate to' },
          { value: 'local', label: 'This machine only' }
        ]
      },
      narrows: (from, to) => to === 'local' || to === from
    }),
    plain({
      key: 'browserSearch',
      label: 'Search from the address bar',
      description:
        'What the address bar does with something that is not an address: send it to a search engine when you press Enter, or say it is not an address. Nothing is sent while you type.',
      hint: searchHint,
      section: 'browser',
      values: {
        kind: 'choice',
        options: options(BROWSER_SEARCH_ENGINES, (search) => browserSearchName(search as BrowserSearch) ?? 'Off')
      },
      // An engine sends what is typed in the address bar to it: a session may only turn it off.
      narrows: (from, to) => to === 'off' || to === from
    }),
    plain({
      key: 'browserMcp',
      label: 'Let Claude drive the browser',
      description:
        'Sessions Floti hosts can open pages of their own, read them, click and type, and drive a page of yours only once you share it from the browser bar.',
      hint: {
        true: 'Sessions Floti hosts can open pages of their own, read them, click and type, and drive a page of yours only once you share it from the browser bar. Floti serves the tools on a loopback port with a token unique to each session.',
        false: 'Off. Floti opens no port at all and sessions are started without the tools, exactly as they were before.'
      },
      section: 'tools',
      values: { kind: 'boolean' },
      ...NARROW_AGENTS
    }),
    plain({
      key: 'browserMcpLocalOnly',
      label: 'Hold Claude to localhost',
      description:
        'Holds Claude’s browser tools to localhost, from a session on any host, even where the pane may go further. Off, they reach as far as the pane does, and never further. Kept on this device.',
      hint: {
        true: 'Claude’s browser tools reach only pages on this computer, from a session on any host, even where the pane may go further. You can still go anywhere yourself.',
        false: 'Claude’s tools reach as far as the pane does, and never further - the narrower of these two settings always wins.'
      },
      section: 'browser',
      values: { kind: 'boolean' },
      narrows: onOnly
    }),

    plain({
      key: 'transcriptArchiveMaxBytes',
      label: 'Keep at most',
      description: 'At the limit the oldest conversation is dropped whole. Lowering it can drop some at once.',
      section: 'archive',
      values: integer(TRANSCRIPT_ARCHIVE_BYTES),
      session: 'nothing',
      why: 'lowering it drops archived conversations at once, and an undo cannot bring them back',
      scope: 'host'
    }),

    plain({
      key: 'updateCheck',
      label: 'Update floti by itself',
      description:
        "This PC's host looks for a newer floti at most once a day and installs it without ending a session. Check now works either way.",
      section: 'updates',
      values: { kind: 'boolean' },
      scope: 'host',
      // On is a request Floti makes by itself: a session may only turn it off.
      narrows: offOnly
    })
  ]
}

let table: readonly SettingEntry[] | null = null
let byKey: ReadonlyMap<SettingKey, SettingEntry> | null = null

/** Every setting the Settings page shows, in the page's order. */
export function settingsTable(): readonly SettingEntry[] {
  table ??= build()
  return table
}

/** The entry for `key`, which the page and the tools look up by. Throws for a key that is not a setting. */
export function settingEntry(key: SettingKey): SettingEntry {
  byKey ??= new Map(settingsTable().map((entry) => [entry.key, entry]))
  const entry = byKey.get(key)
  if (entry === undefined) throw new Error(`${key} is not a setting the Settings page shows`)
  return entry
}

/** The entry named `name`, or null when no setting has that name. */
export function findSettingEntry(name: string): SettingEntry | null {
  return settingsTable().find((entry) => entry.key === name) ?? null
}

/** What the page writes under a setting's label while it holds `value`: its hint, or nothing. */
export function settingHint(key: SettingKey, value?: unknown): string | undefined {
  const entry = settingEntry(key)
  if (entry.hint === 'none') return undefined
  if (entry.hint === undefined) return entry.description
  return entry.hint[String(value)] ?? entry.description
}

/** A path absolute on Windows or on macOS and Linux: the host it is kept on may be either. */
const ABSOLUTE = /^(?:[a-zA-Z]:[\\/]|[\\/])/

const ACCENT_HEX = /^#[0-9a-f]{6}$/

/** Characters a font family may not hold: they would end the inline style it goes into. */
const FONT_PUNCTUATION = ";{}<>,\\/*\"'`"

/** The values `values` takes, as a phrase: "true or false", "one of web or local". */
export function describeSettingValues(values: SettingValues): string {
  switch (values.kind) {
    case 'boolean':
      return 'true or false'
    case 'choice':
      return `one of ${values.options.map((option) => option.value).join(', ')}`
    case 'integer':
      return `a whole number from ${String(values.min)} to ${String(values.max)}`
    case 'theme':
      return 'a theme’s id: lower-case letters, digits and dashes'
    case 'accent':
      return 'null for the theme’s own, or a colour as #rrggbb in lower case'
    case 'font':
      return 'null for the built-in stack, or one font family’s name'
    case 'program':
      return 'null to have Floti find it, or the program’s absolute path'
    case 'folders':
      return `a list of ${values.most === null ? '' : `at most ${String(values.most)} `}absolute folder paths${values.unique ? ', each once' : ''}`
  }
}

/** Why `value` is not one `values` takes, naming what it does take, or null when it is one. */
export function settingValueProblem(values: SettingValues, value: unknown): string | null {
  const fits = ((): boolean => {
    switch (values.kind) {
      case 'boolean':
        return typeof value === 'boolean'
      case 'choice':
        return typeof value === 'string' && values.options.some((option) => option.value === value)
      case 'integer':
        return (
          typeof value === 'number' && Number.isInteger(value) && value >= values.min && value <= values.max
        )
      case 'theme':
        return typeof value === 'string' && value.length <= THEME_ID_MAX_LENGTH && THEME_ID_PATTERN.test(value)
      case 'accent':
        return value === null || (typeof value === 'string' && ACCENT_HEX.test(value))
      case 'font':
        return (
          value === null ||
          (typeof value === 'string' &&
            value.trim() !== '' &&
            value.length <= 100 &&
            ![...value].some((ch) => ch.charCodeAt(0) < 0x20 || FONT_PUNCTUATION.includes(ch)))
        )
      case 'program':
        return value === null || (typeof value === 'string' && value.trim() !== '' && ABSOLUTE.test(value))
      case 'folders': {
        if (!Array.isArray(value)) return false
        if (values.most !== null && value.length > values.most) return false
        const seen = new Set<string>()
        for (const folder of value) {
          if (typeof folder !== 'string' || folder.trim() === '' || !ABSOLUTE.test(folder)) return false
          if (values.unique && seen.has(folder.toLowerCase())) return false
          seen.add(folder.toLowerCase())
        }
        return true
      }
    }
  })()
  return fits ? null : `it takes ${describeSettingValues(values)}`
}

/** `value` as the page would say it: on or off, a choice's label, a number, a list. */
export function settingValueWords(values: SettingValues, value: unknown): string {
  if (values.kind === 'boolean') return value === true ? 'on' : 'off'
  if (values.kind === 'choice') {
    return values.options.find((option) => option.value === value)?.label ?? String(value)
  }
  if (value === null) {
    if (values.kind === 'accent') return 'the theme’s own'
    if (values.kind === 'font') return 'the built-in stack'
    if (values.kind === 'program') return 'found by Floti'
  }
  if (Array.isArray(value)) return value.length === 0 ? 'none' : value.join(', ')
  return String(value)
}
