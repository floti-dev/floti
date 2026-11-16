import { act, renderHook } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { DEFAULT_SETTINGS } from '@helm/core/types'
import type { ClaudeStatus } from '../../../shared/ipc'
import { bridge } from './bridge.testkit'
import { useSetup } from './useSetup'
import { PATIENCE } from '../../../../../../test/patience'

vi.mock('./bridge', () => import('./bridge.testkit'))

const ANSWER: ClaudeStatus = {
  path: '/bin/claude',
  source: 'discovered',
  version: '2.1.225',
  semver: '2.1.225',
  tested: true,
  testedRange: { min: '2.1.0', max: '2.2.0' },
  configDir: '/home/you/.claude',
  configDirExists: true,
  auth: 'authenticated',
  authSignal: 'credentials present',
  error: null
}

beforeEach(() => {
  bridge.reset()
  bridge.whenUnanswered('pending')
})

describe('useSetup status', () => {
  it('keeps status null, with the reason, when the first read fails', async () => {
    bridge.answer('setup:status', () => {
      throw new Error('This PC’s host can’t be used right now; see the notice above.')
    })
    const { result } = renderHook(() => useSetup(DEFAULT_SETTINGS, vi.fn()))
    await vi.waitFor(() => expect(result.current.statusError).toMatch(/can’t be used/), { timeout: PATIENCE })
    expect(result.current.status).toBeNull()
  })

  it('keeps the last answered status when a later read fails, and clears the failure on the next answer', async () => {
    let reply: () => ClaudeStatus = () => ANSWER
    bridge.answer('setup:status', () => reply())
    const { result } = renderHook(() => useSetup(DEFAULT_SETTINGS, vi.fn()))
    await vi.waitFor(() => expect(result.current.status).toEqual(ANSWER), { timeout: PATIENCE })

    reply = () => {
      throw new Error('host gone')
    }
    act(() => result.current.recheck())
    await vi.waitFor(() => expect(result.current.statusError).toBe('host gone'), { timeout: PATIENCE })
    expect(result.current.status).toEqual(ANSWER)

    reply = () => ANSWER
    act(() => result.current.recheck())
    await vi.waitFor(() => expect(result.current.statusError).toBeNull(), { timeout: PATIENCE })
  })

  it('does not turn a refused Locate claude into a missing status', async () => {
    bridge.answer('setup:status', () => ANSWER)
    bridge.answer('setup:locateClaude', () => {
      throw new Error('This PC’s host can’t be used right now; see the notice above.')
    })
    const { result } = renderHook(() => useSetup(DEFAULT_SETTINGS, vi.fn()))
    await vi.waitFor(() => expect(result.current.status).toEqual(ANSWER), { timeout: PATIENCE })

    act(() => result.current.locateClaude())
    await vi.waitFor(() => expect(result.current.statusError).toMatch(/can’t be used/), { timeout: PATIENCE })
    expect(result.current.status).toEqual(ANSWER)
  })
})
