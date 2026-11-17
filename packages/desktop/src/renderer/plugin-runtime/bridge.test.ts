import { afterEach, describe, expect, it, vi } from 'vitest'
import type { FlotiBridge, FlotiContext, FlotiError, FlotiTheme } from '@floti-dev/plugin-sdk'
import { BUILTIN_THEMES, PALETTE_HUES, pluginThemeOf, resolveTheme, type ThemeKind } from '@helm/core/types'
import type { PluginFetchRequest, PluginFetchResponse } from '../../shared/ipc'
import { installBridge } from './bridge'
import { CONNECT, HELLO, contextName, type FrameMessage } from './wire'
import { PATIENCE } from '../../../../../test/patience'

/**
 * `window.floti`, as a plugin page gets it: installed into a real jsdom frame
 * whose parent plays the desktop page framing it. The parent is reached as the
 * frame's own `parent`: jsdom hands a frame a window proxy that is not the
 * object vitest puts on `globalThis.window`.
 *
 * The port Floti hands over is a small fake (jsdom has no MessageChannel):
 * `desktopSide` is the relay's end, so what the page sent is `desktopSide.received`
 * and what Floti answers is `desktopSide.postMessage`.
 */

class FakePort {
  peer: FakePort | null = null
  onmessage: ((event: MessageEvent) => void) | null = null
  received: unknown[] = []
  postMessage(data: unknown): void {
    this.peer?.deliver(data)
  }
  deliver(data: unknown): void {
    this.received.push(data)
    this.onmessage?.(new MessageEvent('message', { data }))
  }
  start(): void {}
  close(): void {}
}

function channel(): { desktopSide: FakePort; pageSide: FakePort } {
  const desktopSide = new FakePort()
  const pageSide = new FakePort()
  desktopSide.peer = pageSide
  pageSide.peer = desktopSide
  return { desktopSide, pageSide }
}

const THEME: FlotiTheme = {
  kind: 'dark',
  tokens: { bg: '#101010', fg: '#eeeeee', accent: '#3366ff' } as FlotiTheme['tokens'],
  radius: 4,
  density: 'compact'
}

const LIGHT: FlotiTheme = { ...THEME, kind: 'light', tokens: { bg: '#ffffff' } as FlotiTheme['tokens'], radius: 2, density: 'comfortable' }

const PANEL: FlotiContext = { plugin: 'sample', surface: 'panel', name: 'main', params: {} }

interface Page {
  win: Window & { floti: FlotiBridge }
  floti: FlotiBridge
  /** What the page posted to its parent before it had a port. */
  hellos: unknown[]
}

afterEach(() => {
  vi.restoreAllMocks()
  document.body.replaceChildren()
})

/** A frame with the bridge installed, as the desktop serves a page: named with its context, booted with a theme. */
function page(context: FlotiContext | null = PANEL, boot: string | null = JSON.stringify({ theme: THEME })): Page {
  const frame = document.createElement('iframe')
  document.body.appendChild(frame)
  const win = frame.contentWindow as Window & { floti: FlotiBridge }
  if (context !== null) win.name = contextName(context)
  const parentPost = vi.spyOn(win.parent, 'postMessage').mockImplementation(() => undefined)
  const floti = installBridge(win, boot)
  const hellos = parentPost.mock.calls.map((call) => call[0])
  return { win, floti, hellos }
}

/** A `message` event made in the page's own realm, with the fields a browser sets. */
function message(win: Window, init: { data: unknown; source: unknown; ports?: unknown[] }): Event {
  const event = new (win as unknown as typeof globalThis).Event('message')
  Object.defineProperties(event, {
    data: { value: init.data },
    source: { value: init.source },
    ports: { value: init.ports ?? [] },
    origin: { value: 'http://localhost' }
  })
  return event
}

/** Floti answering HELLO: CONNECT, from the parent, with the page's port. */
function connect(win: Window, extra: Partial<{ context: FlotiContext; theme: FlotiTheme | null; visible: boolean; mac: boolean }> = {}): FakePort {
  const { desktopSide, pageSide } = channel()
  win.dispatchEvent(
    message(win, {
      data: { type: CONNECT, context: PANEL, theme: null, visible: false, ...extra },
      source: win.parent,
      ports: [pageSide]
    })
  )
  return desktopSide
}

const sent = (port: FakePort): FrameMessage[] => port.received as FrameMessage[]

/** The request the page's last `floti.fetch` sent. */
function lastFetch(port: FakePort): PluginFetchRequest {
  const call = sent(port)
    .filter((entry) => entry.t === 'call' && entry.method === 'fetch')
    .at(-1)
  if (call === undefined || call.t !== 'call') throw new Error('no fetch was sent')
  return call.args[0] as PluginFetchRequest
}

/** The last call the page made, answered. */
function answer(port: FakePort, result: { ok: true; value: unknown } | { ok: false; code: string; message: string }): void {
  const last = sent(port).filter((entry) => entry.t === 'call').at(-1)
  if (last === undefined || last.t !== 'call') throw new Error('no call to answer')
  port.postMessage({ t: 'result', id: last.id, ...result })
}

async function flush(): Promise<void> {
  for (let i = 0; i < 10; i += 1) await Promise.resolve()
}

describe('onHostComputer', () => {
  it('is true on a page whose boot says so, from the first script, before Floti has connected, and stays true', () => {
    const { win, floti } = page(PANEL, JSON.stringify({ theme: THEME, onHostComputer: true }))
    expect(floti.onHostComputer).toBe(true)
    connect(win, { visible: true, context: { ...PANEL, params: { id: 2 } } })
    expect(floti.onHostComputer).toBe(true)
  })

  it('is false on a page drawn from a host on another computer, before and after connecting', () => {
    const { win, floti } = page(PANEL, JSON.stringify({ theme: THEME, channels: 'host', onHostComputer: false }))
    expect(floti.onHostComputer).toBe(false)
    connect(win)
    expect(floti.onHostComputer).toBe(false)
  })

  it('reads as true from a boot that does not say, or no boot at all', () => {
    expect(page(PANEL, JSON.stringify({ theme: THEME })).floti.onHostComputer).toBe(true)
    expect(page(PANEL, null).floti.onHostComputer).toBe(true)
    expect(page(PANEL, 'not json').floti.onHostComputer).toBe(true)
  })

  it('cannot be changed by the page', () => {
    const { floti } = page(PANEL, JSON.stringify({ theme: THEME, onHostComputer: false }))
    expect(() => {
      ;(floti as { onHostComputer: boolean }).onHostComputer = true
    }).toThrow()
    expect(floti.onHostComputer).toBe(false)
  })
})

describe('installing', () => {
  it('reads its context from the frame name before anything is said, and says HELLO to its parent', () => {
    const { floti, hellos } = page({ plugin: 'sample', surface: 'tab', name: 'item', params: { id: '2' } })
    expect(floti.context).toEqual({ plugin: 'sample', surface: 'tab', name: 'item', params: { id: '2' } })
    expect(hellos).toEqual([{ type: HELLO }])
    expect(floti.apiVersion).toBe(2)
  })

  it('paints the boot theme onto <html> before the page has run', () => {
    const { win, floti } = page()
    const root = win.document.documentElement
    expect(root.style.getPropertyValue('--floti-bg')).toBe('#101010')
    expect(root.style.getPropertyValue('--floti-accent')).toBe('#3366ff')
    expect(root.style.getPropertyValue('--floti-radius')).toBe('4px')
    expect(root.dataset['density']).toBe('compact')
    expect(root.dataset['theme']).toBe('dark')
    expect(root.classList.contains('dark')).toBe(true)
    expect(floti.theme).toEqual(THEME)
  })

  it('is on window as floti, frozen, and cannot be replaced', () => {
    const { win, floti } = page()
    expect(win.floti).toBe(floti)
    expect(Object.isFrozen(win.floti)).toBe(true)
    const descriptor = Object.getOwnPropertyDescriptor(win, 'floti')
    expect(descriptor).toMatchObject({ writable: false, configurable: false })
    // Modules are strict, so assigning to it throws rather than failing quietly.
    expect(() => {
      ;(win as unknown as Record<string, unknown>)['floti'] = {}
    }).toThrow(TypeError)
  })
})

describe('the port', () => {
  it('holds calls made before CONNECT, then sends them in order', () => {
    const { win, floti } = page()
    void floti.settings.get()
    void floti.badge.set(3)
    const port = connect(win)
    expect(sent(port)).toEqual([
      { t: 'call', id: 1, method: 'settings.get', args: [] },
      { t: 'call', id: 2, method: 'badge.set', args: [3] }
    ])
  })

  it('is taken only from the parent, only with a port, and only once', () => {
    const { win, floti } = page()
    // Not from the parent.
    const stranger = channel()
    win.dispatchEvent(message(win, { data: { type: CONNECT, context: PANEL, theme: null, visible: false }, source: win, ports: [stranger.pageSide] }))
    // No port.
    win.dispatchEvent(message(win, { data: { type: CONNECT, context: PANEL, theme: null, visible: false }, source: win.parent }))
    void floti.settings.get()
    expect(stranger.desktopSide.received).toEqual([])
    const first = connect(win)
    const second = connect(win)
    void floti.status.set(null)
    expect(sent(first).map((entry) => entry.t === 'call' && entry.method)).toEqual(['settings.get', 'status.set'])
    expect(sent(second)).toEqual([])
  })

  it('takes the context, theme and visibility CONNECT gives, and a null theme keeps the boot one', () => {
    const { win, floti } = page()
    const visibility = vi.fn()
    floti.on('visibility', visibility)
    connect(win, { context: { ...PANEL, params: { from: 'connect' } }, theme: null, visible: true })
    expect(floti.context.params).toEqual({ from: 'connect' })
    expect(floti.theme).toEqual(THEME)
    expect(floti.visible).toBe(true)
    expect(visibility).toHaveBeenCalledWith(true)
  })
})

describe('calls', () => {
  it('resolve with what Floti answered', async () => {
    const { win, floti } = page()
    const port = connect(win)
    const settings = floti.settings.get()
    answer(port, { ok: true, value: { limit: 20 } })
    await expect(settings).resolves.toEqual({ limit: 20 })
  })

  it('reject with a FlotiError carrying the code', async () => {
    const { win, floti } = page()
    const port = connect(win)
    const ran = floti.exec('nope', ['a'])
    expect(sent(port).at(-1)).toEqual({ t: 'call', id: 1, method: 'exec', args: ['nope', ['a'], {}] })
    answer(port, { ok: false, code: 'not-declared', message: '"nope" is not one of the programs' })
    const error = (await ran.catch((failure: unknown) => failure)) as FlotiError
    expect(error.name).toBe('FlotiError')
    expect(error.code).toBe('not-declared')
    expect(error.message).toBe('"nope" is not one of the programs')
  })

  it('reject as an AbortError when Floti says the call was aborted', async () => {
    const { win, floti } = page()
    const port = connect(win)
    const opened = floti.tabs.open('item', { id: '2' }, { title: 'Two' })
    expect(sent(port).at(-1)).toEqual({ t: 'call', id: 1, method: 'tabs.open', args: ['item', { id: '2' }, { title: 'Two' }] })
    answer(port, { ok: false, code: 'aborted', message: 'stopped' })
    await expect(opened).rejects.toMatchObject({ name: 'AbortError' })
  })

  it('send a cancel when the page aborts, reject at once, and ignore a late answer', async () => {
    const { win, floti } = page()
    const port = connect(win)
    const controller = new AbortController()
    const fetched = floti.fetch('https://api.example.com/items', { signal: controller.signal })
    await flush()
    const call = sent(port).at(-1)
    expect(call).toMatchObject({ t: 'call', method: 'fetch' })
    controller.abort()
    await expect(fetched).rejects.toMatchObject({ name: 'AbortError' })
    expect(sent(port).at(-1)).toEqual({ t: 'cancel', id: (call as { id: number }).id })
    port.postMessage({ t: 'result', id: (call as { id: number }).id, ok: true, value: {} })
  })

  it('reject before sending anything when the signal is already aborted', async () => {
    const { win, floti } = page()
    const port = connect(win)
    const controller = new AbortController()
    controller.abort()
    await expect(floti.fetch('https://api.example.com/', { signal: controller.signal })).rejects.toMatchObject({
      name: 'AbortError'
    })
    expect(sent(port)).toEqual([])
  })
})

describe('fetch', () => {
  it('sends the request taken apart: URL, method, headers and the body as bytes', async () => {
    const { win, floti } = page()
    const port = connect(win)
    void floti.fetch('https://api.example.com/items', {
      method: 'POST',
      headers: { 'content-type': 'application/json', authorization: 'Bearer {{sample-token}}' },
      body: JSON.stringify({ title: 'x' })
    })
    await flush()
    const request = lastFetch(port)
    expect(request.url).toBe('https://api.example.com/items')
    expect(request.method).toBe('POST')
    expect(Object.fromEntries(request.headers)).toMatchObject({
      'content-type': 'application/json',
      authorization: 'Bearer {{sample-token}}'
    })
    expect(new TextDecoder().decode(request.body!)).toBe('{"title":"x"}')
    expect(request.redirect).toBe('follow')
  })

  it('sends no body for a GET', async () => {
    const { win, floti } = page()
    const port = connect(win)
    void floti.fetch('https://api.example.com/items')
    await flush()
    const request = lastFetch(port)
    expect(request.method).toBe('GET')
    expect(request.body).toBeNull()
  })

  it('gives back a Response with the status, headers, body, final URL and whether it was redirected', async () => {
    const { win, floti } = page()
    const port = connect(win)
    const fetched = floti.fetch('https://api.example.com/items')
    await flush()
    const response: PluginFetchResponse = {
      status: 201,
      statusText: 'Created',
      headers: [
        ['content-type', 'application/json'],
        ['bad header', 'dropped, not thrown']
      ],
      body: new TextEncoder().encode('{"id":"4"}'),
      url: 'https://api.example.com/items/4',
      redirected: true
    }
    answer(port, { ok: true, value: response })
    const got = await fetched
    expect(got.status).toBe(201)
    expect(got.statusText).toBe('Created')
    expect(got.headers.get('content-type')).toBe('application/json')
    expect(got.url).toBe('https://api.example.com/items/4')
    expect(got.redirected).toBe(true)
    await expect(got.json()).resolves.toEqual({ id: '4' })
  })

  it('gives a 204 no body, as fetch would', async () => {
    const { win, floti } = page()
    const port = connect(win)
    const fetched = floti.fetch('https://api.example.com/items/1/read', { method: 'POST' })
    await flush()
    answer(port, {
      ok: true,
      value: { status: 204, statusText: '', headers: [], body: new Uint8Array(), url: 'https://api.example.com/items/1/read', redirected: false }
    })
    const got = await fetched
    expect(got.status).toBe(204)
    expect(got.body).toBeNull()
  })
})

describe('events', () => {
  it('reach every listener for them, and an unsubscribed one no longer', () => {
    const { win, floti } = page()
    const port = connect(win)
    const first = vi.fn()
    const second = vi.fn()
    const off = floti.on('command', first)
    floti.on('command', second)
    port.postMessage({ t: 'event', name: 'command', data: { id: 'refresh' } })
    off()
    port.postMessage({ t: 'event', name: 'command', data: { id: 'again' } })
    expect(first.mock.calls).toEqual([[{ id: 'refresh' }]])
    expect(second.mock.calls).toEqual([[{ id: 'refresh' }], [{ id: 'again' }]])
  })

  it('keep going past a listener that throws, and the throw is reported rather than swallowed', async () => {
    const { win, floti } = page()
    const port = connect(win)
    const reported: unknown[] = []
    win.addEventListener('error', (event) => {
      event.preventDefault()
      reported.push((event as ErrorEvent).error)
    })
    const after = vi.fn()
    floti.on('action', () => {
      throw new Error('plugin bug')
    })
    floti.on('action', after)
    port.postMessage({ t: 'event', name: 'action', data: { id: 'add' } })
    expect(after).toHaveBeenCalledWith({ id: 'add' })
    await vi.waitFor(() => expect(reported).toHaveLength(1), { timeout: PATIENCE })
    expect((reported[0] as Error).message).toBe('plugin bug')
  })

  it.each([
    ['settings', { mode: 'all' }],
    ['secrets', { token: 'ready' }],
    ['command', { id: 'refresh' }],
    ['action', { id: 'add' }],
    ['sessions', [{ id: 'a1b2', name: 'FLOTI-3', state: 'running' }]]
  ] as const)('pass Floti’s %s event to the page as it came', (name, data) => {
    const { win, floti } = page()
    const port = connect(win)
    const heard = vi.fn()
    floti.on(name, heard)
    port.postMessage({ t: 'event', name, data })
    expect(heard).toHaveBeenCalledWith(data)
  })

  it('apply a new theme to the page before telling it', () => {
    const { win, floti } = page()
    const port = connect(win)
    const seen: string[] = []
    floti.on('theme', (theme) => seen.push(`${theme.kind}:${win.document.documentElement.dataset['theme'] ?? ''}`))
    port.postMessage({ t: 'event', name: 'theme', data: LIGHT })
    expect(seen).toEqual(['light:light'])
    const root = win.document.documentElement
    expect(root.classList.contains('dark')).toBe(false)
    expect(root.style.getPropertyValue('--floti-bg')).toBe('#ffffff')
    expect(root.style.getPropertyValue('--floti-radius')).toBe('2px')
    expect(floti.theme).toEqual(LIGHT)
  })

  it('carry the palette onto <html> as --floti-hue-<name> in the theme on screen, and rewrite it in place on a change', () => {
    const themeFor = (kind: ThemeKind, id: string): FlotiTheme =>
      pluginThemeOf(
        resolveTheme({ preference: kind, systemDark: kind === 'dark', themeDark: id, themeLight: id, accentColor: null }, BUILTIN_THEMES),
        { cornerRadius: 3, density: 'comfortable' }
      )
    const nocturne = themeFor('dark', 'nocturne')
    const { win, floti } = page(PANEL, JSON.stringify({ theme: nocturne }))
    const root = win.document.documentElement
    const painted = (): string[] => PALETTE_HUES.map((hue) => root.style.getPropertyValue(`--floti-hue-${hue}`))
    const listed = (theme: FlotiTheme): string[] => PALETTE_HUES.map((hue) => theme.tokens[`hue-${hue}`])
    expect(painted()).toEqual(listed(nocturne))
    expect(listed(floti.theme)).toEqual(listed(nocturne))

    const port = connect(win)
    for (const next of [themeFor('dark', 'graphite'), themeFor('light', 'daylight')]) {
      port.postMessage({ t: 'event', name: 'theme', data: next })
      expect(painted()).toEqual(listed(next))
      expect(listed(floti.theme)).toEqual(listed(next))
    }
    expect(new Set(painted()).size).toBe(PALETTE_HUES.length)
  })

  it('say visibility only when it changes', () => {
    const { win, floti } = page()
    const port = connect(win)
    const visibility = vi.fn()
    floti.on('visibility', visibility)
    port.postMessage({ t: 'event', name: 'visibility', data: false })
    port.postMessage({ t: 'event', name: 'visibility', data: true })
    port.postMessage({ t: 'event', name: 'visibility', data: true })
    expect(visibility.mock.calls).toEqual([[true]])
    expect(floti.visible).toBe(true)
  })
})

describe('keys', () => {
  /** A keydown in the page, with the page's own handlers given a chance first. */
  async function press(win: Window, init: KeyboardEventInit): Promise<void> {
    const KeyboardEventInPage = (win as unknown as typeof globalThis).KeyboardEvent
    win.document.body.dispatchEvent(new KeyboardEventInPage('keydown', { bubbles: true, cancelable: true, ...init }))
    // The bridge looks again after the page's handlers, on a timer of the
    // page's; one set after it runs after it.
    await new Promise((resolve) => win.setTimeout(resolve, 0))
  }

  const keys = (port: FakePort): unknown[] => sent(port).filter((entry) => entry.t === 'key')

  it('forwards a Floti shortcut the page did not take', async () => {
    const { win } = page()
    const port = connect(win)
    await press(win, { key: 'P', code: 'KeyP', ctrlKey: true, shiftKey: true })
    expect(keys(port)).toEqual([
      {
        t: 'key',
        key: { key: 'P', code: 'KeyP', ctrlKey: true, shiftKey: true, altKey: false, metaKey: false, repeat: false }
      }
    ])
  })

  it("keeps the page's own: editing keys, AltGr, a bare modifier, no modifier at all, and anything it prevented", async () => {
    const { win } = page()
    const port = connect(win)
    for (const key of ['c', 'V', 'x', 'z', 'a', 'y']) await press(win, { key, ctrlKey: true })
    await press(win, { key: '@', ctrlKey: true, altKey: true })
    await press(win, { key: 'q', modifierAltGraph: true, ctrlKey: true } as KeyboardEventInit)
    await press(win, { key: 'Control', ctrlKey: true })
    await press(win, { key: 'n' })
    win.document.addEventListener('keydown', (event) => event.preventDefault(), { once: true })
    await press(win, { key: 'n', ctrlKey: true })
    expect(keys(port)).toEqual([])
  })

  it('on macOS keep Cmd editing keys and Option-typed characters for the page, and forward Cmd shortcuts', async () => {
    const { win } = page()
    const port = connect(win, { mac: true })
    for (const key of ['c', 'V', 'x', 'z', 'a']) await press(win, { key, metaKey: true })
    await press(win, { key: 'z', metaKey: true, shiftKey: true })
    await press(win, { key: 'é', altKey: true })
    expect(keys(port)).toEqual([])

    await press(win, { key: 'p', code: 'KeyP', metaKey: true })
    await press(win, { key: 'c', code: 'KeyC', ctrlKey: true })
    expect(keys(port)).toEqual([
      { t: 'key', key: { key: 'p', code: 'KeyP', ctrlKey: false, shiftKey: false, altKey: false, metaKey: true, repeat: false } },
      { t: 'key', key: { key: 'c', code: 'KeyC', ctrlKey: true, shiftKey: false, altKey: false, metaKey: false, repeat: false } }
    ])
  })
})

describe('the surface title', () => {
  it("is trimmed and capped, and empty or not text means the manifest's title", () => {
    const { win, floti } = page()
    const port = connect(win)
    floti.surface.setTitle('  Second item  ')
    floti.surface.setTitle('x'.repeat(200))
    floti.surface.setTitle('   ')
    floti.surface.setTitle(null)
    expect(sent(port).filter((entry) => entry.t === 'title')).toEqual([
      { t: 'title', title: 'Second item' },
      { t: 'title', title: 'x'.repeat(120) },
      { t: 'title', title: null },
      { t: 'title', title: null }
    ])
  })

  it('is kept when the page asks', () => {
    const { win, floti } = page()
    const port = connect(win)
    floti.surface.keep()
    expect(sent(port).filter((entry) => entry.t === 'keep')).toEqual([{ t: 'keep' }])
  })
})

describe('tools', () => {
  it('are handled in the background module only, never on a page', () => {
    const { floti } = page()
    expect(() => floti.tools.handle('pick', () => 'x')).toThrow(
      expect.objectContaining({ code: 'invalid', message: 'floti.tools.handle works in the background module only' }) as Error
    )
  })
})

describe('BroadcastChannel, relayed through the local host', () => {
  /** A page drawn from the host's files: its boot says its channels are relayed. */
  const hosted = (context: FlotiContext = PANEL): Page => page(context, JSON.stringify({ theme: THEME, channels: 'host' }))
  type Channel = { postMessage(message: unknown): void; close(): void; onmessage: ((event: MessageEvent) => void) | null }
  const open = (win: Window, name: string): Channel =>
    new (win as unknown as { BroadcastChannel: new (name: string) => Channel }).BroadcastChannel(name)
  const posts = (port: FakePort): Array<Extract<FrameMessage, { t: 'call' }>> =>
    sent(port).filter((entry): entry is Extract<FrameMessage, { t: 'call' }> => entry.t === 'call' && entry.method === 'channel.post')

  it('keeps the browser’s own on a page Floti serves from its own folders', () => {
    const { win } = page()
    const own = (win as unknown as { BroadcastChannel: unknown }).BroadcastChannel
    expect(own === undefined || String(own).includes('[native code]')).toBe(true)
  })

  it('posts a message as the bridge’s channel call, with the channel’s name and its own token', () => {
    const { win } = hosted()
    const port = connect(win)
    open(win, 'sample-items').postMessage({ changed: [3], at: undefined, bytes: new Uint8Array([1, 2]) })
    const [post] = posts(port)
    expect(post?.args).toEqual(['sample-items', { changed: [3], bytes: new Uint8Array([1, 2]) }, expect.any(String)])
  })

  it('hands a delivered message to the page’s other channels of that name, never the one that posted it or another name', () => {
    const { win } = hosted()
    const port = connect(win)
    const poster = open(win, 'sample-items')
    const heard: string[] = []
    poster.onmessage = () => heard.push('poster')
    open(win, 'sample-items').onmessage = (event) => heard.push(`same name: ${JSON.stringify(event.data)}`)
    open(win, 'other').onmessage = () => heard.push('other name')
    poster.postMessage({ changed: 3 })
    const token = posts(port)[0]?.args[2]

    port.postMessage({ t: 'event', name: 'channel', data: { name: 'sample-items', message: { changed: 3 }, token } })
    // From a page elsewhere: every channel of the name hears it.
    port.postMessage({ t: 'event', name: 'channel', data: { name: 'sample-items', message: 'from the tab', token: 'elsewhere' } })
    expect(heard).toEqual(['same name: {"changed":3}', 'poster', 'same name: "from the tab"'])
  })

  it.each([
    ['a function', () => () => 1],
    ['a symbol', () => Symbol('x')],
    ['a BigInt', () => 10n],
    ['an object holding a function', () => ({ save: () => undefined })],
    ['a Map', () => new Map([[1, 2]])],
    ['a cyclic object', () => {
      const cyclic: Record<string, unknown> = {}
      cyclic['self'] = cyclic
      return cyclic
    }]
  ])('throws a DataCloneError for %s, and sends nothing', (_, make) => {
    const { win } = hosted()
    const port = connect(win)
    const channel = open(win, 'sample-items')
    let thrown: unknown = null
    try {
      channel.postMessage(make())
    } catch (error) {
      thrown = error
    }
    expect((thrown as DOMException | null)?.name).toBe('DataCloneError')
    expect(posts(port)).toEqual([])
  })

  it('refuses a post on a closed channel and delivers nothing to it', () => {
    const { win } = hosted()
    const port = connect(win)
    const channel = open(win, 'sample-items')
    const heard = vi.fn()
    channel.onmessage = heard
    channel.close()
    expect(() => channel.postMessage('late')).toThrow(expect.objectContaining({ name: 'InvalidStateError' }) as Error)
    port.postMessage({ t: 'event', name: 'channel', data: { name: 'sample-items', message: 'x', token: 'elsewhere' } })
    expect(heard).not.toHaveBeenCalled()
  })
})
