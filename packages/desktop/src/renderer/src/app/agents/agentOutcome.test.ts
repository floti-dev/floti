import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import {
  EMPTY_LAYOUT,
  findTab,
  LOCAL_HOST_KEY,
  openTab,
  paneId,
  type AgentChangeResult,
  type AgentFileText,
  type PaneLayout,
  type PaneRef
} from '@helm/core/types'
import { bridge } from '../bridge.testkit'
import { agentOutcomeIn, agentRefOn, type AgentOutcomeWindow } from './agentOutcome'
import { useAgentDraft, type AgentDraftTarget, type AgentOutcome } from './useAgentDraft'

vi.mock('../bridge', () => import('../bridge.testkit'))

const TEXT = '---\ndescription: Sorts issues.\n---\nYou sort.\n'

const file = (name: string, text: string, hash: string): AgentFileText => ({
  name,
  file: `${name}.md`,
  text,
  hash,
  problems: []
})

const change = (patch: Partial<AgentChangeResult>): AgentChangeResult => ({
  changed: true,
  file: null,
  refusal: null,
  problems: [],
  conflict: null,
  ...patch
})

/** A window holding one tab, `ref`, that agent outcomes act on. */
function windowWith(ref: PaneRef) {
  let layout: PaneLayout = openTab(EMPTY_LAYOUT, ref)
  const dirty = new Set<string>()
  const toasts: { title: string; undo: () => Promise<string | null> }[] = []
  const window: AgentOutcomeWindow = {
    commit: (op) => {
      layout = op(layout)
    },
    markDocDirty: (key, on) => (on ? dirty.add(key) : dirty.delete(key)),
    raise: (title, _detail, undo) => toasts.push({ title, undo })
  }
  return { window, dirty, toasts, has: (tab: PaneRef) => findTab(layout, paneId(tab)) !== null }
}

/** The page as App draws it: its draft kept under its tab's id, its outcomes acted on by the window. */
function page(window: AgentOutcomeWindow, ref: PaneRef, target: AgentDraftTarget) {
  const id = paneId(ref)
  const onOutcome = agentOutcomeIn(window, id, LOCAL_HOST_KEY)
  return renderHook(() => useAgentDraft(LOCAL_HOST_KEY, id, target, onOutcome))
}

/** Starts a save whose answer waits for `answer`. */
function heldWrite() {
  let answer: ((value: AgentChangeResult) => void) | null = null
  bridge.answer('agents:write', () => new Promise<AgentChangeResult>((resolve) => (answer = resolve)))
  return (value: AgentChangeResult) => answer?.(value)
}

beforeEach(() => bridge.reset())

describe('an agent page’s outcome in the window', () => {
  it('moves the tab to the new name on a rename, with what was typed while the save was on its way', async () => {
    const triage = agentRefOn('triage', LOCAL_HOST_KEY)
    const issueTriage = agentRefOn('issue-triage', LOCAL_HOST_KEY)
    const saved = file('issue-triage', TEXT, 'h2')
    bridge.answer('agents:read', ({ name }) => (name === 'triage' ? file('triage', TEXT, 'h1') : saved))
    const win = windowWith(triage)
    const before = page(win.window, triage, { name: 'triage' })
    await waitFor(() => expect(before.result.current.state.loaded).toBe(true))

    const answer = heldWrite()
    act(() => before.result.current.setName('issue-triage'))
    let saving: Promise<unknown> = Promise.resolve()
    act(() => {
      saving = before.result.current.save()
    })
    act(() => before.result.current.setDraft(`${TEXT}typed\n`))
    await act(async () => {
      answer(change({ file: saved }))
      await saving
    })
    expect(win.has(issueTriage)).toBe(true)
    expect(win.has(triage)).toBe(false)
    before.unmount()

    const after = page(win.window, issueTriage, { name: 'issue-triage' })
    expect(after.result.current.state.draft).toBe(`${TEXT}typed\n`)
    expect(after.result.current.state.base?.name).toBe('issue-triage')
    expect(after.result.current.dirty).toBe(true)
  })

  it('moves a new agent’s tab to its name on its first save, with what was typed while it was on its way', async () => {
    const draft: PaneRef = { kind: 'agent-draft', id: 1, copyOf: null }
    const triage = agentRefOn('triage', LOCAL_HOST_KEY)
    const saved = file('triage', TEXT, 'h1')
    bridge.answer('agents:read', () => saved)
    const win = windowWith(draft)
    const before = page(win.window, draft, { copyOf: null, name: 'triage' })
    act(() => before.result.current.setDraft(TEXT))

    const answer = heldWrite()
    let saving: Promise<unknown> = Promise.resolve()
    act(() => {
      saving = before.result.current.save()
    })
    act(() => before.result.current.setDraft(`${TEXT}typed\n`))
    await act(async () => {
      answer(change({ file: saved }))
      await saving
    })
    expect(win.has(triage)).toBe(true)
    expect(win.has(draft)).toBe(false)
    before.unmount()

    const after = page(win.window, triage, { name: 'triage' })
    expect(after.result.current.state.draft).toBe(`${TEXT}typed\n`)
    expect(after.result.current.state.base?.hash).toBe('h1')
    expect(after.result.current.dirty).toBe(true)
  })

  it('gives the tab its old name back on Undo of a save over a change that renamed', async () => {
    const foo = agentRefOn('foo', LOCAL_HOST_KEY)
    const bar = agentRefOn('bar', LOCAL_HOST_KEY)
    bridge.answer('agents:restore', () => change({}))
    const win = windowWith(foo)
    const outcome: AgentOutcome = { kind: 'savedOver', name: 'bar', at: '10:42', from: 'foo', moved: true }
    act(() => agentOutcomeIn(win.window, paneId(foo), LOCAL_HOST_KEY)(outcome))
    expect(win.has(bar)).toBe(true)

    await act(async () => expect(await win.toasts[0]?.undo()).toBeNull())
    expect(bridge.invoked('agents:restore')).toHaveLength(1)
    expect(win.has(foo)).toBe(true)
    expect(win.has(bar)).toBe(false)
  })

  it('undoes a save over a removed file that made it under its new name by restoring the new name', async () => {
    const foo = agentRefOn('foo', LOCAL_HOST_KEY)
    const bar = agentRefOn('bar', LOCAL_HOST_KEY)
    bridge.answer('agents:restore', () => change({}))
    const win = windowWith(foo)
    const outcome: AgentOutcome = { kind: 'savedOver', name: 'bar', at: '10:42', from: 'foo', moved: false }
    act(() => agentOutcomeIn(win.window, paneId(foo), LOCAL_HOST_KEY)(outcome))
    expect(win.has(bar)).toBe(true)

    await act(async () => expect(await win.toasts[0]?.undo()).toBeNull())
    expect(bridge.invoked('agents:restore')).toEqual([{ name: 'bar' }])
    // The page under bar closes as gone when the host says the file went.
    expect(win.has(bar)).toBe(true)
  })
})
