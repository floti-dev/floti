import { act, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { AgentChangeResult } from '@helm/core/types'
import { LOCAL_HOST_KEY } from '@helm/core/types'
import { bridge, remoteBridge } from '../bridge.testkit'
import { restoreAgent, useAgentToast } from './useAgentToast'

vi.mock('../bridge', () => import('../bridge.testkit'))

const restored = (refusal: string | null): AgentChangeResult => ({
  changed: refusal === null,
  file: null,
  refusal,
  problems: [],
  conflict: null
})

beforeEach(() => {
  vi.useFakeTimers()
  bridge.reset()
  remoteBridge('buildbox').reset()
})
afterEach(() => vi.useRealTimers())

describe('a deleted agent’s toast', () => {
  it('stays until closed, and its Undo puts the agent back through agents:restore on its host', async () => {
    const there = remoteBridge('buildbox')
    there.answer('agents:restore', () => restored(null))
    const { result } = renderHook(() => useAgentToast())

    act(() => result.current.raise('Deleted triage', 'Undo puts it back.', restoreAgent('buildbox', 'triage')))
    await act(async () => void (await vi.advanceTimersByTimeAsync(60 * 60 * 1000)))
    expect(result.current.toast?.title).toBe('Deleted triage')

    await act(() => result.current.runUndo())
    expect(there.invoked('agents:restore')).toEqual([{ name: 'triage' }])
    expect(bridge.invoked('agents:restore')).toEqual([])
    expect(result.current.toast).toBeNull()
  })

  it('keeps the toast and says why when the host does not put it back', async () => {
    bridge.answer('agents:restore', () => restored('triage.md changed since it was deleted.'))
    const { result } = renderHook(() => useAgentToast())
    act(() => result.current.raise('Deleted triage', null, restoreAgent(LOCAL_HOST_KEY, 'triage')))

    await act(() => result.current.runUndo())
    expect(result.current.toast).toMatchObject({ title: 'Deleted triage', undoing: false, error: 'triage.md changed since it was deleted.' })

    act(() => result.current.close())
    expect(result.current.toast).toBeNull()
  })

  it('is replaced by the next change, whose Undo is its own', async () => {
    bridge.answer('agents:restore', () => restored(null))
    const { result } = renderHook(() => useAgentToast())
    act(() => result.current.raise('Deleted triage', null, restoreAgent(LOCAL_HOST_KEY, 'triage')))
    act(() => result.current.raise('Deleted lead', null, restoreAgent(LOCAL_HOST_KEY, 'lead')))
    expect(result.current.toast?.title).toBe('Deleted lead')

    await act(() => result.current.runUndo())
    expect(bridge.invoked('agents:restore')).toEqual([{ name: 'lead' }])
  })
})
