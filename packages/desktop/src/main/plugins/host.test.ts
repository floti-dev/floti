import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { BrowserWindow, WebContents } from 'electron'
import type { FlotiTheme } from '@floti-dev/plugin-sdk'
import type { PluginHost } from './host'

vi.mock('electron', async () => (await import('../../../test/electron')).electronFake())

const { createPluginHost } = await import('./host')

/**
 * What the desktop keeps of plugins for its own pages: the runtime and theme
 * they paint with, and which page frames them. The window is a fake that
 * records what it was sent, so every assertion is about what the plugin host
 * decided.
 */

let sent: Array<{ channel: string; payload: unknown }>
let host: PluginHost

const THEME: FlotiTheme = { kind: 'dark', tokens: {} as FlotiTheme['tokens'], radius: 3, density: 'comfortable' }

const fakeWindow = {
  isDestroyed: () => false,
  webContents: {
    id: 1,
    isDestroyed: () => false,
    send: (channel: string, payload: unknown) => sent.push({ channel, payload })
  } as unknown as WebContents
} as unknown as BrowserWindow

beforeEach(() => {
  sent = []
  host = createPluginHost({ window: () => fakeWindow, theme: () => THEME, bridge: 'bridge', stylesheet: 'css' })
})

describe('the page that frames plugins', () => {
  it('is the window, and nothing else', () => {
    expect(host.framesPlugins(1)).toBe(true)
    expect(host.framesPlugins(2)).toBe(false)
  })

  it('is not one once the window is gone', () => {
    const gone = createPluginHost({ window: () => null, theme: () => THEME, bridge: 'bridge', stylesheet: 'css' })
    expect(gone.framesPlugins(1)).toBe(false)
    gone.pushTheme(THEME)
    expect(sent).toEqual([])
  })

  it('is served the runtime, with the theme a page paints first', () => {
    expect(host.runtime.bridge()).toBe('bridge')
    expect(host.runtime.css()).toBe('css')
    expect(JSON.parse(host.runtime.boot('tracker'))).toEqual({ theme: THEME })
  })

  it('is told about a theme change', () => {
    const light: FlotiTheme = { ...THEME, kind: 'light' }
    host.pushTheme(light)
    expect(sent).toEqual([{ channel: 'plugins:theme', payload: light }])
  })
})
