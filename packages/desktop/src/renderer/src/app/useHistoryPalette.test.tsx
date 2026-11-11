import { act, renderHook, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { HistoryPage, HistoryProject, HistoryQuery, HistorySession, HistorySummary } from '@helm/core'
import { bridge, remoteBridge } from './bridge.testkit'
import { splitQuery, useHistoryPalette } from './useHistoryPalette'

vi.mock('./bridge', () => import('./bridge.testkit'))

const session = (sessionId: string, patch: Partial<HistorySession> = {}): HistorySession =>
  ({ sessionId, label: null, title: sessionId, ...patch }) as HistorySession

const page = (...sessions: HistorySession[]): HistoryPage => ({ sessions, total: sessions.length, tookMs: 0 })

const project = (name: string, path: string, lastAt: number): HistoryProject => ({
  project: path,
  name,
  sessions: 1,
  prompts: 1,
  lastAt,
  resumable: 1,
  exists: true
})

beforeEach(() => {
  bridge.whenUnanswered('pending')
})

afterEach(() => {
  bridge.reset()
})

describe('a query, as History asks the host it', () => {
  it('narrows to the most recent folder a word names and searches the rest', () => {
    const projects = [project('ledgerline', '/old/ledgerline', 1), project('ledgerline', '/code/ledgerline', 5)]
    expect(splitQuery('ledgerline export', projects)).toEqual({ search: 'export', project: '/code/ledgerline' })
    expect(splitQuery('Export  queue', projects)).toEqual({ search: 'Export queue', project: null })
  })
})

describe('History’s reads', () => {
  it('reads nothing while closed, and the 10 worked in last at the top level when it opens', async () => {
    bridge.answer('history:sessions', () => page(session('a')))
    const { result, rerender } = renderHook(({ open }) => useHistoryPalette('local', open), {
      initialProps: { open: false }
    })
    expect(bridge.invoked('history:sessions')).toEqual([])
    rerender({ open: true })
    await waitFor(() => expect(result.current.recent).toEqual({ sessions: [session('a')], total: 1 }))
    expect(bridge.invoked('history:sessions')).toEqual([{ topLevel: true, limit: 10 }])
  })

  it('asks both lists of a query at once, narrowed to a folder its words name', async () => {
    bridge.answer('history:projects', () => [project('atlas-api', '/code/atlas-api', 1)])
    bridge.answer('history:sessions', (query: HistoryQuery) =>
      query.scope === 'messages' ? page(session('said')) : query.scope === 'prompts' ? page(session('typed')) : page()
    )
    const { result } = renderHook(() => useHistoryPalette('local', true))
    await waitFor(() => expect(bridge.invoked('history:projects')).toHaveLength(1))
    await waitFor(() => expect(result.current.recent).not.toBeNull())
    act(() => result.current.setQuery('atlas-api rate limit'))
    await waitFor(() => expect(result.current.conversations).toEqual({ sessions: [session('said')], total: 1 }))
    expect(result.current.titles).toEqual({ sessions: [session('typed')], total: 1 })
    expect(bridge.invoked('history:sessions').slice(1)).toEqual([
      { search: 'rate limit', scope: 'prompts', limit: 50, project: '/code/atlas-api' },
      { search: 'rate limit', scope: 'messages', limit: 50, project: '/code/atlas-api' }
    ])
  })

  it('says a list could not be read with the host’s sentence', async () => {
    bridge.answer('history:sessions', (query: HistoryQuery) => {
      if (query.scope === 'messages') throw new Error('database is locked')
      return page()
    })
    const { result } = renderHook(() => useHistoryPalette('local', true))
    act(() => result.current.setQuery('sqlite'))
    await waitFor(() => expect(result.current.conversations).toEqual({ error: 'database is locked' }))
  })

  it('reads a parent’s children when it is unfolded', async () => {
    bridge.answer('history:sessions', (query: HistoryQuery) =>
      query.parent === 'lead' ? page(session('kid', { parent: 'lead' })) : page(session('lead', { children: 1 }))
    )
    const { result } = renderHook(() => useHistoryPalette('local', true))
    await waitFor(() => expect(result.current.recent).not.toBeNull())
    act(() => result.current.unfold(session('lead')))
    await waitFor(() =>
      expect(result.current.children.get('lead')).toEqual({ sessions: [session('kid', { parent: 'lead' })], total: 1 })
    )
    expect(bridge.invoked('history:sessions')).toContainEqual({ parent: 'lead', limit: 50 })
  })

  it('lists a session started outside Floti as soon as the host indexes it, without the window asking', async () => {
    let sessions = [session('a')]
    bridge.answer('history:sessions', () => page(...sessions))
    const { result } = renderHook(() => useHistoryPalette('local', true))
    await waitFor(() => expect(result.current.recent).toEqual({ sessions: [session('a')], total: 1 }))
    sessions = [session('terminal'), session('a')]
    act(() => {
      bridge.emit('history:changed', { sessions: 2 } as HistorySummary)
    })
    await waitFor(() => expect(result.current.recent).toEqual({ sessions: [session('terminal'), session('a')], total: 2 }))
    expect(result.current.summary).toEqual({ sessions: 2 })
    expect(bridge.invoked('history:refresh')).toEqual([])
  })

  it('says the count and the folders could not be read, never that there are none', async () => {
    bridge.answer('history:summary', () => {
      throw new Error('The host did not answer.')
    })
    bridge.answer('history:projects', () => {
      throw new Error('database is locked')
    })
    bridge.answer('history:sessions', () => page())
    const { result } = renderHook(() => useHistoryPalette('local', true))
    await waitFor(() => expect(result.current.summaryError).toBe('The host did not answer.'))
    await waitFor(() => expect(result.current.foldersError).toBe('database is locked'))
    expect(result.current.summary).toBeNull()
  })

  it('drops a retry that lands after History moved to another host', async () => {
    let settle: (summary: HistorySummary) => void = () => undefined
    bridge.answer('history:refresh', () => new Promise<HistorySummary>((resolve) => (settle = resolve)))
    bridge.answer('history:sessions', () => page())
    remoteBridge('h1').whenUnanswered('pending')
    const { result, rerender } = renderHook(({ host }) => useHistoryPalette(host, true), { initialProps: { host: 'local' } })
    act(() => result.current.retry())
    rerender({ host: 'h1' })
    await act(async () => {
      settle({ sessions: 9 } as HistorySummary)
      await Promise.resolve()
    })
    expect(result.current.summary).toBeNull()
  })

  it('puts a new name on every list at once, and says a refused rename with its sentence', async () => {
    // The host's index has the name once the rename is written.
    let label: string | null = null
    bridge.answer('history:sessions', () => page(session('a', { label })))
    bridge.answer('history:rename', () => {
      label = 'Rate limits'
      return session('a', { label })
    })
    const { result } = renderHook(() => useHistoryPalette('local', true))
    await waitFor(() => expect(result.current.recent).not.toBeNull())
    let refused: string | null = 'unset'
    await act(async () => {
      refused = await result.current.rename(session('a'), 'Rate limits')
    })
    expect(refused).toBeNull()
    expect(bridge.invoked('history:rename')).toEqual([{ sessionId: 'a', name: 'Rate limits' }])
    await waitFor(() => expect(result.current.recent).toEqual({ sessions: [session('a', { label: 'Rate limits' })], total: 1 }))

    bridge.answer('history:rename', () => {
      throw new Error('The host refused the name.')
    })
    await act(async () => {
      refused = await result.current.rename(session('a'), 'x')
    })
    expect(refused).toBe('The host refused the name.')
  })
})
