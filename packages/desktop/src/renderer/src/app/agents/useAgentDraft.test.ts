import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { AgentChangeResult, AgentFileText } from '@helm/core/types'
import { LOCAL_HOST_KEY } from '@helm/core/types'
import { bridge, remoteBridge } from '../bridge.testkit'
import { forgetAgentDraft, useAgentDraft, type AgentDraftTarget, type AgentOutcome } from './useAgentDraft'

vi.mock('../bridge', () => import('../bridge.testkit'))

const TEXT = '---\ndescription: Plans.\nclass: light\neffort:\n  claude-code: high\n---\nYou plan.\n'

const file = (name: string, text: string, hash = `h-${text.length}`): AgentFileText => ({
  name,
  file: `${name}.md`,
  inFolder: true,
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

let key = 0
function open(target: AgentDraftTarget, host = LOCAL_HOST_KEY) {
  key += 1
  const outcomes: AgentOutcome[] = []
  const hook = renderHook(() => useAgentDraft(host, `tab-${key}`, target, (o) => outcomes.push(o)))
  return { ...hook, outcomes }
}

beforeEach(() => {
  bridge.reset()
  remoteBridge('buildbox').reset()
})

describe('useAgentDraft', () => {
  it('saves the whole draft without its retired line, with the hash it read', async () => {
    bridge.answer('agents:read', ({ name }) => file(name, TEXT, 'h1'))
    bridge.answer('agents:write', ({ name, text }) => change({ file: file(name, text, 'h2') }))
    const { result, outcomes } = open({ name: 'research-lead' })
    await waitFor(() => expect(result.current.state.loaded).toBe(true))
    expect(result.current.dirty).toBe(false)

    act(() => result.current.setDraft(TEXT.replace('high', 'xhigh')))
    expect(result.current.dirty).toBe(true)
    await act(async () => void (await result.current.save()))

    expect(bridge.invoked('agents:write')).toEqual([
      {
        name: 'research-lead',
        text: '---\ndescription: Plans.\neffort:\n  claude-code: xhigh\n---\nYou plan.\n',
        expectedHash: 'h1',
        renameFrom: null
      }
    ])
    expect(result.current.dirty).toBe(false)
    expect(result.current.state.base?.hash).toBe('h2')
    expect(outcomes).toEqual([{ kind: 'saved', name: 'research-lead' }])
  })

  it('keeps the edit and holds each problem when the host refuses the text', async () => {
    bridge.answer('agents:read', ({ name }) => file(name, TEXT))
    const problems = [{ field: 'effort', line: 5, message: 'effort for claude-code must be low, medium, high, xhigh or max' }]
    bridge.answer('agents:write', () => change({ changed: false, refusal: 'It is not an agent.', problems, file: file('x', TEXT) }))
    const { result, outcomes } = open({ name: 'deploy-check' })
    await waitFor(() => expect(result.current.state.loaded).toBe(true))
    act(() => result.current.setDraft(TEXT.replace('high', 'extreme')))
    await act(async () => void (await result.current.save()))

    expect(result.current.state.draft).toContain('extreme')
    expect(result.current.state.problems).toEqual(problems)
    expect(result.current.state.refusal).toBeNull()
    expect(outcomes).toEqual([])
  })

  it('holds the file as it is now beside the edit on a conflict, and Save mine writes over it', async () => {
    bridge.answer('agents:read', ({ name }) => file(name, TEXT, 'h1'))
    const onDisk = file('lead', TEXT.replace('Plans.', 'Plans briefly.'), 'h3')
    let first = true
    bridge.answer('agents:write', ({ name, text }) => {
      if (first) {
        first = false
        return change({ changed: false, refusal: 'It changed.', file: onDisk, conflict: { onDiskHash: 'h3', onDiskText: onDisk.text } })
      }
      return change({ file: file(name, text, 'h4') })
    })
    const { result, outcomes } = open({ name: 'lead' })
    await waitFor(() => expect(result.current.state.loaded).toBe(true))
    act(() => result.current.setDraft(`${TEXT}more\n`))
    await act(async () => void (await result.current.save()))
    expect(result.current.state.external?.file?.text).toBe(onDisk.text)
    expect(result.current.state.draft).toBe(`${TEXT}more\n`)

    await act(async () => void (await result.current.saveMine()))
    expect(bridge.invoked('agents:write')[1]).toMatchObject({ expectedHash: 'h3', renameFrom: null })
    expect(result.current.state.external).toBeNull()
    expect(outcomes.map((o) => o.kind)).toEqual(['savedOver'])
  })

  it('renames by the name field, and deletes with the hash it read', async () => {
    bridge.answer('agents:read', ({ name }) => file(name, TEXT, 'h1'))
    bridge.answer('agents:write', ({ name, text }) => change({ file: file(name, text, 'h2') }))
    bridge.answer('agents:delete', () => change({}))
    const { result, outcomes } = open({ name: 'triage' })
    await waitFor(() => expect(result.current.state.loaded).toBe(true))
    act(() => result.current.setName('issue-triage'))
    expect(result.current.dirty).toBe(true)
    await act(async () => void (await result.current.save()))
    expect(bridge.invoked('agents:write')[0]).toMatchObject({ name: 'issue-triage', expectedHash: 'h1', renameFrom: 'triage' })

    await act(async () => void (await result.current.remove()))
    expect(bridge.invoked('agents:delete')).toEqual([{ name: 'issue-triage', expectedHash: 'h2' }])
    expect(outcomes).toEqual([
      { kind: 'renamed', from: 'triage', name: 'issue-triage' },
      { kind: 'deleted', name: 'issue-triage' }
    ])
  })

  it('starts a copy from the agent it copies and creates it with no hash', async () => {
    bridge.answer('agents:read', ({ name }) => file(name, TEXT))
    bridge.answer('agents:write', ({ name, text }) => change({ file: file(name, text) }))
    const { result, outcomes } = open({ copyOf: 'worker', name: 'worker-2' })
    await waitFor(() => expect(result.current.state.draft).toBe(TEXT))
    expect(result.current.state.base).toBeNull()
    await act(async () => void (await result.current.save()))
    expect(bridge.invoked('agents:write')[0]).toMatchObject({ name: 'worker-2', expectedHash: null, renameFrom: null })
    expect(outcomes).toEqual([{ kind: 'created', name: 'worker-2' }])
  })

  it('takes an outside change quietly with no edit, and holds it beside one, with Reload and its Undo', async () => {
    let onDisk = file('lead', TEXT, 'h1')
    bridge.answer('agents:read', () => onDisk)
    const { result, outcomes } = open({ name: 'lead' })
    await waitFor(() => expect(result.current.state.loaded).toBe(true))

    onDisk = file('lead', `${TEXT}one\n`, 'h2')
    act(() => bridge.emit('agents:changed', { names: ['lead'] }))
    await waitFor(() => expect(result.current.state.draft).toBe(onDisk.text))
    expect(result.current.state.external).toBeNull()

    act(() => result.current.setDraft(`${TEXT}mine\n`))
    onDisk = file('lead', `${TEXT}two\n`, 'h3')
    act(() => bridge.emit('agents:changed', { names: ['lead', 'worker'] }))
    await waitFor(() => expect(result.current.state.external?.file?.hash).toBe('h3'))
    expect(result.current.state.draft).toBe(`${TEXT}mine\n`)

    act(() => void result.current.reloadFromDisk())
    expect(result.current.state.draft).toBe(onDisk.text)
    const reloaded = outcomes.find((o) => o.kind === 'reloaded')
    act(() => reloaded?.kind === 'reloaded' && reloaded.undo())
    expect(result.current.state.draft).toBe(`${TEXT}mine\n`)
  })

  it('reads and writes on the host it was opened on', async () => {
    const remote = remoteBridge('buildbox')
    remote.answer('agents:read', ({ name }) => file(name, TEXT, 'r1'))
    remote.answer('agents:write', ({ name, text }) => change({ file: file(name, text, 'r2') }))
    const { result } = open({ name: 'worker' }, 'buildbox')
    await waitFor(() => expect(result.current.state.loaded).toBe(true))
    act(() => result.current.setDraft(`${TEXT}x\n`))
    await act(async () => void (await result.current.save()))
    expect(remote.invoked('agents:write')).toHaveLength(1)
    expect(bridge.invoked('agents:read')).toEqual([])
    expect(bridge.invoked('agents:write')).toEqual([])
  })

  it('keeps a page’s edit when it leaves the front and comes back, until its tab is closed', async () => {
    bridge.answer('agents:read', ({ name }) => file(name, TEXT, 'h1'))
    const first = renderHook(() => useAgentDraft(LOCAL_HOST_KEY, 'kept-tab', { name: 'lead' }, () => undefined))
    await waitFor(() => expect(first.result.current.state.loaded).toBe(true))
    act(() => first.result.current.setDraft(`${TEXT}kept\n`))
    first.unmount()
    const again = renderHook(() => useAgentDraft(LOCAL_HOST_KEY, 'kept-tab', { name: 'lead' }, () => undefined))
    expect(again.result.current.state.draft).toBe(`${TEXT}kept\n`)
    again.unmount()
    forgetAgentDraft('kept-tab')
    const fresh = renderHook(() => useAgentDraft(LOCAL_HOST_KEY, 'kept-tab', { name: 'lead' }, () => undefined))
    await waitFor(() => expect(fresh.result.current.state.draft).toBe(TEXT))
  })
})
