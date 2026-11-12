import { describe, expect, it } from 'vitest'
import { FakeConnection } from '../../test/fake-connection'
import { HostRequestError } from '../shared/host/client'
import type * as P from '../shared/host/protocol'
import { followHostUpdate, RELEASES_PAGE } from './update'

const IDLE: P.UpdateState = {
  running: '3.1.0',
  channel: 'stable',
  unavailable: null,
  newest: '3.2.0',
  checkedAt: '2026-11-11T08:00:00Z',
  failed: null,
  checking: false,
  installing: null,
  installFailed: null
}

const step = (line: string, waitingForApp: boolean): P.UpdateState => ({
  ...IDLE,
  installing: { line, waitingForApp }
})

function follow(host: FakeConnection): { quits: () => number; logged: string[]; stop: () => void } {
  let quits = 0
  const logged: string[] = []
  const stop = followHostUpdate(
    host.asConnection(),
    () => {
      quits += 1
    },
    (line) => logged.push(line)
  )
  return { quits: () => quits, logged, stop }
}

describe('quitting for an update this PC’s host runs', () => {
  it('quits once, when floti update says it is waiting for the app, and not before', () => {
    const host = new FakeConnection()
    const { quits, stop } = follow(host)
    host.snapshot('update:state', IDLE)
    host.push('update:state', step('Starting floti update.', false))
    host.push('update:state', step('Downloading Helm-3.2.0-setup.exe.', false))
    expect(quits()).toBe(0)
    host.push('update:state', step('Waiting for Helm to quit; it is updated once it has.', true))
    expect(quits()).toBe(1)
    // Told again, it asks nothing more of the app.
    host.push('update:state', step('Waiting for Helm to quit; it is updated once it has.', true))
    expect(quits()).toBe(1)
    stop()
    expect(host.subscriptions.has('update:state')).toBe(false)
  })

  it('quits at once on a connection whose host already waits for the app', () => {
    const host = new FakeConnection()
    const { quits } = follow(host)
    host.snapshot('update:state', step('Waiting for Helm to quit; it is updated once it has.', true))
    expect(quits()).toBe(1)
  })

  it('never quits for a check, a failed update or a host that does not update', () => {
    const host = new FakeConnection()
    const { quits } = follow(host)
    host.snapshot('update:state', { ...IDLE, checking: true })
    host.push('update:state', { ...IDLE, installFailed: 'Helm-3.2.0-setup.exe does not match its signature.' })
    host.push('update:state', { ...IDLE, unavailable: 'readOnly', newest: null, checkedAt: null })
    expect(quits()).toBe(0)
  })

  it('says in the log when following ends, and quits for nothing', () => {
    const host = new FakeConnection()
    const { quits, logged } = follow(host)
    host.subscriptions.get('update:state')?.error?.(new HostRequestError('update:state', 'failed', 'The host went away.'))
    expect(logged).toEqual(["update: following this PC's host's updates ended (The host went away.)"])
    expect(quits()).toBe(0)
  })

  it('names the releases page it hands a person, never one it fetches', () => {
    expect(RELEASES_PAGE).toMatch(/^https:\/\/github\.com\/floti-dev\/floti\/releases\/latest$/)
  })
})
