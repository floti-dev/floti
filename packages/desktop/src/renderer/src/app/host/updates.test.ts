import { describe, expect, it } from 'vitest'
import { FakeConnection } from '../../../../../test/fake-connection'
import { HostRequestError } from '../../../../shared/host/client'
import type * as P from '../../../../shared/host/protocol'
import { createUpdates, updateView, type UpdateHeard } from './updates'

const STATE: P.UpdateState = {
  running: '3.2.0',
  channel: 'stable',
  unavailable: null,
  newest: '3.2.0',
  checkedAt: '2026-11-11T08:00:00Z',
  failed: null,
  checking: false,
  installing: null,
  installFailed: null
}

const reach = (host: FakeConnection) => () => Promise.resolve(host.asConnection())

async function subscribed(host: FakeConnection): Promise<void> {
  // The connection is reached asynchronously; the subscription follows it.
  for (let turn = 0; turn < 10 && !host.subscriptions.has('update:state'); turn++) await Promise.resolve()
}

describe('Settings > Updates, over this PC’s host', () => {
  it('hears the host’s state whole, first and on every change, and lets it go when stopped', async () => {
    const host = new FakeConnection()
    const heard: UpdateHeard[] = []
    const stop = createUpdates({ log: () => undefined }).watch(reach(host), (update) => heard.push(update))
    await subscribed(host)
    host.snapshot('update:state', STATE)
    host.push('update:state', { ...STATE, checking: true })
    expect(heard).toEqual([
      { ok: true, state: STATE },
      { ok: true, state: { ...STATE, checking: true } }
    ])
    expect(host.sent).toEqual([])
    stop()
    expect(host.subscriptions.has('update:state')).toBe(false)
  })

  it('says why the host cannot be heard, and hears nothing after', async () => {
    const host = new FakeConnection()
    const heard: UpdateHeard[] = []
    createUpdates({ log: () => undefined }).watch(reach(host), (update) => heard.push(update))
    await subscribed(host)
    host.subscriptions.get('update:state')?.error?.(new HostRequestError('update:state', 'failed', 'The host went away.'))
    expect(heard).toEqual([{ ok: false, message: 'The host went away.' }])

    const unreachable: UpdateHeard[] = []
    createUpdates({ log: () => undefined }).watch(
      () => Promise.reject(new Error('No host is running.')),
      (update) => unreachable.push(update)
    )
    await Promise.resolve()
    await Promise.resolve()
    expect(unreachable).toEqual([{ ok: false, message: 'No host is running.' }])
  })

  it('asks the host to check now and answers with the state after the check', async () => {
    const host = new FakeConnection()
    host.answer('update:check', () => ({ ...STATE, newest: '3.3.0' }))
    const result = await createUpdates({ log: () => undefined }).check(reach(host))
    expect(result).toEqual({ ok: true, state: { ...STATE, newest: '3.3.0' } })
    expect(host.sent).toEqual([{ channel: 'update:check', payload: null }])
  })

  it('asks the host to update, and says why when it refuses', async () => {
    const host = new FakeConnection()
    host.answer('update:install', () => null)
    const store = createUpdates({ log: () => undefined })
    expect(await store.install(reach(host))).toEqual({ ok: true })
    expect(host.sent).toEqual([{ channel: 'update:install', payload: null }])

    const refusing = new FakeConnection()
    refusing.answer('update:install', () => {
      throw new HostRequestError(
        'update:install',
        'failed',
        'This host does not run floti installed on its own, so floti update does not update it.'
      )
    })
    expect(await store.install(reach(refusing))).toEqual({
      ok: false,
      message: 'This host does not run floti installed on its own, so floti update does not update it.'
    })
  })
})

describe('what the window draws of the host’s state', () => {
  it('calls the channel’s newest release newer only when it is strictly ahead of this app', () => {
    expect(updateView({ ...STATE, newest: '3.3.0' }, '3.2.0').newer).toBe(true)
    expect(updateView({ ...STATE, newest: '3.2.0' }, '3.2.0').newer).toBe(false)
    expect(updateView({ ...STATE, newest: '3.1.0' }, '3.2.0').newer).toBe(false)
    expect(updateView({ ...STATE, newest: '3.2.0' }, '3.2.0-preview.4').newer).toBe(true)
  })

  it('never offers an update from a version it cannot place, or before it knows its own', () => {
    expect(updateView({ ...STATE, newest: 'latest' }, '3.2.0').newer).toBe(false)
    expect(updateView({ ...STATE, newest: '3.3.0' }, null).newer).toBe(false)
    expect(updateView({ ...STATE, newest: null, checkedAt: null }, '3.2.0').newer).toBe(false)
  })

  it('passes on what the host said of its checks and of an update it runs', () => {
    const view = updateView(
      {
        ...STATE,
        failed: 'could not download update.json.',
        checking: true,
        unavailable: null,
        installing: { line: 'Waiting for Helm to quit; it is updated once it has.', waitingForApp: true },
        installFailed: null
      },
      '3.2.0'
    )
    expect(view).toEqual({
      newest: '3.2.0',
      newer: false,
      checkedAt: '2026-11-11T08:00:00Z',
      failed: 'could not download update.json.',
      checking: true,
      unavailable: null,
      installing: { line: 'Waiting for Helm to quit; it is updated once it has.', waitingForApp: true },
      installFailed: null
    })
  })
})
