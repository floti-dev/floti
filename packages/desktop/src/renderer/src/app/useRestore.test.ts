import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { RestoreOffer, SessionRecord } from '@helm/core'
import type { LaunchedSession, RestoreSessionsResult } from '../../../shared/ipc'
import { bridge } from './bridge.testkit'
import { restoredPairs, restoreReport, useOpenOnFailedRead, useRestore } from './useRestore'

vi.mock('./bridge', () => import('./bridge.testkit'))

const OFFER: RestoreOffer = {
  sessions: [
    {
      id: 4,
      name: 'tab baseline',
      cwd: 'C:\\work\\helm',
      branch: 'main',
      profile: null,
      profileGone: false,
      lastAt: 1,
      blocked: null
    }
  ],
  elsewhere: 0,
  layout: null
}

const launched = (id: number, warnings: string[] = []): LaunchedSession => ({
  session: { id, name: `s${String(id)}` } as SessionRecord,
  overlays: [],
  composedInstructions: false,
  warnings
})

beforeEach(() => {
  bridge.reset()
  bridge.answer('session:restorable', () => OFFER)
})

describe('useRestore', () => {
  it('reads the offer once, reopens what it is asked to, and is then answered', async () => {
    const result: RestoreSessionsResult = { restored: [{ from: 4, launched: launched(11) }], failed: [] }
    bridge.answer('session:restore', () => result)
    const { result: hook } = renderHook(() => useRestore())
    await waitFor(() => expect(hook.current.offer).toEqual(OFFER))

    let answer: RestoreSessionsResult | null = null
    await act(async () => {
      answer = await hook.current.restore([{ id: 4, cols: 100, rows: 30 }])
    })
    expect(answer).toEqual(result)
    expect(bridge.invoked('session:restore')).toEqual([{ sessions: [{ id: 4, cols: 100, rows: 30 }] }])
    expect(bridge.invoked('session:restorable')).toHaveLength(1)
    expect(hook.current.offer).toBeNull()
    expect(hook.current.busy).toBe(false)
    // Asked, and everything came back as it was: nothing to say.
    expect(hook.current.report).toBeNull()
  })

  it('asks again for what the host offers each time the window gets its host back after losing it', async () => {
    const { result: hook, rerender } = renderHook(({ reconnects }) => useRestore(reconnects), {
      initialProps: { reconnects: 0 }
    })
    await waitFor(() => expect(hook.current.offer).toEqual(OFFER))
    const lost: RestoreOffer = {
      ...OFFER,
      sessions: [{ ...OFFER.sessions[0]!, id: 9, name: 'lost with its host' }]
    }
    bridge.answer('session:restorable', () => lost)

    rerender({ reconnects: 1 })

    await waitFor(() => expect(hook.current.offer).toEqual(lost))
    expect(bridge.invoked('session:restorable')).toHaveLength(2)
  })

  it('asks again for what the host offers when a session is lost while the host stays, its keeper gone', async () => {
    bridge.answer('session:restorable', () => null)
    const { result: hook } = renderHook(() => useRestore(0))
    await waitFor(() => expect(bridge.invoked('session:restorable')).toHaveLength(1))
    const lost: RestoreOffer = {
      ...OFFER,
      sessions: [{ ...OFFER.sessions[0]!, id: 9, name: 'lost with its keeper' }]
    }
    bridge.answer('session:restorable', () => lost)

    // An ordinary end asks nothing; a lost one does.
    act(() => bridge.emit('session:exit', { id: 8, status: 'exited', exitCode: 0 } as SessionRecord))
    act(() => bridge.emit('session:exit', { id: 9, status: 'lost', exitCode: null } as SessionRecord))

    await waitFor(() => expect(hook.current.offer).toEqual(lost))
    expect(bridge.invoked('session:restorable')).toHaveLength(2)
  })

  it('says it could not read what was running when the read fails, and Try again reads again', async () => {
    bridge.answer('session:restorable', () => {
      throw new Error("Error invoking remote method 'session:restorable': Error: The sessions' titles could not be read.")
    })
    const { result: hook } = renderHook(() => useRestore())
    await waitFor(() => expect(hook.current.unread).toBe("The sessions' titles could not be read."))
    expect(hook.current.offer).toBeNull()
    expect(hook.current.retrying).toBe(false)

    bridge.answer('session:restorable', () => OFFER)
    act(() => hook.current.retry())
    expect(hook.current.retrying).toBe(true)

    await waitFor(() => expect(hook.current.offer).toEqual(OFFER))
    expect(hook.current.unread).toBeNull()
    expect(hook.current.retrying).toBe(false)
    expect(bridge.invoked('session:restorable')).toHaveLength(2)
  })

  it('keeps an offer it already holds when a later read fails, and forgets the failure when asked', async () => {
    const { result: hook, rerender } = renderHook(({ reconnects }) => useRestore(reconnects), {
      initialProps: { reconnects: 0 }
    })
    await waitFor(() => expect(hook.current.offer).toEqual(OFFER))
    bridge.answer('session:restorable', () => {
      throw new Error('The host stopped answering.')
    })

    rerender({ reconnects: 1 })

    await waitFor(() => expect(hook.current.unread).toBe('The host stopped answering.'))
    expect(hook.current.offer).toEqual(OFFER)
    act(() => hook.current.dismissUnread())
    expect(hook.current.unread).toBeNull()
  })

  it('a read that answers with nothing is nothing to restore, never could not read', async () => {
    bridge.answer('session:restorable', () => null)
    const { result: hook } = renderHook(() => useRestore())
    await waitFor(() => expect(bridge.invoked('session:restorable')).toHaveLength(1))
    await waitFor(() => expect(hook.current.retrying).toBe(false))
    expect(hook.current.offer).toBeNull()
    expect(hook.current.unread).toBeNull()
  })

  it('answers not now with nothing', async () => {
    bridge.answer('session:restore', () => ({ restored: [], failed: [] }))
    const { result: hook } = renderHook(() => useRestore())
    await waitFor(() => expect(hook.current.offer).not.toBeNull())
    act(() => hook.current.dismiss())
    expect(hook.current.offer).toBeNull()
    await waitFor(() => expect(bridge.invoked('session:restore')).toEqual([{ sessions: [] }]))
  })

  it('says why main refused, without the channel Electron puts on it', async () => {
    bridge.answer('session:restore', () => {
      throw new Error(
        "Error invoking remote method 'session:restore': Error: The sessions from before Floti closed have already been answered for."
      )
    })
    const { result: hook } = renderHook(() => useRestore())
    await waitFor(() => expect(hook.current.offer).not.toBeNull())
    await act(async () => {
      expect(await hook.current.restore([{ id: 4, cols: 80, rows: 24 }])).toBeNull()
    })
    expect(hook.current.report).toEqual({
      text: 'The sessions from before Floti closed have already been answered for.',
      failed: true
    })
    expect(hook.current.offer).toBeNull()
    act(() => hook.current.dismissReport())
    expect(hook.current.report).toBeNull()
  })
})

describe('restoreReport', () => {
  it('leads with what did not come back, then what came back short, then that anything happened unasked', () => {
    expect(
      restoreReport(
        {
          restored: [{ from: 1, launched: launched(5) }],
          failed: [{ id: 2, name: 'quiet', reason: 'Claude Code has removed its transcript.' }]
        },
        true
      )
    ).toEqual({
      text: 'Reopened 1 session. “quiet” could not be reopened: Claude Code has removed its transcript.',
      failed: true
    })
    expect(
      restoreReport({ restored: [{ from: 1, launched: launched(5, ['Overlay skipped.']) }], failed: [] }, false)
    ).toEqual({ text: 'Overlay skipped.', failed: false })
    expect(
      restoreReport(
        { restored: [{ from: 1, launched: launched(5) }, { from: 2, launched: launched(6) }], failed: [] },
        true
      )
    ).toEqual({ text: 'Reopened 2 sessions Floti was running when it closed.', failed: false })
    expect(restoreReport({ restored: [], failed: [] }, true)).toBeNull()
  })

  it('says which were already running, shown rather than reopened, after any failure and in place of the unasked line', () => {
    const running = (id: number, name: string) => ({ from: id, session: { id: id + 10, name } as SessionRecord })
    expect(restoreReport({ restored: [{ from: 1, launched: launched(5) }], failed: [], running: [running(2, 'api')] }, true)).toEqual({
      text: '“api” was already running, so it is shown instead of being reopened.',
      failed: false
    })
    expect(
      restoreReport(
        {
          restored: [],
          failed: [{ id: 3, name: 'quiet', reason: 'Its folder is gone.' }],
          running: [running(2, 'api'), running(4, 'web')]
        },
        false
      )
    ).toEqual({
      text:
        '“quiet” could not be reopened: Its folder is gone. “api” was already running, so it is shown instead of being reopened. “web” was already running, so it is shown instead of being reopened.',
      failed: true
    })
  })

  it('pairs each lost row with the session that reopened it or the one already running it', () => {
    expect([
      ...restoredPairs({
        restored: [{ from: 1, launched: launched(5) }],
        failed: [{ id: 3, name: 'quiet', reason: 'gone' }],
        running: [{ from: 2, session: { id: 9, name: 'api' } as SessionRecord }]
      })
    ]).toEqual([
      [1, 5],
      [2, 9]
    ])
  })

  it('names the host that stopped when the sessions went with it', () => {
    expect(
      restoreReport({ restored: [{ from: 1, launched: launched(5) }], failed: [] }, true, 'this PC’s host')
    ).toEqual({ text: 'Reopened 1 session this PC’s host was running when it stopped.', failed: false })
  })
})

describe('useOpenOnFailedRead', () => {
  it('opens the restore page only once the saved layout is in, and once per failure', () => {
    const open = vi.fn()
    const { rerender } = renderHook(({ unread, ready }) => useOpenOnFailedRead(unread, ready, open), {
      initialProps: { unread: 'The host stopped answering.' as string | null, ready: false }
    })
    expect(open).not.toHaveBeenCalled()
    rerender({ unread: 'The host stopped answering.', ready: true })
    expect(open).toHaveBeenCalledTimes(1)
    rerender({ unread: 'Still not answering.', ready: true })
    expect(open).toHaveBeenCalledTimes(1)
    rerender({ unread: null, ready: true })
    rerender({ unread: 'Again.', ready: true })
    expect(open).toHaveBeenCalledTimes(2)
  })
})
