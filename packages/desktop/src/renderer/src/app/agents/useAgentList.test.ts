import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { LaunchAgents } from '@helm/core/types'
import { LOCAL_HOST_KEY } from '@helm/core/types'
import { bridge, remoteBridge } from '../bridge.testkit'
import { useAgentList } from './useAgentList'

vi.mock('../bridge', () => import('../bridge.testkit'))

const listing = (names: string[], folder: string): LaunchAgents =>
  ({
    agents: names.map((name) => ({ name })),
    bad: [],
    backends: [],
    folder,
    readOnly: null,
    shipped: []
  }) as unknown as LaunchAgents

const names = (state: { listing: LaunchAgents | null }): string[] | null =>
  state.listing?.agents.map((agent) => agent.name) ?? null

beforeEach(() => {
  bridge.reset()
  remoteBridge('buildbox').reset()
})

describe('useAgentList with two hosts connected', () => {
  it('lists the chosen host’s agents from that host, and never shows one host’s list as the other’s', async () => {
    const buildbox = remoteBridge('buildbox')
    bridge.answer('agents:list', () => listing(['default', 'lead'], '~/.config/floti/agents'))
    let answer: ((value: LaunchAgents) => void) | null = null
    buildbox.answer('agents:list', () => new Promise<LaunchAgents>((resolve) => (answer = resolve)))
    const { result, rerender } = renderHook(({ host }) => useAgentList(host, true), {
      initialProps: { host: LOCAL_HOST_KEY }
    })
    await waitFor(() => expect(names(result.current)).toEqual(['default', 'lead']))

    // Switched to build-box: this PC's list goes at once, before build-box answers.
    rerender({ host: 'buildbox' })
    expect(result.current.listing).toBeNull()
    expect(buildbox.invoked('agents:list')).toHaveLength(1)
    act(() => answer?.(listing(['default', 'reviewer'], '/home/me/.config/floti/agents')))
    await waitFor(() => expect(names(result.current)).toEqual(['default', 'reviewer']))
    expect(result.current.listing?.folder).toBe('/home/me/.config/floti/agents')
    expect(bridge.invoked('agents:list')).toHaveLength(1)
  })

  it('reads again when the chosen host says its folder changed, and not for the other host', async () => {
    const buildbox = remoteBridge('buildbox')
    let lab = ['default']
    buildbox.answer('agents:list', () => listing(lab, '/srv/agents'))
    bridge.answer('agents:list', () => listing(['default'], '~/agents'))
    const { result } = renderHook(() => useAgentList('buildbox', true))
    await waitFor(() => expect(names(result.current)).toEqual(['default']))

    lab = ['default', 'triage']
    act(() => buildbox.emit('agents:changed', { names: ['triage'] }))
    await waitFor(() => expect(names(result.current)).toEqual(['default', 'triage']))
    act(() => bridge.emit('agents:changed', { names: ['lead'] }))
    expect(buildbox.invoked('agents:list')).toHaveLength(2)
    expect(bridge.invoked('agents:list')).toEqual([])
  })

  it('reads nothing while no surface shows an agent', () => {
    renderHook(() => useAgentList(LOCAL_HOST_KEY, false))
    expect(bridge.invoked('agents:list')).toEqual([])
  })

  it('adds a shipped agent on the chosen host, and shows a refused Add only on the host that refused it', async () => {
    const buildbox = remoteBridge('buildbox')
    bridge.answer('agents:list', () => listing(['default'], '~/agents'))
    buildbox.answer('agents:list', () => listing(['default'], '/srv/agents'))
    bridge.answer('agents:addShipped', () => ({
      changed: false,
      file: null,
      refusal: 'This host changes nothing in its agents folder.',
      problems: [],
      conflict: null
    }))
    const { result, rerender } = renderHook(({ host }) => useAgentList(host, true), {
      initialProps: { host: LOCAL_HOST_KEY }
    })
    act(() => result.current.addShipped('reviewer'))
    await waitFor(() => expect(result.current.addError).toBe('This host changes nothing in its agents folder.'))
    expect(bridge.invoked('agents:addShipped')).toEqual([{ name: 'reviewer' }])

    rerender({ host: 'buildbox' })
    expect(result.current.addError).toBeNull()
    rerender({ host: LOCAL_HOST_KEY })
    expect(result.current.addError).toBe('This host changes nothing in its agents folder.')
  })
})
