import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { ClaudeStatus } from '../../../shared/ipc'
import { bridge, remoteBridge } from './bridge.testkit'
import { useClaudeStatusOn } from './useClaudeStatusOn'

vi.mock('./bridge', () => import('./bridge.testkit'))

const BOX = 'h1'
const FOUND = { path: '/usr/local/bin/claude', version: '2.1.330 (Claude Code)' } as ClaudeStatus

beforeEach(() => {
  bridge.reset()
  remoteBridge(BOX).reset()
  remoteBridge('h2').reset()
})

describe('useClaudeStatusOn', () => {
  it('asks build-box through its own bridge, and asks again on Check again', async () => {
    const box = remoteBridge(BOX)
    box.answer('setup:status', () => FOUND)
    const { result } = renderHook(() => useClaudeStatusOn(BOX))
    await waitFor(() => expect(result.current.status).toEqual(FOUND))
    expect(box.invoked('setup:status')).toHaveLength(1)

    act(() => result.current.recheck())
    expect(result.current.checking).toBe(true)
    await waitFor(() => expect(result.current.checking).toBe(false))
    expect(box.invoked('setup:status')).toHaveLength(2)
    expect(bridge.invocations).toEqual([])
  })

  it('says why it could not look rather than not found, and asks nothing for no host', async () => {
    remoteBridge(BOX).answer('setup:status', () => {
      throw new Error('build-box is not connected.')
    })
    const { result, rerender } = renderHook(({ host }) => useClaudeStatusOn(host), {
      initialProps: { host: BOX as string | null }
    })
    await waitFor(() => expect(result.current.statusError).toBe('build-box is not connected.'))
    expect(result.current.status).toBeNull()

    rerender({ host: null })
    expect(result.current.status).toBeNull()
    expect(result.current.statusError).toBeNull()
  })

  it('never shows one host’s answer as another’s', async () => {
    remoteBridge(BOX).answer('setup:status', () => FOUND)
    remoteBridge('h2').answer('setup:status', () => new Promise<ClaudeStatus>(() => undefined))
    const { result, rerender } = renderHook(({ host }) => useClaudeStatusOn(host), { initialProps: { host: BOX } })
    await waitFor(() => expect(result.current.status).toEqual(FOUND))
    rerender({ host: 'h2' })
    expect(result.current.status).toBeNull()
  })
})
