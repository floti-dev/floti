import {
  describeSettingValues,
  findSettingEntry,
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
 * setting, every value one it takes, and a setting that decides what agents
 * may reach or do may only be narrowed. One refusal and nothing in the call
 * changes. What it does change shows the user a toast naming the session,
 * with Undo, which puts back every value the call changed.
 *
 * The desktop tells only this PC's host the settings it keeps for its
 * sessions (`hostSettingsFor`); a host on another computer is not told them.
 * So the answer to a session there says so rather than claim its host
 * follows.
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
    return refused(`Floti’s window could not read its settings: ${local?.problem ?? 'this PC’s host is not there'}.`)
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
      return `${pluginLabel(each)} can only be turned off from a session: turning it on would widen what agents may reach or do. The user turns it on in Settings > Plugins.`
    }
    return { kind: 'plugin', name, each, to }
  }
  const entry = findSettingEntry(name)
  if (entry === null) {
    const state = STATE_KEYS[name as SettingKey]
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
    return `${entry.label} (${name}) can only be narrowed from a session: ${settingValueWords(entry.values, to)} would widen what agents may reach or do. The user changes it in ${where}.`
  }
  return { kind: 'setting', entry, from, to }
}

/** Writes `changes` on `host`, settings first, then plugins' switches, each as Settings writes it. */
async function write(
  window: DesktopSettingsWindow,
  host: string,
  changes: readonly Change[],
  settings: AppSettings
): Promise<void> {
  let patch: Partial<AppSettings> = {}
  for (const change of changes) {
    if (change.kind === 'setting') patch = { ...patch, ...change.entry.patch({ ...settings, ...patch }, host, change.to) }
  }
  if (Object.keys(patch).length > 0) await window.save(host, patch)
  for (const change of changes) {
    if (change.kind === 'plugin') await window.setPlugin(host, change.each.plugin.path, change.each.which, change.to)
  }
}

/** The same changes, back to what each was. */
const reversed = (changes: readonly Change[]): Change[] =>
  changes.map((change) =>
    change.kind === 'setting' ? { ...change, from: change.to, to: change.from } : { ...change, to: change.each.value }
  )

/** Answers `desktop_set`: judges every change, writes them all or none, and raises the toast with Undo. */
export async function setSettings(call: DesktopCall, window: DesktopSettingsWindow): Promise<DesktopToolResult> {
  const args = (typeof call.arguments === 'object' && call.arguments !== null ? call.arguments : {}) as Record<string, unknown>
  if (!isChanges(args.changes)) {
    return refused('Say what to change in changes: each setting’s name and its new value, such as {"sessionAgents": false}.')
  }
  const hosts = window.hosts()
  const hostId = typeof args.host === 'string' ? args.host : call.host
  const host = hosts.find((each) => each.id === hostId)
  if (host === undefined) {
    return refused(
      `${hostId} is not a host this window is connected to: ${hosts.map((each) => `${each.id} (${each.name})`).join(', ')}. Nothing was changed.`
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
    return refused(`Floti’s window could not read the settings on ${host.name}: ${readable(error)}. Nothing was changed.`)
  }

  const judged = names.map((name) => judge(name, (args.changes as Record<string, unknown>)[name], settings, switches, host))
  const refusals = judged.filter((each): each is string => typeof each === 'string')
  if (refusals.length > 0) return refused(`${refusals.join(' ')} Nothing was changed.`)
  const changes = (judged as Change[]).filter((change) =>
    change.kind === 'setting' ? !same(change.from, change.to) : change.to !== change.each.value
  )
  if (changes.length === 0) {
    return answered(`Nothing to change: ${names.join(', ')} ${names.length === 1 ? 'is' : 'are'} already as asked.`, {
      changed: [],
      host: host.id
    })
  }

  try {
    await write(window, host.id, changes, settings)
  } catch (error) {
    return refused(`Floti could not save that: ${readable(error)}.`)
  }

  const said = changes.map((change) => `${changeLabel(change)}: ${changeWords(change)}`)
  const named = changes.map((change) => `${changeLabel(change)}: ${changeWords(change)} (${changeName(change)})`)
  const who = window.sessionName(call.host, call.session)
  window.raise(
    changes.length === 1 ? `${who} changed a setting` : `${who} changed ${String(changes.length)} settings`,
    said.join(' · '),
    async () => {
      try {
        await write(window, host.id, reversed(changes), await window.settingsOn(host.id))
        return null
      } catch (error) {
        return readable(error)
      }
    }
  )

  const told = changes.some((change) => change.kind === 'setting' && change.entry.toldToHost)
  const follows = !told
    ? ''
    : call.host === LOCAL_HOST_KEY
      ? ' Floti’s host follows at once: sessions started from now on get what it says.'
      : ` This PC’s host follows at once, but ${window.hosts().find((each) => each.id === call.host)?.name ?? call.host}, the host your session runs on, is not told this setting: its sessions go on as they were.`
  return answered(
    `Changed ${named.join('; ')} on ${host.name}. The user sees a toast naming your session, with Undo.${follows}`,
    {
      changed: changes.map((change) => ({
        name: changeName(change),
        from: change.kind === 'setting' ? change.from : change.each.value,
        to: change.to
      })),
      host: host.id,
      ...(told ? { yourHostFollows: call.host === LOCAL_HOST_KEY } : {})
    }
  )
}
