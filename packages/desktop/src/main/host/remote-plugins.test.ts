import { describe, expect, it } from 'vitest'
import { FakeConnection, fakeLink } from '../../../test/host-connection'
import type * as P from './protocol'
import { createRemotePlugins } from './remote-plugins'

const BUILD_BOX = 'build-box'

/** A plugin as a host lists it: on, with a background module. */
const listed = (id: string, patch: Partial<P.PluginInfo> = {}): P.PluginInfo => ({
  path: `/plugins/${id}`,
  id,
  name: id,
  version: '1.0.0',
  description: null,
  enabled: true,
  error: null,
  warnings: [],
  revision: 1,
  icon: null,
  rail: null,
  panels: { main: { title: id, url: `floti-plugin://${id}/panel.html`, actions: [] } },
  tabs: {},
  pageStrip: false,
  background: { url: `floti-plugin://${id}/background.js`, state: 'running', error: null },
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
  badge: null,
  ...patch
})

/** build-box, attached on a connection of its own. */
function buildBox() {
  const remote = new FakeConnection()
  const remotePlugins = createRemotePlugins({ log: () => undefined })
  const remoteLink = fakeLink(remote)
  const attached = remotePlugins.attach(BUILD_BOX, remoteLink)
  return { remote, remoteLink, remotePlugins, attached }
}

describe('each remote host’s plugins', () => {
  it('s19: has build-box’s pages found by its label, served only for the plugins build-box has on', async () => {
    const w = buildBox()
    await w.remoteLink.connect()
    w.remote.snapshot('plugins:changed', [listed('trackr')])
    const plugins = w.remotePlugins.byLabel(BUILD_BOX)
    expect(plugins).not.toBeNull()
    expect(w.remotePlugins.byLabel('lab')).toBeNull()
    expect(await plugins?.serves('trackr')).toBe(true)
    expect(await plugins?.serves('notes')).toBe(false)
  })

  it('serves build-box’s new read of a plugin to a page that loads it before main’s list hears of the reload', async () => {
    const w = buildBox()
    // build-box's files as its current read of Trackr has them: one read, then the next at the same paths.
    let revision = 1
    w.remote.answer('plugins:file', ({ path }) => {
      const body = new TextEncoder().encode(`r${String(revision)} ${path}`)
      return { status: 200, contentType: 'text/plain', version: '0.2.0', revision, totalBytes: body.length, body }
    })
    const first = listed('trackr', { version: '0.2.0', panels: { main: { title: 'Trackr', url: 'floti-plugin://trackr/pages/panel.html', actions: [] } } })
    await w.remoteLink.connect()
    w.remote.snapshot('plugins:changed', [first])
    const plugins = w.remotePlugins.byLabel(BUILD_BOX)!
    const text = async (path: string): Promise<string> => new TextDecoder().decode((await plugins.file('trackr', path)).body)
    expect(await text('pages/panel.html')).toBe('r1 pages/panel.html')
    expect(await text('pages/panel.js')).toBe('r1 pages/panel.js')

    // Reloaded on build-box, same version string. The window's own
    // connection heard it and the frame reloaded; main's link has not heard
    // it yet.
    revision = 2
    expect(await text('pages/panel.html')).toBe('r2 pages/panel.html')
    expect(await text('pages/panel.js')).toBe('r2 pages/panel.js')

    // Main's list catches up: the new read's files are kept from then on.
    w.remote.push('plugins:changed', [{ ...first, revision: 2 }])
    const asked = w.remote.payloads('plugins:file').length
    expect(await text('pages/panel.js')).toBe('r2 pages/panel.js')
    expect(await text('pages/panel.js')).toBe('r2 pages/panel.js')
    expect(w.remote.payloads('plugins:file')).toHaveLength(asked + 1)
  })

  it('lets go of a host removed, and of every host at shutdown', async () => {
    const w = buildBox()
    await w.remoteLink.connect()
    w.remote.snapshot('plugins:changed', [listed('trackr')])
    w.attached.stop()
    expect(w.remotePlugins.byLabel(BUILD_BOX)).toBeNull()

    const again = w.remotePlugins.attach(BUILD_BOX, w.remoteLink)
    expect(w.remotePlugins.byLabel(BUILD_BOX)).not.toBeNull()
    w.remotePlugins.shutdown()
    expect(w.remotePlugins.byLabel(BUILD_BOX)).toBeNull()
    again.stop()
  })
})
