import { describe, expect, it } from 'vitest'
import { FakeConnection, fakeReach } from '../../../../../test/fake-connection'
import { CLAUDE_CODE_BACKEND } from '../../../../shared/host/backend'
import type * as P from '../../../../shared/host/protocol'
import { sessionFromHost } from '../../../../shared/host/records'
import type {
  EventChannel,
  EventPayload,
  DesktopBridge,
  PluginCallRequest,
  PluginInfo,
  RequestChannel,
  RequestPayload,
  RequestResult,
  SendChannel,
  SendPayload
} from '../../../../shared/ipc'
import { createPluginsAdapter } from './plugins'
import type { HostAdapter } from './router'

const UNREACHABLE = 'No local host answered on its socket.'
const DIR = '/plugins/my sample'

const hostPlugin = (patch: Partial<P.PluginInfo> = {}): P.PluginInfo => ({
  path: DIR,
  id: 'sample',
  name: 'Sample',
  version: '1.0.0',
  description: null,
  enabled: true,
  error: null,
  warnings: [],
  revision: 1,
  icon: null,
  rail: { title: 'Sample', panel: 'main' },
  panels: { main: { title: 'Sample', url: 'floti-plugin://sample/dist/panel.html', actions: [] } },
  tabs: { item: { title: 'Item', url: 'floti-plugin://sample/dist/item.html' } },
  pageStrip: false,
  background: { url: 'floti-plugin://sample/dist/background.js', state: 'running', error: null },
  commands: [],
  settings: [{ type: 'text', key: 'greeting', label: 'Greeting' }],
  settingValues: { greeting: 'hello' },
  network: [],
  secrets: [],
  exec: [],
  service: null,
  runsPrograms: false,
  agent: null,
  startsSessions: true,
  seesSessions: false,
  status: null,
  badge: null,
  ...patch
})

const ROW: P.SessionRecord = {
  id: 31,
  name: 'triage',
  label: null,
  cwd: '/work dir/app',
  branch: null,
  projectPath: null,
  profileId: null,
  argv: ['claude', '-n', 'triage', 'Look at the failing build'],
  conversationId: null,
  backend: CLAUDE_CODE_BACKEND,
  status: 'running',
  startedAt: '2026-10-30T09:00:00.000Z',
  endedAt: null,
  durationMs: null,
  exitCode: null
}

/** Lets every promise already settled run its callbacks. */
async function settle(): Promise<void> {
  for (let i = 0; i < 20; i++) await Promise.resolve()
}

/** The preload's bridge as main would answer it: what the page sent, and events main pushes. */
function ownBridge(answers: Record<string, (payload: unknown) => unknown> = {}) {
  const sent: Array<{ channel: string; payload: unknown }> = []
  const invoked: Array<{ channel: string; payload: unknown }> = []
  const listeners = new Map<string, Set<(payload: unknown) => void>>()
  const own = {
    invoke: (channel: string, payload: unknown) => {
      invoked.push({ channel, payload })
      const answer = answers[channel]
      return answer === undefined ? Promise.reject(new Error(`no ${channel}`)) : Promise.resolve(answer(payload))
    },
    send: (channel: string, payload: unknown) => {
      sent.push({ channel, payload })
    },
    on: (channel: string, listener: (payload: unknown) => void) => {
      const set = listeners.get(channel) ?? new Set()
      set.add(listener)
      listeners.set(channel, set)
      return () => set.delete(listener)
    }
  } as unknown as DesktopBridge
  const push = <K extends EventChannel>(channel: K, payload: EventPayload<K>): void => {
    for (const listener of listeners.get(channel) ?? []) listener(payload)
  }
  return { own, sent, invoked, push }
}

function routeOf<T>(adapter: HostAdapter, kind: 'requests' | 'sends' | 'events', channel: string): T {
  const route = (adapter[kind] as Record<string, unknown> | undefined)?.[channel]
  if (route === undefined) throw new Error(`no route for ${channel}`)
  return route as T
}

const ask = <K extends RequestChannel>(adapter: HostAdapter, channel: K, payload?: RequestPayload<K>) =>
  routeOf<(payload: unknown) => Promise<RequestResult<K>>>(adapter, 'requests', channel)(payload)

const send = <K extends SendChannel>(adapter: HostAdapter, channel: K, payload: SendPayload<K>): void => {
  routeOf<(payload: unknown) => void>(adapter, 'sends', channel)(payload)
}

function hear<K extends EventChannel>(adapter: HostAdapter, channel: K): Array<EventPayload<K>> {
  const heard: Array<EventPayload<K>> = []
  routeOf<(listener: (payload: EventPayload<K>) => void) => () => void>(adapter, 'events', channel)((payload) =>
    heard.push(payload)
  )
  return heard
}

const pageCall = (method: string, args: unknown[] = [], patch: Partial<PluginCallRequest> = {}): PluginCallRequest => ({
  plugin: 'sample',
  surface: 'panel',
  method,
  args,
  callId: '1.1.7',
  ...patch
})

interface Options {
  connection?: FakeConnection | null
  hosted?: P.PluginInfo[]
  chosen?: string | null
}

/** The window's adapter on a fake connection whose host keeps a registry. */
function windowWorld(options: Options = {}) {
  const connection = options.connection === undefined ? new FakeConnection() : options.connection
  const hosted = [...(options.hosted ?? [hostPlugin()])]
  connection
    ?.answer('plugins:list', () => [...hosted])
    .answer('plugins:add', ({ path }) => {
      hosted.push(hostPlugin({ path, id: 'added', name: 'Added' }))
      return { path, error: null, plugins: [...hosted] }
    })
  const bridge = ownBridge({
    'path:chooseDirectory': () => ({ path: options.chosen ?? null })
  })
  const adopted: number[] = []
  const adapter = createPluginsAdapter({
    link: fakeReach(connection, UNREACHABLE),
    own: bridge.own,
    adopt: (row) => {
      adopted.push(row.id)
      return sessionFromHost(row)
    }
  })
  return { connection: connection ?? new FakeConnection(), adapter, bridge, hosted, adopted }
}

describe('the window’s plugins over its own connection', () => {
  it('waits for the first connect, then lists the host’s plugins, never an empty list in their place', async () => {
    const { adapter, connection } = windowWorld()
    const listed = await ask(adapter, 'plugins:list')
    expect(listed.map((plugin) => [plugin.id, plugin.settingValues])).toEqual([['sample', { greeting: 'hello' }]])
    expect([...connection.subscriptions.keys()].sort()).toEqual([
      'plugins:changed',
      'plugins:deliver',
      'plugins:ui',
      'secrets:changed'
    ])
  })

  it('rejects with the link’s sentence when the host cannot be reached, and a page’s call settles in the page', async () => {
    const { adapter } = windowWorld({ connection: null })
    await expect(ask(adapter, 'plugins:list')).rejects.toThrow(UNREACHABLE)
    await expect(ask(adapter, 'secrets:list')).rejects.toThrow(UNREACHABLE)
    await expect(ask(adapter, 'plugins:metrics')).rejects.toThrow(UNREACHABLE)
    for (const method of ['fetch', 'exec', 'settings.get', 'secrets.request', 'sessions.start', 'tabs.open']) {
      expect(await ask(adapter, 'plugins:call', pageCall(method))).toEqual({ ok: false, code: 'unavailable', message: UNREACHABLE })
    }
  })

  it('sends Settings’ actions and secrets to the host, adds a folder the window’s own picker chose, and names the host’s log lines as Floti’s', async () => {
    const { adapter, connection, bridge } = windowWorld({ chosen: '/plugins/added one' })
    connection.answer('plugins:setEnabled', () => [hostPlugin({ enabled: false })])
    connection.answer('plugins:log', () => [
      { at: '2026-10-30T09:00:00.000Z', stream: 'host', text: 'service started' },
      { at: '2026-10-30T09:00:01.000Z', stream: 'out', text: 'listening' }
    ])
    connection.answer('secrets:save', (input) => ({
      available: true,
      secrets: [{ key: input.key, hosts: input.hosts, plugins: input.plugins, updatedAt: '2026-10-30T09:00:00.000Z' }]
    }))
    const changed = hear(adapter, 'plugins:changed')

    const added = await ask(adapter, 'plugins:add')
    expect(bridge.invoked.filter((each) => each.channel === 'path:chooseDirectory')).toEqual([
      { channel: 'path:chooseDirectory', payload: { title: 'Add a plugin folder' } }
    ])
    expect(added.path).toBe('/plugins/added one')
    expect(added.plugins.map((plugin) => plugin.id)).toEqual(['sample', 'added'])
    expect(changed.at(-1)?.map((plugin) => plugin.id)).toEqual(['sample', 'added'])

    expect((await ask(adapter, 'plugins:setEnabled', { path: DIR, enabled: false }))[0]?.enabled).toBe(false)
    expect(connection.payloads('plugins:setEnabled')).toEqual([{ path: DIR, enabled: false }])
    expect((await ask(adapter, 'plugins:log', { path: DIR })).map((line) => line.stream)).toEqual(['floti', 'out'])
    const input = { key: 'TOKEN', value: 'x', hosts: [], plugins: ['sample'] }
    expect((await ask(adapter, 'secrets:save', input)).secrets).toHaveLength(1)
    expect(connection.payloads('secrets:save')).toEqual([input])
  })

  it('answers a cancelled pick with the host’s list and no folder', async () => {
    const { adapter, connection } = windowWorld({ chosen: null })
    const added = await ask(adapter, 'plugins:add')
    expect(added).toMatchObject({ path: null, error: null })
    expect(added.plugins.map((plugin) => plugin.id)).toEqual(['sample'])
    expect(connection.payloads('plugins:add')).toEqual([])
  })

  it('sends a page’s fetch, exec, settings, secrets, sessions and channel post to the host under the relay’s own id', async () => {
    const { adapter, connection } = windowWorld()
    connection.answer('plugins:call', ({ method }) => ({ ok: true, value: method === 'settings.get' ? { greeting: 'hi' } : null }))
    const methods = ['fetch', 'exec', 'settings.get', 'secrets.request', 'sessions.list', 'sessions.start', 'channel.post']
    for (const method of methods) await ask(adapter, 'plugins:call', pageCall(method, ['x']))
    const sent = connection.payloads('plugins:call') as P.PluginCallRequest[]
    expect(sent.map((each) => each.method)).toEqual(methods)
    expect(sent[0]).toEqual({ plugin: 'sample', surface: 'panel', method: 'fetch', args: ['x'], callId: '1.1.7' })
    connection.answer('plugins:call', () => ({ ok: false, code: 'not-declared', message: 'not listed' }))
    expect(await ask(adapter, 'plugins:call', pageCall('exec', ['rm']))).toEqual({ ok: false, code: 'not-declared', message: 'not listed' })

    send(adapter, 'plugins:cancel', { callId: '1.1.7' })
    await settle()
    expect(connection.payloads('plugins:cancel')).toEqual([{ callId: '1.1.7' }])
  })

  it('opens one of the plugin’s tabs, and a link, in the window without asking the host, with main’s checks', async () => {
    const { adapter, connection } = windowWorld()
    const ui = hear(adapter, 'plugins:ui')
    await ask(adapter, 'plugins:list')
    const before = connection.sent.length

    expect(await ask(adapter, 'plugins:call', pageCall('tabs.open', ['item', { id: 7 }, { title: 'Seven' }]))).toEqual({
      ok: true,
      value: undefined
    })
    expect(await ask(adapter, 'plugins:call', pageCall('tabs.open', ['nope']))).toMatchObject({ ok: false, code: 'not-declared' })
    expect(await ask(adapter, 'plugins:call', pageCall('open', ['https://example.com/a']))).toEqual({ ok: true, value: undefined })
    expect(await ask(adapter, 'plugins:call', pageCall('open', ['http://example.com/']))).toMatchObject({ ok: false, code: 'invalid' })
    expect(await ask(adapter, 'plugins:call', pageCall('open', ['https://example.com/c']))).toMatchObject({ ok: false, code: 'busy' })

    expect(ui).toEqual([
      { kind: 'openTab', plugin: 'sample', tab: 'item', params: { id: 7 }, title: 'Seven', preview: false },
      { kind: 'link', plugin: 'sample', url: 'https://example.com/a' }
    ])
    expect(connection.sent.slice(before)).toEqual([])
  })

  it('draws what the host asks on this connection and answers on it: a secret dialog, and a session after the user’s yes', async () => {
    const { adapter, connection, adopted } = windowWorld()
    const ui = hear(adapter, 'plugins:ui')
    await ask(adapter, 'plugins:list')
    connection.answer('plugins:session', ({ start }) => (start ? ROW : null))

    connection.push('plugins:ui', { kind: 'secret', requestId: 's1', plugin: 'sample', key: 'token', hosts: ['https://api.example.com'] })
    connection.push('plugins:ui', { kind: 'session', requestId: 'a1', plugin: 'sample', cwd: '/work dir/app', prompt: 'Look', name: 'triage' })
    expect(ui.map((request) => request.kind)).toEqual(['secret', 'session'])

    send(adapter, 'plugins:answer', { requestId: 's1' })
    const started = await ask(adapter, 'plugins:session', { requestId: 'a1', start: true, cols: 120, rows: 30 })
    expect(await ask(adapter, 'plugins:session', { requestId: 'a2', start: false, cols: 0, rows: 0 })).toBeNull()
    await settle()

    expect(connection.payloads('plugins:answer')).toEqual([{ requestId: 's1' }])
    expect(connection.payloads('plugins:session')).toEqual([
      { requestId: 'a1', start: true, cols: 120, rows: 30 },
      { requestId: 'a2', start: false, cols: 0, rows: 0 }
    ])
    // Kept with the window's sessions, so App opens it as a tab.
    expect(started?.id).toBe(31)
    expect(adopted).toEqual([31])
  })

  it('takes every delivery but those for the background', async () => {
    const { adapter, connection } = windowWorld()
    const delivered = hear(adapter, 'plugins:deliver')
    await ask(adapter, 'plugins:list')
    connection.push('plugins:deliver', { event: 'settings', plugin: 'sample', data: { greeting: 'hey' }, to: 'all' })
    connection.push('plugins:deliver', { event: 'command', plugin: 'sample', data: { id: 'refresh' }, to: 'background' })
    connection.push('plugins:deliver', {
      event: 'channel',
      plugin: 'sample',
      data: { name: 'sample-items', message: { changed: 3 }, token: 'tab-1' },
      to: 'all'
    })
    expect(delivered).toEqual([
      { plugin: 'sample', event: 'settings', data: { greeting: 'hey' }, to: 'all' },
      { plugin: 'sample', event: 'channel', data: { name: 'sample-items', message: { changed: 3 }, token: 'tab-1' }, to: 'all' }
    ])
  })

  it('shows the background as the host lists it, and follows the host as it moves', async () => {
    const { adapter, connection } = windowWorld()
    const changed = hear(adapter, 'plugins:changed')
    const background = (list: PluginInfo[] | undefined) => list?.[0]?.background
    expect(background(await ask(adapter, 'plugins:list'))).toMatchObject({ state: 'running' })

    const crashed = { url: 'floti-plugin://sample/dist/background.js', state: 'crashed' as const, error: 'it threw' }
    connection.push('plugins:changed', [hostPlugin({ revision: 2, background: crashed })])
    expect(background(changed.at(-1))).toEqual(crashed)
  })
})
