import { DEFAULT_SETTINGS, LOCAL_HOST_KEY, settingsTable, type AppSettings, type PluginInfo } from '@floti/core/types'
import { SETTINGS_SECTIONS } from '@floti/ui'
import { describe, expect, it } from 'vitest'
import { HOST_WORKSPACE_KEYS } from '../../../shared/host/records'
import { listSettings, setSettings, type DesktopSettingsWindow, type PluginSwitch } from './desktopSettings'

const BUILD_BOX = 'build-box'

const plugin = (id: string, enabled: boolean, tools: boolean | null): PluginInfo =>
  ({
    path: `C:\\plugins\\${id}`,
    id,
    name: id === 'trackr' ? 'Trackr' : id,
    enabled,
    agent: tools === null ? null : { enabled: tools, server: `floti-plugin-${id}`, instructions: null, tools: [] }
  }) as unknown as PluginInfo

const hostKept = (key: string): boolean => (HOST_WORKSPACE_KEYS as readonly string[]).includes(key)

/** `patch`'s settings this device keeps, and the ones a host keeps. */
const byKeeper = (patch: Partial<AppSettings>): [Partial<AppSettings>, Partial<AppSettings>] => [
  Object.fromEntries(Object.entries(patch).filter(([key]) => !hostKept(key))),
  Object.fromEntries(Object.entries(patch).filter(([key]) => hostKept(key)))
]

/**
 * A window over this device's settings and each host's own keys and plugins,
 * recording every write. What `initial` gives a host keeps goes to This PC's
 * host; `onBuildBox` is the build box's own.
 */
function windowWith(initial: Partial<AppSettings> = {}, onBuildBox: Partial<AppSettings> = {}) {
  const [mine, local] = byKeeper(initial)
  let device: AppSettings = { ...DEFAULT_SETTINGS, ...mine }
  const kept: Record<string, Partial<AppSettings>> = {
    [LOCAL_HOST_KEY]: { scanRoots: ['C:\\work', 'C:\\play'], ...local },
    [BUILD_BOX]: { scanRoots: ['/srv'], updateCheck: false, ...onBuildBox }
  }
  const plugins: Record<string, PluginInfo[]> = {
    [LOCAL_HOST_KEY]: [plugin('trackr', true, true), plugin('quiet', false, null)],
    [BUILD_BOX]: []
  }
  const saves: Array<[string, Partial<AppSettings>]> = []
  const switched: Array<[string, string, PluginSwitch, boolean]> = []
  const toasts: Array<{ title: string; detail: string | null; undo: () => Promise<string | null> }> = []
  const settingsOn = (host: string): AppSettings => ({ ...device, ...kept[host] })
  const window: DesktopSettingsWindow = {
    hosts: () => [
      { id: LOCAL_HOST_KEY, name: 'This PC' },
      { id: BUILD_BOX, name: 'Build box' }
    ],
    settingsOn: (host) => Promise.resolve(settingsOn(host)),
    pluginsOn: (host) => Promise.resolve(plugins[host] ?? []),
    save: (host, patch) => {
      saves.push([host, patch])
      for (const [key, value] of Object.entries(patch)) {
        if (hostKept(key)) kept[host] = { ...kept[host], [key]: value }
        else device = { ...device, [key]: value }
      }
      return Promise.resolve()
    },
    setPlugin: (host, path, which, on) => {
      switched.push([host, path, which, on])
      plugins[host] = (plugins[host] ?? []).map((each) =>
        each.path !== path
          ? each
          : which === 'on'
            ? { ...each, enabled: on }
            : ({ ...each, agent: { ...each.agent!, enabled: on } } as PluginInfo)
      )
      return Promise.resolve()
    },
    sessionName: (host, session) => (host === LOCAL_HOST_KEY && session === 7 ? 'lead' : `${host}:${String(session)}`),
    raise: (title, detail, undo) => toasts.push({ title, detail, undo })
  }
  return { window, saves, switched, toasts, settingsOn, plugins }
}

const call = (tool: string, args: unknown, host = LOCAL_HOST_KEY) => ({ host, session: 7, tool, arguments: args })
const set = (changes: unknown, extra: Record<string, unknown> = {}, host = LOCAL_HOST_KEY) =>
  call('desktop_set', { changes, ...extra }, host)
const texts = (result: { content: { text: string }[] }): string[] => result.content.map((block) => block.text)

describe('desktop_settings in the window', () => {
  it('lists every setting from the table with its label, description, value, values and default, and plugins’ switches', async () => {
    const { window } = windowWith({ agentDepth: 3 })
    const result = await listSettings(call('desktop_settings', {}), window)
    expect(result.isError).toBeUndefined()
    const [sentence, json] = texts(result)
    const listed = JSON.parse(json!) as {
      hosts: Array<{ id: string; yours: boolean }>
      settings: Array<Record<string, unknown>>
    }
    expect(listed.settings.map((each) => each.name)).toEqual([
      ...settingsTable().map((entry) => entry.key),
      'plugin.trackr.on',
      'plugin.trackr.tools',
      'plugin.quiet.on'
    ])
    const depth = listed.settings.find((each) => each.name === 'agentDepth')
    expect(depth).toEqual({
      name: 'agentDepth',
      label: 'Depth',
      section: 'agents',
      description: settingsTable().find((entry) => entry.key === 'agentDepth')!.description,
      values: 'a whole number from 1 to 8',
      default: 2,
      scope: 'host',
      session: 'narrow',
      byHost: { [LOCAL_HOST_KEY]: 3, [BUILD_BOX]: 2 }
    })
    // Kept per host: each host's own value, and each host's own default where they differ.
    expect(listed.settings.find((each) => each.name === 'scanRoots')?.byHost).toEqual({
      [LOCAL_HOST_KEY]: ['C:\\work', 'C:\\play'],
      [BUILD_BOX]: ['/srv']
    })
    expect(listed.settings.find((each) => each.name === 'quitSessions')).toMatchObject({
      byHost: { [LOCAL_HOST_KEY]: 'end', [BUILD_BOX]: 'keep' },
      default: { [LOCAL_HOST_KEY]: 'end', [BUILD_BOX]: 'keep' }
    })
    expect(listed.settings.find((each) => each.name === 'plugin.trackr.tools')).toMatchObject({
      byHost: { [LOCAL_HOST_KEY]: true },
      session: 'narrow'
    })
    expect(listed.hosts).toEqual([
      { id: LOCAL_HOST_KEY, name: 'This PC', yours: true },
      { id: BUILD_BOX, name: 'Build box', yours: false }
    ])
    // The words a person reads: every setting by name, with what it is and takes.
    expect(sentence).toContain(`agentDepth ${LOCAL_HOST_KEY}: 3 · ${BUILD_BOX}: 2`)
    expect(sentence).toContain('Depth (Agents, kept per host; narrow only')
    // This device's own: one value.
    expect(sentence).toContain('browserMcpLocalOnly = false')
    expect(sentence).toContain('Takes a whole number from 1 to 8. Default 2.')
    // No secret is a setting.
    expect(json).not.toMatch(/secret/i)
  })

  it('says which host could not be read rather than listing it as having nothing', async () => {
    const { window } = windowWith()
    const failing: DesktopSettingsWindow = {
      ...window,
      settingsOn: (host) => (host === BUILD_BOX ? Promise.reject(new Error('the link is down')) : window.settingsOn(host))
    }
    const listed = JSON.parse(texts(await listSettings(call('desktop_settings', {}), failing))[1]!) as {
      hosts: Array<{ id: string; unreadable?: string }>
      settings: Array<{ name: string; byHost?: Record<string, unknown> }>
    }
    expect(listed.hosts.find((host) => host.id === BUILD_BOX)?.unreadable).toBe('the link is down')
    expect(listed.settings.find((each) => each.name === 'scanRoots')?.byHost).toEqual({
      [LOCAL_HOST_KEY]: ['C:\\work', 'C:\\play']
    })
  })

  it('puts every setting in a section Settings has', () => {
    const sections = new Set<string>(SETTINGS_SECTIONS.map((section) => section.id))
    for (const entry of settingsTable()) expect(sections.has(entry.section), entry.key).toBe(true)
  })
})

describe('desktop_set in the window', () => {
  it('turns agents off through the Settings save, and the toast names the session with an Undo that puts it back', async () => {
    const w = windowWith()
    const result = await setSettings(set({ sessionAgents: false }), w.window)
    expect(result.isError).toBeUndefined()
    expect(w.saves).toEqual([[LOCAL_HOST_KEY, { sessionAgents: false }]])
    expect(w.settingsOn(LOCAL_HOST_KEY).sessionAgents).toBe(false)
    const [sentence, json] = texts(result)
    expect(sentence).toBe(
      'Changed for This PC (local): Sessions can start agents: off (sessionAgents) on This PC. The user sees a toast naming your session, with Undo. This PC follows at once: sessions it starts from now on get what it says.'
    )
    expect(JSON.parse(json!)).toEqual({
      changed: [{ name: 'sessionAgents', from: true, to: false, landed: 'on This PC' }],
      host: LOCAL_HOST_KEY,
      hostName: 'This PC'
    })
    expect(w.toasts.map(({ title, detail }) => ({ title, detail }))).toEqual([
      { title: 'lead changed a setting', detail: 'Sessions can start agents: off' }
    ])

    await expect(w.toasts[0]!.undo()).resolves.toBeNull()
    expect(w.saves.at(-1)).toEqual([LOCAL_HOST_KEY, { sessionAgents: true }])
    expect(w.settingsOn(LOCAL_HOST_KEY).sessionAgents).toBe(true)
  })

  it('changes several at once, and Undo puts back every one it changed, plugins’ switches included', async () => {
    const w = windowWith()
    await setSettings(
      set({ agentDepth: 1, browserReach: 'local', theme: 'dark', 'plugin.trackr.tools': false, scanRoots: ['C:\\work'] }),
      w.window
    )
    // This device's settings in one save, the host's own in another, then the plugin.
    expect(w.saves).toEqual([
      [LOCAL_HOST_KEY, { browserReach: 'local', theme: 'dark' }],
      [LOCAL_HOST_KEY, { agentDepth: 1, scanRoots: ['C:\\work'] }]
    ])
    expect(w.switched).toEqual([[LOCAL_HOST_KEY, 'C:\\plugins\\trackr', 'tools', false]])
    expect(w.toasts[0]?.title).toBe('lead changed 5 settings')

    await w.toasts[0]!.undo()
    const back = w.settingsOn(LOCAL_HOST_KEY)
    expect([back.agentDepth, back.browserReach, back.theme, back.scanRoots]).toEqual([
      2,
      'web',
      'system',
      ['C:\\work', 'C:\\play']
    ])
    expect(w.switched.at(-1)).toEqual([LOCAL_HOST_KEY, 'C:\\plugins\\trackr', 'tools', true])
  })

  it('refuses a change that would widen what agents may reach or do, saying the user makes it in Settings, and changes nothing', async () => {
    for (const [changes, says] of [
      [{ sessionAgents: true }, 'Sessions can start agents (sessionAgents) can only be narrowed from a session'],
      [{ agentDepth: 5 }, 'The user changes it in Settings > Agents.'],
      [{ browserMcpLocalOnly: false, browserReach: 'local' }, 'Settings > Browser'],
      [{ scanRoots: ['C:\\work', 'D:\\new'] }, 'Settings > Workspace'],
      [{ 'plugin.quiet.on': true }, 'The user turns it on in Settings > Plugins.'],
      [{ claudePath: 'C:\\other\\claude.exe' }, 'is changed only by the user, in Settings > General'],
      [{ transcriptArchiveMaxBytes: 2048 }, 'an undo cannot bring them back'],
      // What Floti or its sessions do on their own goes only toward doing less.
      [{ updateCheck: true }, 'Update floti by itself (updateCheck) can only be narrowed from a session'],
      [{ browserSearch: 'bing' }, 'Search from the address bar (browserSearch) can only be narrowed'],
      [{ quitSessions: 'keep' }, 'When you quit Floti (quitSessions) can only be narrowed'],
      [{ restoreWithoutAsking: true }, 'Settings > Sessions'],
      [{ scanRoots: ['C:\\WORK', 'C:\\play'] }, 'Folders Floti scans (scanRoots) can only be narrowed']
    ] as const) {
      const w = windowWith({ sessionAgents: false, browserMcpLocalOnly: true, updateCheck: false })
      const result = await setSettings(set(changes), w.window)
      expect(result.isError, JSON.stringify(changes)).toBe(true)
      expect(texts(result)[0]).toContain(says)
      expect(texts(result)[0]).toMatch(/Nothing was changed on This PC \(local\)\.$/)
      expect(w.saves).toEqual([])
      expect(w.switched).toEqual([])
      expect(w.toasts).toEqual([])
    }
  })

  it('refuses a value outside a setting’s values naming them, and a name that is not a setting', async () => {
    const w = windowWith()
    const result = await setSettings(
      set({
        theme: 'purple',
        agentDepth: 99,
        terminalCursorBlink: 'yes',
        paneLayout: null,
        nonsense: 1,
        'plugin.gone.on': false,
        constructor: 1,
        toString: 1
      }),
      w.window
    )
    expect(result.isError).toBe(true)
    const [sentence] = texts(result)
    expect(sentence).toContain('theme (Theme) takes one of system, light, dark; "purple" is not one.')
    expect(sentence).toContain('agentDepth (Depth) takes a whole number from 1 to 8; 99 is not one.')
    expect(sentence).toContain('terminalCursorBlink (Blink the cursor) takes true or false; "yes" is not one.')
    expect(sentence).toContain('paneLayout is not a setting but something Floti remembers')
    expect(sentence).toContain('nonsense is not one of Floti’s settings')
    // A name that only an object's prototype has is not mistaken for one.
    expect(sentence).toContain('constructor is not one of Floti’s settings')
    expect(sentence).toContain('toString is not one of Floti’s settings')
    const proto = await setSettings(call('desktop_set', JSON.parse('{"changes":{"__proto__":1}}')), w.window)
    expect(texts(proto)[0]).toBe(
      '__proto__ is not one of Floti’s settings: desktop_settings lists them by name. Nothing was changed on This PC (local).'
    )
    expect(sentence).toContain('plugin.gone.on is not a plugin switch on This PC: it has plugin.trackr.on, plugin.trackr.tools, plugin.quiet.on.')
    expect(w.saves).toEqual([])
  })

  it('changes a host setting on the caller’s own host, or the one named, and names that host in every answer', async () => {
    const w = windowWith()
    const onBuildBox = await setSettings(set({ quitSessions: 'end', scanRoots: [] }, {}, BUILD_BOX), w.window)
    expect(onBuildBox.isError).toBeUndefined()
    expect(w.saves).toEqual([
      [BUILD_BOX, { quitSessions: { [BUILD_BOX]: 'end' } }],
      [BUILD_BOX, { scanRoots: [] }]
    ])
    expect(w.toasts[0]?.title).toBe('build-box:7 changed 2 settings')
    expect(texts(onBuildBox)[0]).toContain(
      'Changed for Build box (build-box): When you quit Floti: End the sessions it started (quitSessions) on this device, for Build box; Folders Floti scans: none (scanRoots) on Build box.'
    )

    // A session on the build box turns agents off there, and only there.
    const fromRemote = await setSettings(set({ sessionAgents: false }, {}, BUILD_BOX), w.window)
    const [sentence, json] = texts(fromRemote)
    expect(w.saves.at(-1)).toEqual([BUILD_BOX, { sessionAgents: false }])
    expect(w.settingsOn(BUILD_BOX).sessionAgents).toBe(false)
    expect(w.settingsOn(LOCAL_HOST_KEY).sessionAgents).toBe(true)
    expect(sentence).toBe(
      'Changed for Build box (build-box): Sessions can start agents: off (sessionAgents) on Build box. The user sees a toast naming your session, with Undo. Build box follows at once: sessions it starts from now on get what it says.'
    )
    expect(JSON.parse(json!)).toEqual({
      changed: [{ name: 'sessionAgents', from: true, to: false, landed: 'on Build box' }],
      host: BUILD_BOX,
      hostName: 'Build box'
    })

    // Naming another host: what this device keeps still lands here, and the answer says so.
    const named = await setSettings(set({ density: 'compact', updateCheck: false }, { host: 'local' }, BUILD_BOX), w.window)
    expect(texts(named)[0]).toContain(
      'Changed for This PC (local): Density: Compact (density) on this device; Update floti by itself: off (updateCheck) on This PC.'
    )
    const elsewhere = await setSettings(set({ terminalFontSize: 16 }, { host: BUILD_BOX }), w.window)
    expect(texts(elsewhere)[0]).toContain('Changed for Build box (build-box): Size: 16 (terminalFontSize) on this device.')
    expect(texts(elsewhere)[0]).not.toContain('follows at once')

    const unknown = await setSettings(set({ updateCheck: true }, { host: 'laptop' }), w.window)
    expect(texts(unknown)[0]).toBe(
      'laptop is not a host this window is connected to: local (This PC), build-box (Build box). Nothing was changed.'
    )
  })

  it('judges a narrow-only setting against the named host’s own value, never another host’s', async () => {
    const w = windowWith({}, { sessionAgents: false, agentDepth: 1 })
    // Off on the build box: turning it on there widens, though it is on here.
    const widen = await setSettings(set({ sessionAgents: true }, { host: BUILD_BOX }), w.window)
    expect(widen.isError).toBe(true)
    expect(texts(widen)[0]).toContain('Sessions can start agents (sessionAgents) can only be narrowed from a session')
    expect(texts(widen)[0]).toMatch(/Nothing was changed on Build box \(build-box\)\.$/)
    const deeper = await setSettings(set({ agentDepth: 2 }, {}, BUILD_BOX), w.window)
    expect(deeper.isError).toBe(true)
    expect(w.saves).toEqual([])

    // Here it is 2, so 2 from a build box session asking for This PC is nothing to change, and 1 narrows it.
    const same = await setSettings(set({ agentDepth: 2 }, { host: LOCAL_HOST_KEY }, BUILD_BOX), w.window)
    expect(texts(same)[0]).toBe('Nothing to change on This PC (local): agentDepth is already as asked.')
    const narrower = await setSettings(set({ agentDepth: 1 }, { host: LOCAL_HOST_KEY }, BUILD_BOX), w.window)
    expect(narrower.isError).toBeUndefined()
    expect(w.saves).toEqual([[LOCAL_HOST_KEY, { agentDepth: 1 }]])
  })

  it('names the host when it could not read its settings', async () => {
    const w = windowWith()
    const failing: DesktopSettingsWindow = {
      ...w.window,
      settingsOn: (host) => (host === BUILD_BOX ? Promise.reject(new Error('the link is down')) : w.window.settingsOn(host))
    }
    const result = await setSettings(set({ sessionAgents: false }, { host: BUILD_BOX }), failing)
    expect(texts(result)[0]).toBe(
      'Floti’s window could not read the settings on Build box (build-box): the link is down. Nothing was changed.'
    )
  })

  it('changes nothing and raises no toast when every value is already as asked', async () => {
    const w = windowWith()
    const result = await setSettings(set({ sessionAgents: true, 'plugin.trackr.on': true }), w.window)
    expect(texts(result)[0]).toBe('Nothing to change on This PC (local): sessionAgents, plugin.trackr.on are already as asked.')
    expect(w.saves).toEqual([])
    expect(w.switched).toEqual([])
    expect(w.toasts).toEqual([])
  })

  it('says when the save failed and nothing changed, and that Undo could not put it back', async () => {
    const w = windowWith()
    const failing: DesktopSettingsWindow = { ...w.window, save: () => Promise.reject(new Error('the database is locked')) }
    const result = await setSettings(set({ density: 'compact', 'plugin.trackr.on': false }), failing)
    expect(result.isError).toBe(true)
    expect(texts(result)[0]).toBe(
      'Floti could not save Density: Compact (density) on This PC (local): the database is locked. Trackr is on: off (plugin.trackr.on) was not tried. Nothing was changed.'
    )
    expect(w.toasts).toEqual([])
    expect(w.switched).toEqual([])

    await setSettings(set({ density: 'compact' }), w.window)
    w.window.save = () => Promise.reject(new Error('the host went away'))
    await expect(w.toasts[0]!.undo()).resolves.toBe('the host went away')
  })

  it('raises the toast for what was written when a later step fails, and says what changed and what did not', async () => {
    const w = windowWith()
    const failing: DesktopSettingsWindow = {
      ...w.window,
      setPlugin: (host, path, which, on) =>
        path.endsWith('quiet') ? Promise.reject(new Error('the plugin is busy')) : w.window.setPlugin(host, path, which, on)
    }
    w.plugins[LOCAL_HOST_KEY] = [plugin('trackr', true, true), plugin('quiet', true, null)]
    const result = await setSettings(
      set({ sessionAgents: false, 'plugin.trackr.tools': false, 'plugin.quiet.on': false, scanRoots: [] }),
      failing
    )
    expect(result.isError).toBe(true)
    expect(texts(result)[0]).toBe(
      'Changed for This PC (local): Sessions can start agents: off (sessionAgents) on This PC; Folders Floti scans: none (scanRoots) on This PC; Trackr’s tools: off (plugin.trackr.tools) on This PC, but Floti could not save quiet is on: off (plugin.quiet.on): the plugin is busy. The user sees a toast naming your session, with Undo, for what changed. This PC follows at once: sessions it starts from now on get what it says.'
    )
    expect(w.toasts.map(({ title, detail }) => ({ title, detail }))).toEqual([
      {
        title: 'lead changed 3 settings',
        detail: 'Sessions can start agents: off · Folders Floti scans: none · Trackr’s tools: off'
      }
    ])

    await expect(w.toasts[0]!.undo()).resolves.toBeNull()
    const back = w.settingsOn(LOCAL_HOST_KEY)
    expect([back.sessionAgents, back.scanRoots]).toEqual([true, ['C:\\work', 'C:\\play']])
    expect(w.switched.at(-1)).toEqual([LOCAL_HOST_KEY, 'C:\\plugins\\trackr', 'tools', true])
  })

  it('refuses a call with nothing to change', async () => {
    const w = windowWith()
    for (const args of [{}, { changes: {} }, { changes: [] }, null]) {
      const result = await setSettings(call('desktop_set', args), w.window)
      expect(result.isError).toBe(true)
      expect(texts(result)[0]).toContain('Say what to change')
      expect(texts(result)[0]).toContain('Nothing was changed on This PC (local).')
    }
  })
})
