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

  it('keeps the page and says why when the file cannot be read again and the host cannot say it is gone', async () => {
    const remote = remoteBridge('buildbox')
    let reachable = true
    remote.answer('agents:read', ({ name }) => {
      if (!reachable) throw new Error('The connection to build-box ended.')
      return file(name, TEXT, 'r1')
    })
    remote.answer('agents:list', () => {
      throw new Error('The connection to build-box ended.')
    })
    const { result, outcomes } = open({ name: 'worker' }, 'buildbox')
    await waitFor(() => expect(result.current.state.loaded).toBe(true))

    reachable = false
    act(() => remote.emit('agents:changed', { names: ['worker'] }))
    await waitFor(() => expect(result.current.state.loadError).toBe('The connection to build-box ended.'))
    expect(outcomes).toEqual([])
    expect(result.current.state.base?.hash).toBe('r1')

    // Read again once the host is back: the error goes.
    reachable = true
    act(() => remote.emit('agents:changed', { names: ['worker'] }))
    await waitFor(() => expect(result.current.state.loadError).toBeNull())
    expect(outcomes).toEqual([])
  })

  it('is gone when the file cannot be read and the host’s list no longer has it', async () => {
    let there = true
    bridge.answer('agents:read', ({ name }) => {
      if (!there) throw new Error(`There is no agent file ${name}.md in the agents folder.`)
      return file(name, TEXT, 'h1')
    })
    bridge.answer('agents:list', () => ({ agents: [{ name: 'worker' }], bad: [{ name: 'lead-2' }] }) as never)
    const { result, outcomes } = open({ name: 'lead' })
    await waitFor(() => expect(result.current.state.loaded).toBe(true))
    there = false
    act(() => bridge.emit('agents:changed', { names: ['lead'] }))
    await waitFor(() => expect(outcomes).toEqual([{ kind: 'gone', name: 'lead' }]))
  })

  it('renames the file as it is now when Save mine follows a rename and an outside change', async () => {
    bridge.answer('agents:read', ({ name }) => file(name, TEXT, 'h1'))
    const { result, outcomes } = open({ name: 'triage' })
    await waitFor(() => expect(result.current.state.loaded).toBe(true))
    act(() => result.current.setName('issue-triage'))
    bridge.answer('agents:read', ({ name }) => file(name, `${TEXT}outside\n`, 'h2'))
    act(() => bridge.emit('agents:changed', { names: ['triage'] }))
    await waitFor(() => expect(result.current.state.external?.file?.hash).toBe('h2'))

    bridge.answer('agents:write', ({ name, text }) => change({ file: file(name, text, 'h3') }))
    await act(async () => void (await result.current.saveMine()))
    expect(bridge.invoked('agents:write')).toEqual([
      { name: 'issue-triage', text: withoutClass(TEXT), expectedHash: 'h2', renameFrom: 'triage' }
    ])
    expect(outcomes).toEqual([{ kind: 'savedOver', name: 'issue-triage', at: expect.any(String), from: 'triage', moved: true }])
  })

  it('makes the agent under its new name on Save mine after a rename and the old file’s removal, and names the old name', async () => {
    bridge.answer('agents:read', ({ name }) => file(name, TEXT, 'h1'))
    const { result, outcomes } = open({ name: 'foo' })
    await waitFor(() => expect(result.current.state.loaded).toBe(true))
    act(() => result.current.setName('bar'))
    bridge.answer('agents:read', () => {
      throw new Error('There is no agent file foo.md in the agents folder.')
    })
    bridge.answer('agents:list', () => ({ agents: [], bad: [] }) as never)
    act(() => bridge.emit('agents:changed', { names: ['foo'] }))
    await waitFor(() => expect(result.current.state.external).toMatchObject({ file: null }))

    bridge.answer('agents:write', ({ name, text }) => change({ file: file(name, text, 'h2') }))
    await act(async () => void (await result.current.saveMine()))
    expect(bridge.invoked('agents:write')).toEqual([
      { name: 'bar', text: withoutClass(TEXT), expectedHash: null, renameFrom: null }
    ])
    expect(outcomes).toEqual([{ kind: 'savedOver', name: 'bar', at: expect.any(String), from: 'foo', moved: false }])
  })

  it('clears a failed read again once the change from outside it holds reads again', async () => {
    let reachable = true
    let onDisk = file('lead', TEXT, 'h1')
    bridge.answer('agents:read', () => {
      if (!reachable) throw new Error('The connection ended.')
      return onDisk
    })
    bridge.answer('agents:list', () => {
      throw new Error('The connection ended.')
    })
    const { result } = open({ name: 'lead' })
    await waitFor(() => expect(result.current.state.loaded).toBe(true))
    act(() => result.current.setDraft(`${TEXT}mine
`))
    onDisk = file('lead', `${TEXT}outside
`, 'h2')
    act(() => bridge.emit('agents:changed', { names: ['lead'] }))
    await waitFor(() => expect(result.current.state.external?.file?.hash).toBe('h2'))

    reachable = false
    act(() => bridge.emit('agents:changed', { names: ['lead'] }))
    await waitFor(() => expect(result.current.state.loadError).toBe('The connection ended.'))

    reachable = true
    act(() => bridge.emit('agents:changed', { names: ['lead'] }))
    await waitFor(() => expect(result.current.state.loadError).toBeNull())
    expect(result.current.state.external?.file?.hash).toBe('h2')
  })

  it('keeps what was typed while a save was on its way', async () => {
    bridge.answer('agents:read', ({ name }) => file(name, TEXT, 'h1'))
    let answer: ((value: AgentChangeResult) => void) | null = null
    bridge.answer('agents:write', () => new Promise<AgentChangeResult>((resolve) => (answer = resolve)))
    const { result } = open({ name: 'lead' })
    await waitFor(() => expect(result.current.state.loaded).toBe(true))
    act(() => result.current.setDraft(`${TEXT}one\n`))
    let saving: Promise<unknown> = Promise.resolve()
    act(() => {
      saving = result.current.save()
    })
    act(() => result.current.setDraft(`${TEXT}one\ntwo\n`))
    await act(async () => {
      answer?.(change({ file: file('lead', withoutClass(`${TEXT}one\n`), 'h2') }))
      await saving
    })
    expect(result.current.state.base?.hash).toBe('h2')
    expect(result.current.state.draft).toBe(`${TEXT}one\ntwo\n`)
    expect(result.current.dirty).toBe(true)
  })

  it('names a copy after every name its host’s folder holds, a bad file’s included', async () => {
    bridge.answer('agents:read', ({ name }) => file(name, TEXT))
    bridge.answer('agents:list', () => ({ agents: [{ name: 'worker' }], bad: [{ name: 'worker-2' }] }) as never)
    const { result } = open({ copyOf: 'worker', name: '' })
    await waitFor(() => expect(result.current.state.loaded).toBe(true))
    expect(result.current.state.name).toBe('worker-3')
    expect(result.current.state.draft).toBe(TEXT)
  })

  it('keeps nothing for a closed tab when an Undo or an answer arrives after it closed', async () => {
    bridge.answer('agents:read', ({ name }) => file(name, TEXT, 'h1'))
    const outcomes: AgentOutcome[] = []
    const page = renderHook(() => useAgentDraft(LOCAL_HOST_KEY, 'closed-tab', { name: 'lead' }, (o) => outcomes.push(o)))
    await waitFor(() => expect(page.result.current.state.loaded).toBe(true))
    act(() => page.result.current.setDraft(`${TEXT}mine\n`))
    bridge.answer('agents:read', ({ name }) => file(name, `${TEXT}outside\n`, 'h2'))
    act(() => bridge.emit('agents:changed', { names: ['lead'] }))
    await waitFor(() => expect(page.result.current.state.external).not.toBeNull())
    act(() => void page.result.current.reloadFromDisk())

    // The tab closes; its reload toast's Undo is pressed after.
    page.unmount()
    forgetAgentDraft('closed-tab')
    const reloaded = outcomes.find((o) => o.kind === 'reloaded')
    act(() => reloaded?.kind === 'reloaded' && reloaded.undo())

    const reopened = renderHook(() => useAgentDraft(LOCAL_HOST_KEY, 'closed-tab', { name: 'lead' }, () => undefined))
    expect(reopened.result.current.state.loaded).toBe(false)
    await waitFor(() => expect(reopened.result.current.state.draft).toBe(`${TEXT}outside\n`))
    // And the page opened again keeps its own edit as before.
    act(() => reopened.result.current.setDraft(`${TEXT}again\n`))
    reopened.unmount()
    const third = renderHook(() => useAgentDraft(LOCAL_HOST_KEY, 'closed-tab', { name: 'lead' }, () => undefined))
    expect(third.result.current.state.draft).toBe(`${TEXT}again\n`)
  })

  it('takes the file as it is now on Revert while a change from outside is held, and is gone when it was removed', async () => {
    bridge.answer('agents:read', ({ name }) => file(name, TEXT, 'h1'))
    const { result, outcomes } = open({ name: 'lead' })
    await waitFor(() => expect(result.current.state.loaded).toBe(true))
    act(() => result.current.setDraft(`${TEXT}mine\n`))
    bridge.answer('agents:read', ({ name }) => file(name, `${TEXT}outside\n`, 'h2'))
    act(() => bridge.emit('agents:changed', { names: ['lead'] }))
    await waitFor(() => expect(result.current.state.external).not.toBeNull())
    act(() => result.current.revert())
    expect(result.current.state.external).toBeNull()
    expect(result.current.state.base?.hash).toBe('h2')
    expect(result.current.state.draft).toBe(`${TEXT}outside\n`)
    expect(result.current.dirty).toBe(false)

    act(() => result.current.setDraft(`${TEXT}mine\n`))
    bridge.answer('agents:read', () => {
      throw new Error('There is no agent file lead.md in the agents folder.')
    })
    bridge.answer('agents:list', () => ({ agents: [], bad: [] }) as never)
    act(() => bridge.emit('agents:changed', { names: ['lead'] }))
    await waitFor(() => expect(result.current.state.external).toMatchObject({ file: null }))
    act(() => result.current.revert())
    expect(outcomes).toEqual([{ kind: 'gone', name: 'lead' }])
  })
})

/** The text a save sends: the draft without its retired `class` line. */
function withoutClass(text: string): string {
  return text.replace('class: light\n', '')
}
