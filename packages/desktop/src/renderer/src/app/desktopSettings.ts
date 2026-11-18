import {
  describeSettingValues,
  findSettingEntry,
  HOST_SETTING_KEYS,
  LOCAL_HOST_KEY,
  settingsTable,
  settingValueProblem,
  settingValueWords,
  STATE_KEYS,
  type AppSettings,
  type PluginInfo,
  type SettingEntry,
  type SettingKey,
  type SettingValues
} from '@floti/core/types'
import { SETTINGS_SECTIONS } from '@floti/ui'
import { HOST_WORKSPACE_KEYS } from '../../../shared/host/records'
import type { DesktopCall, DesktopToolResult } from './host/desktopTools'

/**
 * `desktop_settings` and `desktop_set`, as the window answers them: a
 * session reads Floti's settings and changes them, through the same save the
 * Settings page makes.
 *
 * What a setting is called, what it does, the values it takes and what a
 * session may do to it are the settings table's (`settingsTable` in core),
 * which the page draws its labels and hints from. Plugins' switches are not
 * app settings but each host's plugin state, so they are listed beside them
 * as `plugin.<id>.on` and `plugin.<id>.tools` and changed through the host's
 * own `plugins:setEnabled` and `plugins:setTools`.
 *
 * A set is judged whole before anything is written: every name must be a
 * setting, every value one it takes, and a setting that decides what agents,
 * or Floti on its own, may reach or do may only be narrowed. One refusal and
 * nothing in the call changes. What it does change shows the user a toast
 * naming the session, with Undo, which puts back every value the call
 * changed - also when a write fails partway, for whatever was already
 * written, and the answer then says what changed and what did not.
 *
 * A setting a host keeps (`HOST_WORKSPACE_KEYS`) is read from and written
 * to the host the call names, or the caller's own host when it names none:
 * the host whose connection delivered the call. Every window attached to
 * that host hears the change, and the host's sessions follow it. Every
 * answer names the host it read or changed, by its name and id, and a
 * narrow-only setting is judged against that host's own value.
 */

/** A host the window reaches, as the tools name it. */
export interface SettingsHost {
  id: string
  name: string
}

/** What the window does for the settings tools, given by the app. */
export interface DesktopSettingsWindow {
  /** This PC's host, then each host on another computer the window is connected to. */
  hosts(): readonly SettingsHost[]
  /** The settings as Settings shows them for `host`: this device's, with that host's own keys laid over. */
  settingsOn(host: string): Promise<AppSettings>
  /** `host`'s plugins, as Settings lists them. */
  pluginsOn(host: string): Promise<readonly PluginInfo[]>
  /** Saves `patch` as the Settings page does: the host's own keys on `host`, the rest on this device. */
  save(host: string, patch: Partial<AppSettings>): Promise<void>
  /** Turns a plugin (`on`) or its tools (`tools`) on or off on `host`, as Settings does. */
  setPlugin(host: string, path: string, which: PluginSwitch, on: boolean): Promise<void>
  /** What the window calls session `session` of `host`. */
  sessionName(host: string, session: number): string
  /** Shows the toast saying what changed, with its Undo: null once Undo put it back, else why not. */
  raise(title: string, detail: string | null, undo: () => Promise<string | null>): void
}

export type PluginSwitch = 'on' | 'tools'

/** What a session may do to a listed switch: as the table says it. */
type SessionMayWords = 'change' | 'narrow' | 'nothing'

const PLUGIN_NAME = /^plugin\.(.+)\.(on|tools)$/

const BOOLEAN: SettingValues = { kind: 'boolean' }

const sectionLabel = (section: string): string =>
  SETTINGS_SECTIONS.find((each) => each.id === section)?.label ?? section

const answered = (sentence: string, structured: unknown): DesktopToolResult => ({
  content: [
    { type: 'text', text: sentence },
    { type: 'text', text: JSON.stringify(structured) }
  ]
})

const refused = (sentence: string): DesktopToolResult => ({ content: [{ type: 'text', text: sentence }], isError: true })

const readable = (error: unknown): string => (error instanceof Error ? error.message : String(error))

const same = (a: unknown, b: unknown): boolean => JSON.stringify(a) === JSON.stringify(b)

/** A host as an answer names it: its name, then its id. */
const named = (host: SettingsHost): string => `${host.name} (${host.id})`

/** One plugin switch on one host. */
interface PluginSwitchOn {
  plugin: PluginInfo & { id: string }
  which: PluginSwitch
  value: boolean
}

/** The switches `plugins` give, by name. A plugin with no id Floti could read has none a session can name. */
function pluginSwitches(plugins: readonly PluginInfo[]): Map<string, PluginSwitchOn> {
  const switches = new Map<string, PluginSwitchOn>()
  for (const plugin of plugins) {
    if (plugin.id === null) continue
    const named = plugin as PluginInfo & { id: string }
    switches.set(`plugin.${plugin.id}.on`, { plugin: named, which: 'on', value: plugin.enabled })
    if (plugin.agent !== null) {
      switches.set(`plugin.${plugin.id}.tools`, { plugin: named, which: 'tools', value: plugin.agent.enabled })
    }
  }
  return switches
}

const pluginLabel = (each: PluginSwitchOn): string =>
  each.which === 'on' ? `${each.plugin.name} is on` : `${each.plugin.name}’s tools`

const pluginDescription = (each: PluginSwitchOn): string =>
  each.which === 'on'
    ? `Whether the ${each.plugin.name} plugin runs on the host: its pages, panels, background and tools.`
    : `Whether new sessions are offered the ${each.plugin.name} plugin’s tools, as ${each.plugin.agent?.server ?? 'its own server'}.`

/** What a host's read gave: its settings and plugins, or why it could not be read. */
interface HostRead {
  host: SettingsHost
  settings: AppSettings | null
  plugins: readonly PluginInfo[] | null
  problem: string | null
}

async function readHost(window: DesktopSettingsWindow, host: SettingsHost): Promise<HostRead> {
  const [settings, plugins] = await Promise.allSettled([window.settingsOn(host.id), window.pluginsOn(host.id)])
  const problems = [settings, plugins].flatMap((each) => (each.status === 'rejected' ? [readable(each.reason)] : []))
  return {
    host,
    settings: settings.status === 'fulfilled' ? settings.value : null,
    plugins: plugins.status === 'fulfilled' ? plugins.value : null,
    problem: problems.length === 0 ? null : problems.join('; ')
  }
}

/** One line of the listing, as the JSON gives it. */
interface Listed {
  name: string
  label: string
  section: string
  description: string
  values: string
  default: unknown
  scope: 'device' | 'host'
  session: SessionMayWords
  /** For a setting kept on this device. */
  value?: unknown
  /** For one kept per host: each host's value, where it could be read. */
  byHost?: Record<string, unknown>
}

function listedEntry(entry: SettingEntry, reads: readonly HostRead[]): Listed {
  const base = {
    name: entry.key,
    label: entry.label,
    section: entry.section,
    description: entry.description,
    values: describeSettingValues(entry.values),
    scope: entry.scope,
    session: entry.session
  }
  if (entry.scope === 'device') {
    const local = reads[0]?.settings ?? null
    return { ...base, default: entry.defaultOn(LOCAL_HOST_KEY), value: local === null ? null : entry.read(local, LOCAL_HOST_KEY) }
  }
  const byHost: Record<string, unknown> = {}
  const defaults: Record<string, unknown> = {}
  for (const read of reads) {
    defaults[read.host.id] = entry.defaultOn(read.host.id)
    if (read.settings !== null) byHost[read.host.id] = entry.read(read.settings, read.host.id)
  }
  const one = new Set(Object.values(defaults).map((each) => JSON.stringify(each))).size <= 1
  return { ...base, default: one ? entry.defaultOn(LOCAL_HOST_KEY) : defaults, byHost }
}

function listedPlugins(reads: readonly HostRead[]): Listed[] {
  const byName = new Map<string, Listed>()
  for (const read of reads) {
    for (const [name, each] of pluginSwitches(read.plugins ?? [])) {
      const listed = byName.get(name) ?? {
        name,
        label: pluginLabel(each),
        section: 'plugins',
        description: pluginDescription(each),
        values: describeSettingValues(BOOLEAN),
        default: true,
        scope: 'host' as const,
        session: 'narrow' as const,
        byHost: {}
      }
      listed.byHost = { ...listed.byHost, [read.host.id]: each.value }
      byName.set(name, listed)
    }
  }
  return [...byName.values()]
}

const SESSION_WORDS: Record<SessionMayWords, string> = {
  change: 'you may change it',
  narrow: 'narrow only: you may narrow it, only the user widens it',
  nothing: 'only the user changes it'
}

function listingLine(listed: Listed): string {
  const value =
    listed.scope === 'device'
      ? `= ${JSON.stringify(listed.value)}`
      : Object.entries(listed.byHost ?? {})
          .map(([host, value]) => `${host}: ${JSON.stringify(value)}`)
          .join(' · ') || '(no host could be read)'
  return [
    `${listed.name} ${value}`,
    `  ${listed.label} (${sectionLabel(listed.section)}${listed.scope === 'host' ? ', kept per host' : ''}; ${SESSION_WORDS[listed.session]}). ${listed.description}`,
    `  Takes ${listed.values}. Default ${JSON.stringify(listed.default)}.`
  ].join('\n')
}

/** Answers `desktop_settings`: every setting, its value and what it takes, and each host's plugins' switches. */
export async function listSettings(call: DesktopCall, window: DesktopSettingsWindow): Promise<DesktopToolResult> {
  const reads = await Promise.all(window.hosts().map((host) => readHost(window, host)))
  const local = reads.find((read) => read.host.id === LOCAL_HOST_KEY)
  if (local === undefined || local.settings === null) {
    const where = local === undefined ? `this PC’s host (${LOCAL_HOST_KEY})` : named(local.host)
    return refused(`Floti’s window could not read its settings on ${where}: ${local?.problem ?? 'it is not there'}.`)
  }
  const settings = [...settingsTable().map((entry) => listedEntry(entry, reads)), ...listedPlugins(reads)]
  const hosts = reads.map((read) => ({
    id: read.host.id,
    name: read.host.name,
    yours: read.host.id === call.host,
    ...(read.problem === null ? {} : { unreadable: read.problem })
  }))
  const hostWords = hosts
    .map((host) => `${host.id} (${host.name}${host.yours ? ', your session’s' : ''}${host.unreadable === undefined ? '' : `, could not be read: ${host.unreadable}`})`)
    .join(', ')
  const sentence = [
    `Floti’s ${String(settings.length)} settings: each one’s name and value, what it is and does, the values it takes and its default. Change one with desktop_set, by name. One kept per host changes on your own session’s host unless you name another. Hosts: ${hostWords}.`,
    '',
    ...settings.map(listingLine)
  ].join('\n')
  return answered(sentence, { hosts, settings })
}

/** One change a set makes, judged and ready to write. */
type Change =
  | { kind: 'setting'; entry: SettingEntry; from: unknown; to: unknown }
  | { kind: 'plugin'; name: string; each: PluginSwitchOn; to: boolean }

const changeLabel = (change: Change): string =>
  change.kind === 'setting' ? change.entry.label : pluginLabel(change.each)

const changeName = (change: Change): string => (change.kind === 'setting' ? change.entry.key : change.name)

const changeWords = (change: Change): string =>
  change.kind === 'setting' ? settingValueWords(change.entry.values, change.to) : change.to ? 'on' : 'off'

/** Whether `value` reads as an object of names to values. */
const isChanges = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value) && Object.keys(value).length > 0

/** The change `name` asks for, judged against `settings` and `switches` as they are, or why it is refused. */
function judge(
  name: string,
  to: unknown,
  settings: AppSettings,
  switches: ReadonlyMap<string, PluginSwitchOn>,
  host: SettingsHost
): Change | string {
  const plugin = PLUGIN_NAME.exec(name)
  if (plugin !== null) {
    const each = switches.get(name)
    if (each === undefined) {
      const named = [...switches.keys()]
      return `${name} is not a plugin switch on ${host.name}${named.length === 0 ? ', which has no plugins' : `: it has ${named.join(', ')}`}.`
    }
    if (typeof to !== 'boolean') return `${name} (${pluginLabel(each)}) takes true or false; ${JSON.stringify(to)} is not one.`
    if (to && !each.value) {
      return `${pluginLabel(each)} can only be turned off from a session: turning it on would widen what agents or Floti may do. The user turns it on in Settings > Plugins.`
    }
    return { kind: 'plugin', name, each, to }
  }
  const entry = findSettingEntry(name)
  if (entry === null) {
    const state = Object.hasOwn(STATE_KEYS, name) ? STATE_KEYS[name as SettingKey] : undefined
    return state === undefined
      ? `${name} is not one of Floti’s settings: desktop_settings lists them by name.`
      : `${name} is not a setting but something Floti remembers (${state}), so a session does not change it.`
  }
  const where = `Settings > ${sectionLabel(entry.section)}`
  const problem = settingValueProblem(entry.values, to)
  if (problem !== null) return `${name} (${entry.label}) takes ${describeSettingValues(entry.values)}; ${JSON.stringify(to)} is not one.`
  if (entry.session === 'nothing') {
    return `${entry.label} (${name}) is changed only by the user, in ${where}: ${entry.why ?? 'a session does not change it'}.`
  }
  const from = entry.read(settings, host.id)
  if (entry.session === 'narrow' && !(entry.narrows?.(from, to) ?? false)) {
    return `${entry.label} (${name}) can only be narrowed from a session: ${settingValueWords(entry.values, to)} would widen what agents or Floti may do. The user changes it in ${where}.`
  }
  return { kind: 'setting', entry, from, to }
}

const hostKept = (key: string): boolean => (HOST_WORKSPACE_KEYS as readonly string[]).includes(key)

/** Where `change` lands: this device, or `host` for what a host keeps and for its plugins. */
function landing(change: Change, host: SettingsHost): string {
  if (change.kind === 'plugin' || hostKept(change.entry.key)) return `on ${host.name}`
  return change.entry.scope === 'host' ? `on this device, for ${host.name}` : 'on this device'
}

/** Whether `change` is to a setting that governs what a host's sessions run and may do. */
const governsSessions = (change: Change): boolean =>
  change.kind === 'setting' && (HOST_SETTING_KEYS as readonly string[]).includes(change.entry.key)

/** What a write did: the changes it made, and the ones it could not, with why. */
interface Written {
  done: Change[]
  failed: { changes: Change[]; problem: string } | null
  /** Changes after the failure, which were not tried. */
  untried: Change[]
}

/**
 * Writes `changes` on `host` as Settings writes them, in steps that land
 * whole or not at all: this device's settings in one save, the host's own in
 * another, then each plugin's switch. It stops at the first step that fails.
 */
async function write(
  window: DesktopSettingsWindow,
  host: string,
  changes: readonly Change[],
  settings: AppSettings
): Promise<Written> {
  const settingChanges = changes.filter((change): change is Extract<Change, { kind: 'setting' }> => change.kind === 'setting')
  const steps: Change[][] = [
    settingChanges.filter((change) => !hostKept(change.entry.key)),
    settingChanges.filter((change) => hostKept(change.entry.key)),
    ...changes.filter((change) => change.kind === 'plugin').map((change) => [change])
  ].filter((step) => step.length > 0)
  const done: Change[] = []
  let current = settings
  for (const [index, step] of steps.entries()) {
    try {
      const first = step[0]
      if (first?.kind === 'plugin') {
        await window.setPlugin(host, first.each.plugin.path, first.each.which, first.to)
      } else {
        let patch: Partial<AppSettings> = {}
        for (const change of step) {
          if (change.kind === 'setting') patch = { ...patch, ...change.entry.patch({ ...current, ...patch }, host, change.to) }
        }
        await window.save(host, patch)
        current = { ...current, ...patch }
      }
      done.push(...step)
    } catch (error) {
      return { done, failed: { changes: step, problem: readable(error) }, untried: steps.slice(index + 1).flat() }
    }
  }
  return { done, failed: null, untried: [] }
}

/** The same changes, back to what each was. */
const reversed = (changes: readonly Change[]): Change[] =>
  changes.map((change) =>
    change.kind === 'setting' ? { ...change, from: change.to, to: change.from } : { ...change, to: change.each.value }
  )

/**
 * Answers `desktop_set`: judges every change and refuses the call whole for
 * any refusal, writes the rest, and raises the toast with Undo for whatever
 * was written, saying what landed where and what failed.
 */
export async function setSettings(call: DesktopCall, window: DesktopSettingsWindow): Promise<DesktopToolResult> {
  const args = (typeof call.arguments === 'object' && call.arguments !== null ? call.arguments : {}) as Record<string, unknown>
  const hosts = window.hosts()
  const hostId = typeof args.host === 'string' ? args.host : call.host
  const host = hosts.find((each) => each.id === hostId)
  if (host === undefined) {
    return refused(
      `${hostId} is not a host this window is connected to: ${hosts.map((each) => `${each.id} (${each.name})`).join(', ')}. Nothing was changed.`
    )
  }
  if (!isChanges(args.changes)) {
    return refused(
      `Say what to change in changes: each setting’s name and its new value, such as {"sessionAgents": false}. Nothing was changed on ${named(host)}.`
    )
  }
  const names = Object.keys(args.changes)
  const needsPlugins = names.some((name) => PLUGIN_NAME.test(name))
  let settings: AppSettings
  let switches: Map<string, PluginSwitchOn>
  try {
    settings = await window.settingsOn(host.id)
    switches = needsPlugins ? pluginSwitches(await window.pluginsOn(host.id)) : new Map()
  } catch (error) {
    return refused(`Floti’s window could not read the settings on ${named(host)}: ${readable(error)}. Nothing was changed.`)
  }

  const judged = names.map((name) => judge(name, (args.changes as Record<string, unknown>)[name], settings, switches, host))
  const refusals = judged.filter((each): each is string => typeof each === 'string')
  if (refusals.length > 0) return refused(`${refusals.join(' ')} Nothing was changed on ${named(host)}.`)
  const changes = (judged as Change[]).filter((change) =>
    change.kind === 'setting' ? !same(change.from, change.to) : change.to !== change.each.value
  )
  if (changes.length === 0) {
    return answered(
      `Nothing to change on ${named(host)}: ${names.join(', ')} ${names.length === 1 ? 'is' : 'are'} already as asked.`,
      { changed: [], host: host.id, hostName: host.name }
    )
  }

  const { done, failed, untried } = await write(window, host.id, changes, settings)
  const listed = (some: readonly Change[]): string =>
    some.map((change) => `${changeLabel(change)}: ${changeWords(change)} (${changeName(change)})`).join('; ')
  if (done.length === 0) {
    return refused(
      `Floti could not save ${listed(failed?.changes ?? [])} on ${named(host)}: ${failed?.problem ?? 'it did not say why'}.${untried.length === 0 ? '' : ` ${listed(untried)} was not tried.`} Nothing was changed.`
    )
  }

  const who = window.sessionName(call.host, call.session)
  window.raise(
    done.length === 1 ? `${who} changed a setting` : `${who} changed ${String(done.length)} settings`,
    done.map((change) => `${changeLabel(change)}: ${changeWords(change)}`).join(' · '),
    async () => {
      let now: AppSettings
      try {
        now = await window.settingsOn(host.id)
      } catch (error) {
        return readable(error)
      }
      const back = await write(window, host.id, reversed(done), now)
      return back.failed === null ? null : back.failed.problem
    }
  )

  const changed = done.map((change) => `${changeLabel(change)}: ${changeWords(change)} (${changeName(change)}) ${landing(change, host)}`)
  const follows = done.some(governsSessions)
    ? ` ${host.name} follows at once: sessions it starts from now on get what it says.`
    : ''
  if (failed !== null) {
    return refused(
      `Changed for ${named(host)}: ${changed.join('; ')}, but Floti could not save ${listed(failed.changes)}: ${failed.problem}.${untried.length === 0 ? '' : ` ${listed(untried)} was not tried.`} The user sees a toast naming your session, with Undo, for what changed.${follows}`
    )
  }
  return answered(`Changed for ${named(host)}: ${changed.join('; ')}. The user sees a toast naming your session, with Undo.${follows}`, {
    changed: done.map((change) => ({
      name: changeName(change),
      from: change.kind === 'setting' ? change.from : change.each.value,
      to: change.to,
      landed: landing(change, host)
    })),
    host: host.id,
    hostName: host.name
  })
}
