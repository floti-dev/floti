import { act, renderHook, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { OwnDataCopyState } from '../../../shared/ipc'
import { bridge } from './bridge.testkit'
import { useOwnDataCopy } from './useOwnDataCopy'

vi.mock('./bridge', () => import('./bridge.testkit'))

/**
 * The notice that copying what only Helm's own database holds into the local
 * host stopped part way: the copy's state read once, then each change main
 * pushes, until it is dismissed or a later attempt finishes.
 */

const REASON = 'the local host refused a batch of conversations (failed)'

const unfinished: OwnDataCopyState = {
  state: 'unfinished',
  copied: { conversations: 2, messages: 3, usageRows: 1, sessionNames: 1 },
  reason: REASON
}

afterEach(() => {
  bridge.reset()
})

describe('useOwnDataCopy', () => {
  it('says why a copy that stopped part way did not finish, and nothing once a later one finishes', async () => {
    bridge.answer('ownData:state', () => unfinished)
    const { result } = renderHook(() => useOwnDataCopy())
    await waitFor(() => expect(result.current.unfinished).toBe(REASON))

    act(() => bridge.emit('ownData:changed', { state: 'copying' }))
    expect(result.current.unfinished).toBeNull()
    act(() => bridge.emit('ownData:changed', { state: 'finished' }))
    expect(result.current.unfinished).toBeNull()
  })

  it('says nothing while the copy runs or after it finished', async () => {
    bridge.answer('ownData:state', () => ({ state: 'finished' }))
    const { result } = renderHook(() => useOwnDataCopy())
    await waitFor(() => expect(bridge.invoked('ownData:state')).toHaveLength(1))
    expect(result.current.unfinished).toBeNull()

    act(() => bridge.emit('ownData:changed', { state: 'copying' }))
    expect(result.current.unfinished).toBeNull()
  })

  it('hears a copy that stops after the first read, and keeps quiet once dismissed', async () => {
    bridge.answer('ownData:state', () => ({ state: 'copying' }))
    const { result } = renderHook(() => useOwnDataCopy())
    await waitFor(() => expect(bridge.invoked('ownData:state')).toHaveLength(1))
    expect(result.current.unfinished).toBeNull()

    act(() => bridge.emit('ownData:changed', unfinished))
    expect(result.current.unfinished).toBe(REASON)

    act(() => result.current.dismiss())
    expect(result.current.unfinished).toBeNull()
    act(() => bridge.emit('ownData:changed', unfinished))
    expect(result.current.unfinished).toBeNull()
  })
})
