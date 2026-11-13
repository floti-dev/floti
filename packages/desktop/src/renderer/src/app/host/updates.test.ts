import { describe, expect, it } from 'vitest'
import { FakeConnection, fakeReach, reconnectingReach } from '../../../../../test/fake-connection'
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

const settle = async (): Promise<void> => {
  for (let turn = 0; turn < 10; turn++) await Promise.resolve()
}

describe('Settings > Updates, over this PC’s host', () => {
  it('hears the host’s state whole, first and on every change, and hands the latest to a late listener', async () => {
    const host = new FakeConnection()
    const heard: UpdateHeard[] = []
    const updates = createUpdates(fakeReach(host), { log: () => undefined })
    const stop = updates.watch((update) => heard.push(update))
    await settle()
    host.snapshot('update:state', STATE)
    host.push('update:state', { ...STATE, checking: true })
    expect(heard).toEqual([
      { ok: true, state: STATE },
      { ok: true, state: { ...STATE, checking: true } }
    ])
    expect(host.sent).toEqual([])
    stop()
    const late: UpdateHeard[] = []
    updates.watch((update) => late.push(update))
    expect(late).toEqual([{ ok: true, state: { ...STATE, checking: true } }])
  })

  it('follows the host across a relaunch: the end is said, and the next connection’s state clears it', async () => {
    const first = new FakeConnection()
    const reach = reconnectingReach(first)
    const heard: UpdateHeard[] = []
    createUpdates(reach, { log: () => undefined }).watch((update) => heard.push(update))
    await settle()
    first.snapshot('update:state', { ...STATE, running: '3.1.0', newest: '3.2.0' })
    // The host relaunches on the floti its check put in place.
    first.subscriptions.get('update:state')?.error?.(new HostRequestError('update:state', 'failed', 'The host restarted.'))
    expect(heard.at(-1)).toEqual({ ok: false, message: 'The host restarted.' })

    const next = new FakeConnection()
    await reach.reconnect(next)
    expect(next.subscriptions.has('update:state')).toBe(true)
    next.snapshot('update:state', STATE)
    expect(heard.at(-1)).toEqual({ ok: true, state: STATE })
    // Later checks on the new host reach the window.
    next.push('update:state', { ...STATE, newest: '3.3.0' })
    expect(heard.at(-1)).toEqual({ ok: true, state: { ...STATE, newest: '3.3.0' } })
  })

  it('says why the host cannot be reached', async () => {
    const heard: UpdateHeard[] = []
    createUpdates(fakeReach(null, 'No host is running.'), { log: () => undefined }).watch((update) =>
      heard.push(update)
    )
    await settle()
    expect(heard).toEqual([{ ok: false, message: 'No host is running.' }])
  })

  it('asks the host to check now and answers with the state after the check', async () => {
    const host = new FakeConnection()
    host.answer('update:check', () => ({ ...STATE, newest: '3.3.0' }))
    const result = await createUpdates(fakeReach(host), { log: () => undefined }).check()
    expect(result).toEqual({ ok: true, state: { ...STATE, newest: '3.3.0' } })
    expect(host.sent).toEqual([{ channel: 'update:check', payload: null }])
  })

  it('asks the host to update, and says why when it refuses', async () => {
    const host = new FakeConnection()
    host.answer('update:install', () => null)
    expect(await createUpdates(fakeReach(host), { log: () => undefined }).install()).toEqual({ ok: true })
    expect(host.sent).toEqual([{ channel: 'update:install', payload: null }])

    const refusing = new FakeConnection()
    refusing.answer('update:install', () => {
      throw new HostRequestError(
        'update:install',
        'failed',
        'This host does not run floti installed on its own, so floti update does not update it.'
      )
    })
    expect(await createUpdates(fakeReach(refusing), { log: () => undefined }).install()).toEqual({
      ok: false,
      message: 'This host does not run floti installed on its own, so floti update does not update it.'
    })
  })
})

describe('what the window draws of the host’s state', () => {
  it('calls a release newer only when it is strictly ahead of this app', () => {
    expect(updateView({ ...STATE, newest: '3.3.0' }, '3.2.0').newer).toBe(true)
    expect(updateView({ ...STATE, newest: '3.2.0' }, '3.2.0').newer).toBe(false)
    expect(updateView({ ...STATE, running: '3.1.0', newest: '3.1.0' }, '3.2.0').newer).toBe(false)
    expect(updateView({ ...STATE, newest: '3.2.0' }, '3.2.0-preview.4').newer).toBe(true)
  })

  it('counts the host’s own floti as a release, with no check kept or one behind it', () => {
    const noCheck = updateView({ ...STATE, running: '3.3.0', newest: null, checkedAt: null }, '3.2.0')
    expect(noCheck).toMatchObject({ newest: '3.3.0', newer: true })
    const behind = updateView({ ...STATE, running: '3.3.0', newest: '3.2.0' }, '3.2.0')
    expect(behind).toMatchObject({ newest: '3.3.0', newer: true })
  })

  it('never offers an update from a version it cannot place, or before it knows its own', () => {
    expect(updateView({ ...STATE, newest: 'latest' }, '3.2.0').newer).toBe(false)
    expect(updateView({ ...STATE, newest: '3.3.0' }, null).newer).toBe(false)
  })

  it('passes on what the host said of its checks and of an update it runs', () => {
    const view = updateView(
      {
        ...STATE,
        failed: 'could not download update.json.',
        checking: true,
        installing: { line: 'Waiting for Floti to quit; it is updated once it has.', waitingForApp: true }
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
      installing: { line: 'Waiting for Floti to quit; it is updated once it has.', waitingForApp: true },
      installFailed: null
    })
  })
})
