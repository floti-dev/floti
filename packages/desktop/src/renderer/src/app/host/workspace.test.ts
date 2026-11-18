import { describe, expect, it } from 'vitest'
import { DEFAULT_SETTINGS, type AppSettings } from '@floti/core/types'
import { FakeConnection, fakeReach, unusableReach } from '../../../../../test/fake-connection'
import type * as P from '../../../../shared/host/protocol'
import type {
  EventChannel,
  EventPayload,
  DesktopBridge,
  OwnAppInfo,
  RequestChannel,
  RequestPayload,
  RequestResult
} from '../../../../shared/ipc'
import type { HostAdapter } from './router'
import { HOST_UNUSABLE_FOR_ACTIONS } from './link'
import { createWorkspaceAdapter } from './workspace'

const HUB = '/work dir/hub'
const ALPHA = '/work dir/hub/repos/alpha'
const PICKED = '/tools dir/claude.exe'
const PI = '/tools dir/pi.exe'

const UNREACHABLE = 'No local host answered on its socket.'

const hostSettings = (patch: Partial<P.HostSettings> = {}): P.HostSettings => ({
  scanRoots: [],
  pinnedProjects: [],
  firstRunCompletedAt: null,
  backendPaths: {},
  terminalShell: null,
  transcriptArchiveMaxBytes: DEFAULT_SETTINGS.transcriptArchiveMaxBytes,
  browserMcp: true,
  browserMcpLocalOnly: true,
  sessionMcp: true,
  desktopMcp: true,
  sessionAgents: true,
  agentChildrenPerParent: 4,
  agentSessionsPerHost: 8,
  agentDepth: 2,
  agentStartsPerParent: 16,
  agentCommandsPerParent: 4,
  updateCheck: true,
  restoreWithoutAsking: false,
  ...patch
})

const git = (dirty: number): P.GitState => ({ branch: 'main', detached: false, dirty, ahead: 0, behind: 0, error: null })

const scanResult = (dirty: number): P.DiscoveryResult => ({
  roots: [HUB],
  workspaces: [],
  projects: [
    {
      path: ALPHA,
      name: 'alpha',
      kind: 'repo',
      workspacePath: HUB,
      hasClaudeDir: true,
      inventory: { skills: 0, commands: 0, agents: 0, hooks: false, settings: false, claudeMd: true, mcp: false },
      git: git(dirty)
    }
  ],
  errors: [],
  scannedAt: '2026-10-30T10:00:00.000Z',
  durationMs: 12
})

const backend = (patch: Partial<P.BackendStatus> = {}): P.BackendStatus => ({
  installed: true,
  path: PICKED,
  source: 'setting',
  version: '2.1.5 (Claude Code)',
  semver: '2.1.5',
  tested: true,
  testedRange: { min: '2.1.0', max: '2.2.0' },
  configDir: '/fixture home/.claude',
  configDirExists: true,
  signedIn: { state: 'unknown' },
  signInSignal: 'no sign-in artefact was found',
  error: null,
  ...patch
})

const BUILD: OwnAppInfo = {
  version: '9.9.9',
  mode: 'dev',
  dataDir: '/data dir',
  dbFile: '/data dir/desktop.db',
  migrations: [],
  versions: { electron: '1', chrome: '2', node: '3' },
  windowsBuild: null,
  home: '/home',
  releasesUrl: 'https://example.invalid/releases'
}

/** What this PC's host says of itself. */
const HOST_INFO = {
  version: '0.4.1',
  logFile: '/home/me/.local/state/floti/logs/host.log',
  desktop: true
} as unknown as P.AppInfo

/** The desktop's main process as the window's bridge reaches it: its own settings, build and picker. */
function ownBridge(patch: Partial<AppSettings> = {}) {
  let settings: AppSettings = { ...DEFAULT_SETTINGS, ...patch }
  const writes: Array<Partial<AppSettings>> = []
  const picks: Array<string | null> = []
  const listeners = new Set<(settings: AppSettings) => void>()
  const bridge = {
    invoke: (channel: string, payload?: unknown): Promise<unknown> => {
      switch (channel) {
        case 'ownSettings:read':
          return Promise.resolve(settings)
        case 'ownSettings:write':
          writes.push(payload as Partial<AppSettings>)
          settings = { ...settings, ...(payload as Partial<AppSettings>) }
          return Promise.resolve(settings)
        case 'ownApp:info':
          return Promise.resolve(BUILD)
        case 'path:chooseFile':
          return Promise.resolve({ path: picks.shift() ?? null })
        default:
          return Promise.reject(new Error(`${channel} is not Floti's own`))
      }
    },
    on: (channel: string, listener: (settings: AppSettings) => void) => {
      if (channel === 'ownSettings:changed') listeners.add(listener)
      return () => listeners.delete(listener)
    }
  }
  return {
    bridge: bridge as unknown as Pick<DesktopBridge, 'invoke' | 'on'>,
    writes,
    picks,
    /** Main changed the desktop's own settings, as the browser pane's history does. */
    change: (next: Partial<AppSettings>) => {
      settings = { ...settings, ...next }
      for (const listener of listeners) listener(settings)
    }
  }
}

function world(connection: FakeConnection | null = new FakeConnection(), own = ownBridge()) {
  let host = hostSettings()
  connection
    ?.answer('settings:read', () => host)
    .answer('settings:write', (patch) => (host = { ...host, ...patch }))
    .answer('discovery:cached', () => [{ ...scanResult(1).projects[0]!, lastSeenAt: '2026-10-29T10:00:00.000Z', template: null }])
    .answer('discovery:scan', () => scanResult(2))
    .answer('git:refresh', () => ({ [ALPHA]: git(5) }))
    .answer('roots:accept', ({ path }) => (host = { ...host, scanRoots: [...host.scanRoots, path] }).scanRoots)
    .answer('setup:status', () => ({ backends: { 'claude-code': backend() } }))
    .answer('app:info', () => ({ ...HOST_INFO }))
    .answer('setup:locateBackend', () => backend())
    .answer('setup:complete', () => (host = { ...host, firstRunCompletedAt: '2026-10-30T11:00:00.000Z' }))
  const link = fakeReach(connection, UNREACHABLE)
  let focus: () => void = () => undefined
  const adapter = createWorkspaceAdapter({
    link,
    own: own.bridge,
    onFocus: (listener) => {
      focus = listener
    }
  })
  const emitted: Array<{ channel: string; payload: unknown }> = []
  return {
    connection: connection ?? new FakeConnection(),
    link,
    adapter,
    own,
    emitted,
    listen: () => {
      for (const channel of ['settings:changed', 'discovery:updated', 'scan:status', 'git:updated'] as const) {
        on(adapter, channel, (payload) => emitted.push({ channel, payload }))
      }
    },
    focus: () => focus(),
    host: () => host
  }
}

function ask<K extends RequestChannel>(adapter: HostAdapter, channel: K, payload?: RequestPayload<K>): Promise<RequestResult<K>> {
  const route = adapter.requests?.[channel]
  if (route === undefined) throw new Error(`no route for ${channel}`)
  return route(payload as RequestPayload<K>)
}

function on<K extends EventChannel>(adapter: HostAdapter, channel: K, listener: (payload: EventPayload<K>) => void): () => void {
  const route = adapter.events?.[channel]
  if (route === undefined) throw new Error(`no route for ${channel}`)
  return route(listener)
}

/** Lets every answer already given be heard: the fake host answers on the next turns of the microtask queue. */
const settle = async (): Promise<void> => {
  for (let turn = 0; turn < 20; turn++) await Promise.resolve()
}

describe('the window’s workspace over its own connection', () => {
  it('keeps the host’s settings in the host and Floti’s own with Floti, and shows both laid together', async () => {
    const { adapter, own, connection, host } = world(new FakeConnection(), ownBridge({ theme: 'light' }))
    const next = await ask(adapter, 'settings:write', {
      scanRoots: [HUB],
      firstRunCompletedAt: '2026-10-30T09:00:00.000Z',
      theme: 'dark',
      terminalFontSize: 15,
      browserSearch: 'off'
    })
    expect(connection.payloads('settings:write')).toEqual([
      { scanRoots: [HUB], firstRunCompletedAt: '2026-10-30T09:00:00.000Z' }
    ])
    expect(own.writes).toEqual([{ theme: 'dark', terminalFontSize: 15, browserSearch: 'off' }])
    expect(host().scanRoots).toEqual([HUB])
    expect(next).toMatchObject({ scanRoots: [HUB], theme: 'dark', terminalFontSize: 15, browserSearch: 'off' })
    await expect(ask(adapter, 'settings:read')).resolves.toMatchObject({ scanRoots: [HUB], theme: 'dark' })
  })

  it('a write of only Floti’s own settings asks the host nothing', async () => {
    const { adapter, own, connection } = world()
    await ask(adapter, 'settings:write', { windowBounds: { width: 900, height: 700, x: 1, y: 2 } })
    expect(own.writes).toEqual([{ windowBounds: { width: 900, height: 700, x: 1, y: 2 } }])
    expect(connection.payloads('settings:write')).toEqual([])
  })

  it('keeps showing children in their parent’s tab with this device, and never tells the host', async () => {
    const { adapter, own, connection } = world()
    const next = await ask(adapter, 'settings:write', { showChildrenInParentTab: false })
    expect(own.writes).toEqual([{ showChildrenInParentTab: false }])
    expect(connection.payloads('settings:write')).toEqual([])
    expect(next).toMatchObject({ showChildrenInParentTab: false })
  })

  it('tells the window when either half changes, with the other laid over', async () => {
    const { adapter, own, connection, emitted, listen, link } = world()
    listen()
    await link.connect()
    await ask(adapter, 'settings:read')
    connection.push('settings:changed', hostSettings({ pinnedProjects: [ALPHA] }))
    own.change({ theme: 'dark' })
    await settle()
    const told = emitted.filter((e) => e.channel === 'settings:changed').map((e) => e.payload as AppSettings)
    expect(told.at(-1)).toMatchObject({ pinnedProjects: [ALPHA], theme: 'dark' })
  })

  it('a change to Floti’s own settings before the host’s are known tells the window both halves, never Floti’s alone', async () => {
    const connection = new FakeConnection()
    const { adapter, own, emitted, listen, link } = world(connection)
    connection.answer('settings:read', () => hostSettings({ scanRoots: [HUB] }))
    listen()
    const written = ask(adapter, 'settings:write', { windowBounds: { width: 900, height: 700, x: 1, y: 2 } })
    own.change({ theme: 'dark' })
    await link.connect()
    expect((await written).scanRoots).toEqual([HUB])
    await settle()
    const told = emitted.filter((e) => e.channel === 'settings:changed').map((e) => e.payload as AppSettings)
    expect(told.length).toBeGreaterThan(0)
    for (const settings of told) expect(settings.scanRoots).toEqual([HUB])
    expect(told.at(-1)).toMatchObject({ scanRoots: [HUB], theme: 'dark' })
  })

  it('a window that listens after the connection told its settings hears them at once, before any scan', async () => {
    const connection = new FakeConnection()
    const { adapter, link } = world(connection)
    on(adapter, 'scan:status', () => undefined)
    await link.connect()
    connection.snapshot('settings:changed', hostSettings({ scanRoots: [HUB] }))
    await settle()
    const heard: AppSettings[] = []
    on(adapter, 'settings:changed', (settings) => heard.push(settings))
    expect(heard.map((settings) => settings.scanRoots)).toEqual([[HUB]])
  })

  it('with the host out of reach, a change to Floti’s own settings is still told', async () => {
    const { own, emitted, listen } = world(null)
    listen()
    own.change({ theme: 'dark' })
    await settle()
    const told = emitted.filter((e) => e.channel === 'settings:changed').map((e) => e.payload as AppSettings)
    expect(told.at(-1)).toMatchObject({ theme: 'dark' })
  })

  it('a read asked before the first connect gets the host’s answer, not an empty one first', async () => {
    const { adapter } = world()
    const [cached, settings] = await Promise.all([ask(adapter, 'discovery:cached'), ask(adapter, 'settings:read')])
    expect(cached.map((p) => p.path)).toEqual([ALPHA])
    expect(cached[0]?.git).toEqual({ branch: 'main', detached: false, dirty: 1, ahead: 0, behind: 0 })
    expect(settings.scanRoots).toEqual([])
  })

  it('says why when the host cannot be reached, never no projects, and the first scan says it where scans show', async () => {
    const { adapter, emitted, listen } = world(null, ownBridge({ theme: 'dark' }))
    await expect(ask(adapter, 'discovery:cached')).rejects.toThrow(UNREACHABLE)
    await expect(ask(adapter, 'roots:suggest')).rejects.toThrow(UNREACHABLE)
    await expect(ask(adapter, 'template:list')).rejects.toThrow(UNREACHABLE)
    // The window still paints with the desktop's own settings.
    await expect(ask(adapter, 'settings:read')).resolves.toMatchObject({ theme: 'dark' })
    listen()
    await settle()
    expect(emitted).toContainEqual({ channel: 'scan:status', payload: { running: false, error: UNREACHABLE } })
  })

  it('starts the first scan once, and hands the host’s discovery, scan state and git to the window', async () => {
    const { connection, emitted, listen, link } = world()
    listen()
    listen()
    await link.connect()
    await settle()
    expect(connection.payloads('discovery:scan')).toEqual([{ includeGit: true }])
    connection.push('discovery:updated', scanResult(3))
    connection.push('scan:status', { running: false, error: null })
    connection.push('git:updated', { [ALPHA]: git(4) })
    await settle()
    expect(emitted).toContainEqual({ channel: 'scan:status', payload: { running: false } })
    expect(emitted).toContainEqual({
      channel: 'git:updated',
      payload: { [ALPHA]: { branch: 'main', detached: false, dirty: 4, ahead: 0, behind: 0 } }
    })
    const discovered = emitted.filter((e) => e.channel === 'discovery:updated')
    expect(discovered).toHaveLength(2)
  })

  it('tells the window a connection’s settings before its first scan, so a scan root is never offered empty', async () => {
    const { connection, emitted, listen, link } = world()
    listen()
    await link.connect()
    connection.snapshot('settings:changed', hostSettings({ scanRoots: [ALPHA] }))
    connection.snapshot('discovery:updated', scanResult(1) as never)
    await settle()
    const order = emitted.map((e) => e.channel).filter((c) => c === 'settings:changed' || c === 'discovery:updated')
    expect(order.slice(0, 2)).toEqual(['settings:changed', 'discovery:updated'])
    expect(emitted.find((e) => e.channel === 'settings:changed')?.payload).toMatchObject({ scanRoots: [ALPHA] })
  })

  it('reads git again when the window comes to the front, once it has the host’s scan', async () => {
    const { connection, listen, link, focus } = world()
    focus()
    expect(connection.payloads('git:refresh')).toEqual([])
    listen()
    await link.connect()
    connection.push('discovery:updated', scanResult(3))
    await settle()
    focus()
    await settle()
    expect(connection.payloads('git:refresh')).toEqual([null])
  })

  it('locates claude with the window’s picker, and shows it once the host checked and kept it, with no copy on this device', async () => {
    const { adapter, own, connection, emitted, listen } = world()
    let kept: string | null = null
    connection
      .answer('settings:read', () => hostSettings({ backendPaths: kept === null ? {} : { 'claude-code': kept } }))
      .answer('setup:locateBackend', ({ path }) => {
        kept = path
        return backend()
      })
    listen()
    own.picks.push(null)
    await ask(adapter, 'setup:locateClaude')
    expect(connection.payloads('setup:locateBackend')).toEqual([])

    own.picks.push(PICKED)
    const status = await ask(adapter, 'setup:locateClaude')
    expect(connection.payloads('setup:locateBackend')).toEqual([{ backend: 'claude-code', path: PICKED }])
    expect(status.path).toBe(PICKED)
    await settle()
    const told = emitted.filter((e) => e.channel === 'settings:changed').map((e) => e.payload as AppSettings)
    expect(told.at(-1)?.claudePath).toBe(PICKED)
    expect(own.writes).toEqual([])

    connection.answer('setup:locateBackend', () => backend({ installed: false, error: 'not a program' }))
    own.picks.push('/tools dir/notes.txt')
    await ask(adapter, 'setup:locateClaude')
    expect(own.writes).toEqual([])
  })

  it('writes every setting a host keeps for its sessions to that host, keeping its other executables, and none to this device', async () => {
    const { adapter, own, connection } = world()
    let current = hostSettings({ backendPaths: { pi: PI } })
    connection
      .answer('settings:read', () => current)
      .answer('settings:write', (patch) => (current = { ...current, ...patch }))
    const next = await ask(adapter, 'settings:write', {
      sessionAgents: false,
      agentDepth: 1,
      terminalShell: '/bin/zsh',
      claudePath: PICKED,
      browserMcpLocalOnly: true
    })
    expect(connection.payloads('settings:write')).toEqual([
      { sessionAgents: false, agentDepth: 1, terminalShell: '/bin/zsh', backendPaths: { 'claude-code': PICKED, pi: PI } }
    ])
    // What limits this device's own browser pane is this device's.
    expect(own.writes).toEqual([{ browserMcpLocalOnly: true }])
    expect(next).toMatchObject({
      sessionAgents: false,
      agentDepth: 1,
      terminalShell: '/bin/zsh',
      claudePath: PICKED,
      browserMcpLocalOnly: true
    })
  })

  it('hears a host setting another window changed, and shows the host’s value over whatever this device once kept', async () => {
    const { adapter, connection, emitted, listen, link } = world(new FakeConnection(), ownBridge({ sessionAgents: true }))
    listen()
    await link.connect()
    await ask(adapter, 'settings:read')
    connection.push('settings:changed', hostSettings({ sessionAgents: false, agentChildrenPerParent: 1 }))
    await settle()
    const told = emitted.filter((e) => e.channel === 'settings:changed').map((e) => e.payload as AppSettings)
    expect(told.at(-1)).toMatchObject({ sessionAgents: false, agentChildrenPerParent: 1 })
    // The window carries nothing to a host: that is main's, once, for this PC's.
    expect(connection.payloads('settings:write')).toEqual([])
  })

  it('carries nothing to a host on another computer: it keeps what it holds, and a change to it goes over its own connection', async () => {
    const connection = new FakeConnection()
    let current = hostSettings()
    connection
      .answer('settings:read', () => current)
      .answer('settings:write', (patch) => (current = { ...current, ...patch }))
    const own = ownBridge({ sessionAgents: false, agentDepth: 1 })
    const link = fakeReach(connection, UNREACHABLE)
    const adapter = createWorkspaceAdapter({ link, own: own.bridge, remote: { name: () => 'build-box' } })
    const read = await ask(adapter, 'settings:read')
    expect(connection.payloads('settings:write')).toEqual([])
    expect(read).toMatchObject({ sessionAgents: true, agentDepth: 2 })

    await ask(adapter, 'settings:write', { sessionAgents: false })
    expect(connection.payloads('settings:write')).toEqual([{ sessionAgents: false }])
    expect(own.writes).toEqual([])
    await expect(ask(adapter, 'setup:locateClaude')).rejects.toThrow(/cannot choose the claude build-box runs/)
  })

  it('finishes setup and accepts a scan root in the host', async () => {
    const { adapter, host } = world()
    await expect(ask(adapter, 'setup:complete')).resolves.toMatchObject({ firstRunCompletedAt: '2026-10-30T11:00:00.000Z' })
    await expect(ask(adapter, 'roots:accept', { path: HUB })).resolves.toEqual([HUB])
    expect(host().scanRoots).toEqual([HUB])
  })

  it('answers app:info with Floti’s own build and the host’s log file', async () => {
    await expect(ask(world().adapter, 'app:info')).resolves.toEqual({
      ...BUILD,
      hostLogFile: HOST_INFO.logFile
    })
    await expect(ask(world(null).adapter, 'app:info')).resolves.toEqual({
      ...BUILD,
      hostLogFile: null
    })
  })

  it('answers with Floti’s own settings and build, and saves its own half, while this PC’s host cannot be used and every host read waits', async () => {
    const own = ownBridge({ theme: 'dark' })
    // This PC's link: every read waits for the host, but one with its own answer is told why at once.
    const adapter = createWorkspaceAdapter({
      link: {
        connection: () => new Promise(() => undefined),
        connectionIfUsable: () => Promise.reject(new Error(UNREACHABLE)),
        onConnected: () => undefined
      },
      own: own.bridge
    })
    await expect(ask(adapter, 'settings:read')).resolves.toMatchObject({ theme: 'dark' })
    await expect(ask(adapter, 'app:info')).resolves.toEqual({ ...BUILD, hostLogFile: null })
    await expect(ask(adapter, 'settings:write', { theme: 'light' })).resolves.toMatchObject({ theme: 'light' })
    let scanned = false
    void ask(adapter, 'discovery:cached').then(() => (scanned = true))
    await settle()
    expect(scanned).toBe(false)
  })
})

describe('actions while this PC’s host cannot be used', () => {
  function unusableWorld() {
    const connection = new FakeConnection()
      .answer('settings:read', () => hostSettings())
      .answer('settings:write', (patch) => hostSettings(patch))
      .answer('template:create', () => ({ ok: true }) as never)
      .answer('setup:status', () => ({ backends: { 'claude-code': backend() } }))
      .answer('setup:locateBackend', () => backend())
    const own = ownBridge({ theme: 'dark' })
    const link = unusableReach(connection, HOST_UNUSABLE_FOR_ACTIONS)
    const adapter = createWorkspaceAdapter({ link, own: own.bridge })
    return { connection, own, link, adapter }
  }

  it('writing the host’s half of the settings fails at once with the short sentence, and is written once the host can be used', async () => {
    const w = unusableWorld()
    await expect(ask(w.adapter, 'settings:write', { scanRoots: [HUB] })).rejects.toThrow(HOST_UNUSABLE_FOR_ACTIONS)
    expect(w.connection.payloads('settings:write')).toEqual([])
    // Floti's own half needs no host.
    await expect(ask(w.adapter, 'settings:write', { theme: 'light' })).resolves.toMatchObject({ theme: 'light' })
    w.link.makeUsable()
    await expect(ask(w.adapter, 'settings:write', { scanRoots: [HUB] })).resolves.toMatchObject({ scanRoots: [HUB] })
  })

  it('a template action fails at once with the short sentence, and reaches the host once it can be used', async () => {
    const w = unusableWorld()
    await expect(ask(w.adapter, 'template:create', { name: 'starter' })).rejects.toThrow(HOST_UNUSABLE_FOR_ACTIONS)
    expect(w.connection.payloads('template:create')).toEqual([])
    w.link.makeUsable()
    await ask(w.adapter, 'template:create', { name: 'starter' })
    expect(w.connection.payloads('template:create')).toHaveLength(1)
  })

  it('Locate claude fails at once with the short sentence and opens no picker, and opens it once the host can be used', async () => {
    const w = unusableWorld()
    w.own.picks.push(PICKED)
    await expect(ask(w.adapter, 'setup:locateClaude')).rejects.toThrow(HOST_UNUSABLE_FOR_ACTIONS)
    // The picker was never opened: what it would have answered is still waiting.
    expect(w.own.picks).toEqual([PICKED])
    w.link.makeUsable()
    await expect(ask(w.adapter, 'setup:locateClaude')).resolves.toMatchObject({ path: PICKED })
    expect(w.own.picks).toEqual([])
  })
})
