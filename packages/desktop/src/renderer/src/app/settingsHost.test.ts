import { describe, expect, it, vi } from 'vitest'
import { DEFAULT_SETTINGS, LOCAL_HOST_KEY, type AppSettings } from '@floti/core/types'
import type { PluginInfo } from '../../../shared/ipc'
import { secretChoicesOn, settingsHostFor, type SettingsHostDeps } from './settingsHost'

const BOX = 'h1'
const THIS_PC: AppSettings = { ...DEFAULT_SETTINGS, sessionAgents: true }
const BUILDBOX: AppSettings = { ...DEFAULT_SETTINGS, sessionAgents: false }

function deps(overrides: Partial<SettingsHostDeps> = {}): SettingsHostDeps {
  return {
    host: BOX,
    name: 'buildbox',
    reached: true,
    local: THIS_PC,
    remote: (host) => (host === BOX ? BUILDBOX : null),
    writeLocal: vi.fn(),
    writeRemote: vi.fn(async () => null),
    awaySince: undefined,
    log: vi.fn(),
    ...overrides
  }
}

describe('settingsHostFor', () => {
  it('reads the picked host’s settings: buildbox’s through its bridge, This PC’s from the window', () => {
    expect(settingsHostFor(deps()).values?.sessionAgents).toBe(false)
    expect(settingsHostFor(deps({ host: LOCAL_HOST_KEY, name: 'This PC' })).values?.sessionAgents).toBe(true)
  })

  it('sends a change with buildbox picked to buildbox, never to This PC', async () => {
    const given = deps()
    settingsHostFor(given).write({ sessionAgents: true, agentDepth: 3 })
    await Promise.resolve()
    expect(given.writeRemote).toHaveBeenCalledWith(BOX, { sessionAgents: true, agentDepth: 3 })
    expect(given.writeLocal).not.toHaveBeenCalled()
  })

  it('sends this device’s own keys to This PC whichever host is picked', () => {
    const given = deps()
    settingsHostFor(given).write({ sessionMcp: false, browserMcpLocalOnly: true, showChildrenInParentTab: false })
    expect(given.writeRemote).toHaveBeenCalledWith(BOX, { sessionMcp: false })
    expect(given.writeLocal).toHaveBeenCalledWith({ browserMcpLocalOnly: true, showChildrenInParentTab: false })

    const own = deps()
    settingsHostFor(own).write({ browserMcpLocalOnly: false })
    expect(own.writeRemote).not.toHaveBeenCalled()
    expect(own.writeLocal).toHaveBeenCalledWith({ browserMcpLocalOnly: false })
  })

  it('sends everything through This PC’s bridge with This PC picked, whose adapter splits it', () => {
    const given = deps({ host: LOCAL_HOST_KEY, name: 'This PC' })
    settingsHostFor(given).write({ sessionAgents: false, browserMcpLocalOnly: true })
    expect(given.writeLocal).toHaveBeenCalledWith({ sessionAgents: false, browserMcpLocalOnly: true })
    expect(given.writeRemote).not.toHaveBeenCalled()
  })

  it('reads null for a host out of reach, says when its link failed, and never shows what it last said', () => {
    const at = new Date(2026, 10, 17, 14, 2).getTime()
    const away = settingsHostFor(deps({ reached: false, awaySince: at }))
    expect(away.values).toBeNull()
    expect(away.view).toEqual({
      name: 'buildbox',
      remote: true,
      unknown: { kind: 'away', failedAt: new Date(at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) }
    })
    expect(settingsHostFor(deps({ reached: false })).view.unknown).toEqual({ kind: 'away', failedAt: null })
    expect(settingsHostFor(deps({ host: LOCAL_HOST_KEY, reached: false })).values).toBeNull()
  })

  it('says a reached host that has not answered yet is being read', () => {
    expect(settingsHostFor(deps({ remote: () => null })).view.unknown).toEqual({ kind: 'reading' })
    expect(settingsHostFor(deps()).view.unknown).toBeNull()
  })

  it('reports a change buildbox refused, naming it, and writes nothing to This PC instead', async () => {
    const given = deps({ writeRemote: vi.fn(async () => 'buildbox refused it.') })
    settingsHostFor(given).write({ agentDepth: 9 })
    await vi.waitFor(() => expect(given.log).toHaveBeenCalledWith('Settings: buildbox did not keep the change: buildbox refused it.'))
    expect(given.writeLocal).not.toHaveBeenCalled()
  })
})

describe('secretChoicesOn', () => {
  const plugin = (name: string, keys: string[]): PluginInfo =>
    ({ name, network: [`https://${name}.example.com`], secrets: keys.map((key) => ({ key })) }) as unknown as PluginInfo
  const plugins: Record<string, ReadonlyMap<string, PluginInfo>> = {
    [LOCAL_HOST_KEY]: new Map([['notes', plugin('notes', ['NOTES_TOKEN'])]]),
    [BOX]: new Map([['deploy', plugin('deploy', ['DEPLOY_KEY'])]])
  }
  const pluginsOn = (host: string): ReadonlyMap<string, PluginInfo> => plugins[host] ?? new Map()

  it('offers the plugins on the host it is asked for: buildbox’s with buildbox picked, never This PC’s', () => {
    expect(secretChoicesOn(BOX, pluginsOn)).toEqual([
      { id: 'deploy', name: 'deploy', network: ['https://deploy.example.com'], secrets: ['DEPLOY_KEY'] }
    ])
    expect(secretChoicesOn(LOCAL_HOST_KEY, pluginsOn).map((choice) => choice.id)).toEqual(['notes'])
    expect(secretChoicesOn('gone', pluginsOn)).toEqual([])
  })
})
