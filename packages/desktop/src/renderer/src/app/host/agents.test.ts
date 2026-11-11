import { describe, expect, it } from 'vitest'
import { FakeConnection, reconnectingReach } from '../../../../../test/fake-connection'
import type * as P from '../../../../shared/host/protocol'
import type { RequestChannel, RequestPayload, RequestResult } from '../../../../shared/ipc'
import { createAgentsAdapter } from './agents'
import type { HostAdapter } from './router'

const file = (name: string, text: string): P.AgentFile => ({
  name,
  file: `${name}.md`,
  inFolder: true,
  text,
  hash: `hash-of-${text.length}`,
  values: [],
  fields: null,
  body: '',
  problems: [
    { field: 'effort', line: 4, message: 'effort for claude-code must be low', backend: 'claude-code' },
    { field: 'worktree', line: 5, message: 'worktree must be yes or no' }
  ],
  unusedLines: []
})

const change = (patch: Partial<P.AgentChange> = {}): P.AgentChange => ({
  changed: true,
  file: null,
  refusal: null,
  problems: [],
  conflict: null,
  ...patch
})

function call<C extends RequestChannel>(adapter: HostAdapter, channel: C, payload: RequestPayload<C>): Promise<RequestResult<C>> {
  const route = adapter.requests?.[channel] as unknown as (p: RequestPayload<C>) => Promise<RequestResult<C>>
  return route(payload)
}

describe('the agents adapter', () => {
  it('reads, writes, deletes, restores and adds on the host it reaches', async () => {
    const connection = new FakeConnection()
      .answer('agents:read', ({ name }) => file(name, '---\n---\n'))
      .answer('agents:write', () =>
        change({ changed: false, refusal: 'It changed.', conflict: { onDiskHash: 'h2', onDiskText: 'now' } })
      )
      .answer('agents:delete', () => change())
      .answer('agents:restore', ({ name }) => change({ file: file(name, 'back') }))
      .answer('agents:addShipped', () => change())
    const adapter = createAgentsAdapter(reconnectingReach(connection))

    const read = await call(adapter, 'agents:read', { name: 'lead' })
    expect(read).toEqual({
      name: 'lead',
      file: 'lead.md',
      text: '---\n---\n',
      hash: 'hash-of-8',
      problems: [
        { field: 'effort', line: 4, message: 'effort for claude-code must be low', backend: 'claude-code' },
        { field: 'worktree', line: 5, message: 'worktree must be yes or no' }
      ]
    })
    expect(read.problems[1]).not.toHaveProperty('backend')
    const written = await call(adapter, 'agents:write', {
      name: 'issue-triage',
      text: 'x',
      expectedHash: 'h1',
      renameFrom: 'triage'
    })
    expect(written.conflict).toEqual({ onDiskHash: 'h2', onDiskText: 'now' })
    expect(connection.payloads('agents:write')).toEqual([
      { name: 'issue-triage', text: 'x', expectedHash: 'h1', renameFrom: 'triage' }
    ])
    await call(adapter, 'agents:delete', { name: 'triage', expectedHash: 'h1' })
    expect(connection.payloads('agents:delete')).toEqual([{ name: 'triage', expectedHash: 'h1' }])
    expect((await call(adapter, 'agents:restore', { name: 'triage' })).file?.text).toBe('back')
    await call(adapter, 'agents:addShipped', { name: 'reviewer' })
    expect(connection.payloads('agents:addShipped')).toEqual([{ name: 'reviewer' }])
  })

  it('renders instructions on the host it reaches, as the host renders them', async () => {
    const rendered = { html: '<h1>Review</h1>', words: 1 } as unknown as P.RenderedMarkdown
    const connection = new FakeConnection().answer('agents:render', () => rendered)
    const adapter = createAgentsAdapter(reconnectingReach(connection))

    expect(await call(adapter, 'agents:render', { text: '# Review' })).toBe(rendered)
    expect(connection.payloads('agents:render')).toEqual([{ text: '# Review' }])
  })

  it('subscribes to agents:changed only while the window listens, and again on a new connection', async () => {
    const first = new FakeConnection()
    const reach = reconnectingReach(first)
    const adapter = createAgentsAdapter(reach)
    await reach.connection()
    expect(first.subscriptions.has('agents:changed')).toBe(false)

    const heard: string[][] = []
    const off = adapter.events!['agents:changed']!((change) => heard.push(change.names))
    expect(first.subscriptions.has('agents:changed')).toBe(true)
    first.push('agents:changed', { names: ['lead'] })
    expect(heard).toEqual([['lead']])

    const second = new FakeConnection()
    await reach.reconnect(second)
    second.push('agents:changed', { names: ['worker'] })
    expect(heard).toEqual([['lead'], ['worker']])

    off()
    expect(second.subscriptions.has('agents:changed')).toBe(false)
  })
})
