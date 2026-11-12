import { act, renderHook } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { FakeConnection, fakeReach } from '../../../../test/fake-connection'
import { HostRequestError } from '../../../shared/host/client'
import type * as P from '../../../shared/host/protocol'
import { PATIENCE } from '../../../../../../test/patience'
import { createUpdates } from './host/updates'
import { useUpdate, type UpdateDeps } from './useUpdate'

const STATE: P.UpdateState = {
  running: '3.2.0',
  channel: 'stable',
  unavailable: null,
  newest: '3.2.0',
  checkedAt: '2026-11-11T08:00:00Z',
  failed: null,
  checking: false,
  installing: null,
  installFailed: null
}

function depsFor(host: FakeConnection): UpdateDeps {
  return { updates: createUpdates(fakeReach(host), { log: () => undefined }) }
}

async function heard(host: FakeConnection): Promise<void> {
  await vi.waitFor(() => expect(host.subscriptions.has('update:state')).toBe(true), { timeout: PATIENCE })
}

describe('useUpdate', () => {
  it('is nothing until the host is heard, then what it says, newer against this app', async () => {
    const host = new FakeConnection()
    const deps = depsFor(host)
    const { result } = renderHook(() => useUpdate(deps, '3.2.0'))
    expect(result.current.view).toBeNull()
    await heard(host)
    act(() => host.snapshot('update:state', STATE))
    expect(result.current.view?.newer).toBe(false)
    act(() => host.push('update:state', { ...STATE, newest: '3.3.0' }))
    expect(result.current.view).toMatchObject({ newest: '3.3.0', newer: true })
    expect(host.sent).toEqual([])
  })

  it('asks the host to check on every press, busy until the check has ended', async () => {
    const host = new FakeConnection()
    let answer: (state: P.UpdateState) => void = () => undefined
    host.answer(
      'update:check',
      () =>
        new Promise<P.UpdateState>((resolve) => {
          answer = resolve
        }) as unknown as P.UpdateState
    )
    const deps = depsFor(host)
    const { result } = renderHook(() => useUpdate(deps, '3.2.0'))
    await heard(host)
    act(() => host.snapshot('update:state', STATE))
    expect(result.current.checking).toBe(false)

    act(() => result.current.check())
    expect(result.current.checking).toBe(true)
    await vi.waitFor(() => expect(host.payloads('update:check')).toHaveLength(1), { timeout: PATIENCE })
    await act(async () => answer({ ...STATE, newest: '3.3.0' }))
    await vi.waitFor(() => expect(result.current.checking).toBe(false), { timeout: PATIENCE })
    expect(result.current.view?.newest).toBe('3.3.0')

    // A check the host runs by itself shows as checking too.
    act(() => host.push('update:state', { ...STATE, checking: true }))
    expect(result.current.checking).toBe(true)
  })

  it('says why an Update was refused, until the host is next heard', async () => {
    const host = new FakeConnection()
    host.answer('update:install', () => {
      throw new HostRequestError('update:install', 'failed', 'floti update could not be started.')
    })
    const deps = depsFor(host)
    const { result } = renderHook(() => useUpdate(deps, '3.2.0'))
    await heard(host)
    act(() => host.snapshot('update:state', { ...STATE, newest: '3.3.0' }))

    act(() => result.current.install())
    await vi.waitFor(() => expect(result.current.error).toBe('floti update could not be started.'), {
      timeout: PATIENCE
    })
    expect(host.payloads('update:install')).toEqual([null])

    act(() => host.push('update:state', { ...STATE, installing: { line: 'Starting floti update.', waitingForApp: false } }))
    expect(result.current.error).toBeNull()
    expect(result.current.view?.installing?.line).toBe('Starting floti update.')
  })
})
