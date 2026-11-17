import { describe, expect, it } from 'vitest'
import { LOCAL_HOST_KEY } from '@floti/core/types'
import type * as P from '../host/protocol'
import { pluginFromHost } from '../host/records'
import {
  formatPluginOrigin,
  hostFrameKey,
  hostLabel,
  hostOfLabel,
  onHostOrigin,
  parsePluginOrigin,
  pluginOrigin
} from './origin'

const BUILD_BOX = '6f1d2c3a-1b2c-4d5e-8f90-a1b2c3d4e5f6'

describe('a plugin’s origin on each host', () => {
  it('s19: keeps This PC’s plugin on floti-plugin://<id> and gives the same plugin on build-box an origin of its own', () => {
    expect(pluginOrigin({ host: LOCAL_HOST_KEY, plugin: 'trackr' })).toBe('floti-plugin://trackr')
    const theirs = pluginOrigin({ host: BUILD_BOX, plugin: 'trackr' })
    expect(theirs).toBe(`floti-plugin://trackr.${BUILD_BOX}`)
    expect(theirs).not.toBe(pluginOrigin({ host: LOCAL_HOST_KEY, plugin: 'trackr' }))
    expect(theirs).not.toBe(pluginOrigin({ host: 'another-host', plugin: 'trackr' }))
  })

  it('derives a DNS-safe label from any host id, the same every time, and never one for This PC', () => {
    expect(hostLabel(BUILD_BOX)).toBe(BUILD_BOX)
    const odd = hostLabel('Build Box/2')
    expect(odd).toMatch(/^h[0-9a-f]{16}$/)
    expect(hostLabel('Build Box/2')).toBe(odd)
    expect(hostLabel('Build Box/3')).not.toBe(odd)
    expect(() => hostLabel(LOCAL_HOST_KEY)).toThrow()
  })

  it('reads an origin or a URL back to its plugin and host', () => {
    expect(parsePluginOrigin('floti-plugin://trackr')).toEqual({ plugin: 'trackr', label: null })
    expect(parsePluginOrigin('floti-plugin://trackr/dist/a.html?x=1')).toEqual({ plugin: 'trackr', label: null })
    expect(parsePluginOrigin(`floti-plugin://trackr.${BUILD_BOX}/dist/a.html`)).toEqual({ plugin: 'trackr', label: BUILD_BOX })
    expect(parsePluginOrigin(`FLOTI-PLUGIN://trackr.${BUILD_BOX}`)).toEqual({ plugin: 'trackr', label: BUILD_BOX })
    expect(parsePluginOrigin('https://trackr')).toBeNull()
    expect(parsePluginOrigin('floti-pluginx://trackr')).toBeNull()
    expect(parsePluginOrigin('floti-plugin://')).toBeNull()
    expect(parsePluginOrigin('floti-plugin://user@trackr')).toBeNull()
    expect(parsePluginOrigin('floti-plugin://trackr.bad_label')).toBeNull()
    expect(parsePluginOrigin(null)).toBeNull()
    expect(formatPluginOrigin({ plugin: 'trackr', label: 'build-box' })).toBe('floti-plugin://trackr.build-box')
    expect(formatPluginOrigin({ plugin: 'trackr', label: null })).toBe('floti-plugin://trackr')

    const hosts = [BUILD_BOX, 'Build Box/2']
    expect(hostOfLabel(null, hosts)).toBe(LOCAL_HOST_KEY)
    expect(hostOfLabel(BUILD_BOX, hosts)).toBe(BUILD_BOX)
    expect(hostOfLabel(hostLabel('Build Box/2'), hosts)).toBe('Build Box/2')
    expect(hostOfLabel('not-added', hosts)).toBeNull()
  })

  it('moves a page URL a remote host listed onto that host’s origin, keeping its path, and leaves This PC’s alone', () => {
    const url = 'floti-plugin://trackr/dist/main%20page.html?tab=1#top'
    expect(onHostOrigin(url, { host: LOCAL_HOST_KEY, plugin: 'trackr' })).toBe(url)
    expect(onHostOrigin(url, { host: BUILD_BOX, plugin: 'trackr' })).toBe(
      `floti-plugin://trackr.${BUILD_BOX}/dist/main%20page.html?tab=1#top`
    )
    expect(onHostOrigin('floti-plugin://trackr', { host: BUILD_BOX, plugin: 'trackr' })).toBe(`floti-plugin://trackr.${BUILD_BOX}/`)
  })

  it('keys a remote host’s frame under its host, and This PC’s as today', () => {
    expect(hostFrameKey(undefined, 'background:trackr')).toBe('background:trackr')
    expect(hostFrameKey(LOCAL_HOST_KEY, 'background:trackr')).toBe('background:trackr')
    expect(hostFrameKey(BUILD_BOX, 'background:trackr')).toBe(`host:${BUILD_BOX}/background:trackr`)
  })
})

describe('a plugin as a host lists it', () => {
  const listed: P.PluginInfo = {
    path: '/home/me/plugins/trackr',
    id: 'trackr',
    name: 'Trackr',
    version: '1.0.0',
    description: null,
    enabled: true,
    error: null,
    warnings: [],
    revision: 3,
    icon: null,
    rail: { title: 'Trackr', panel: 'main' },
    panels: { main: { title: 'Trackr', url: 'floti-plugin://trackr/dist/panel.html', actions: [] } },
    tabs: { item: { title: 'Item', url: 'floti-plugin://trackr/dist/item.html' } },
    pageStrip: false,
    background: { url: 'floti-plugin://trackr/dist/background.js', state: 'running', error: null },
    commands: [],
    settings: [],
    settingValues: {},
    network: [],
    secrets: [],
    exec: [],
    service: null,
    runsPrograms: false,
    agent: null,
    startsSessions: false,
    seesSessions: false,
    status: null,
    badge: null
  }

  it('s19 s22: has build-box’s pages and background on build-box’s origin, and This PC’s where they were', () => {
    const ours = pluginFromHost(listed)
    expect(ours.panels['main']?.url).toBe('floti-plugin://trackr/dist/panel.html')
    expect(ours.background?.url).toBe('floti-plugin://trackr/dist/background.js')

    const theirs = pluginFromHost(listed, BUILD_BOX)
    expect(theirs.panels['main']?.url).toBe(`floti-plugin://trackr.${BUILD_BOX}/dist/panel.html`)
    expect(theirs.tabs['item']?.url).toBe(`floti-plugin://trackr.${BUILD_BOX}/dist/item.html`)
    expect(theirs.background?.url).toBe(`floti-plugin://trackr.${BUILD_BOX}/dist/background.js`)
  })
})
