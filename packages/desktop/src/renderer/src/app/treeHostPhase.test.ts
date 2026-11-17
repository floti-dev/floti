import { describe, expect, it } from 'vitest'
import type { RemoteHost } from '@floti/core/types'
import { treeHostPhase } from './treeHostPhase'

const BUILD_BOX: Pick<RemoteHost, 'name' | 'transport'> = { name: 'build-box', transport: { kind: 'ssh', target: 'build-box' } }
const NEVER = { listed: false }
const WAS_LIVE = { listed: true }
const TIMED_OUT = 'ssh: connect to host build-box port 22: Connection timed out'

describe('treeHostPhase', () => {
  it('is connecting while nothing has been heard from the host, and while it retries before it was ever live', () => {
    expect(treeHostPhase(BUILD_BOX, null, NEVER)).toEqual({ kind: 'connecting' })
    expect(treeHostPhase(BUILD_BOX, { state: 'retrying' }, NEVER)).toEqual({ kind: 'connecting' })
  })

  it('is loading once connected until the host has listed its sessions, then live', () => {
    expect(treeHostPhase(BUILD_BOX, { state: 'connected', version: '2.5.0' }, NEVER)).toEqual({ kind: 'loading' })
    expect(treeHostPhase(BUILD_BOX, { state: 'connected', version: '2.5.0' }, WAS_LIVE)).toEqual({ kind: 'live' })
  })

  it('is Unknown, never loading for ever, when listing its sessions failed while connected', () => {
    expect(
      treeHostPhase(BUILD_BOX, { state: 'connected' }, { listed: false, listError: 'The request timed out.' })
    ).toEqual({ kind: 'unknown', reason: 'The request timed out.' })
    // Listed before, a later listing that failed leaves it live with the rows it has.
    expect(treeHostPhase(BUILD_BOX, { state: 'connected' }, { listed: true, listError: 'The request timed out.' })).toEqual({
      kind: 'live'
    })
  })

  it('is reconnecting when the link retries after the host was live', () => {
    expect(treeHostPhase(BUILD_BOX, { state: 'retrying' }, WAS_LIVE)).toEqual({ kind: 'reconnecting' })
  })

  it('stays reconnecting after it was live while its retries fail, saying why, with the line ssh last wrote', () => {
    expect(treeHostPhase(BUILD_BOX, { state: 'failed', lastLine: TIMED_OUT }, WAS_LIVE)).toEqual({
      kind: 'reconnecting',
      failure: { reason: 'Can’t reach build-box', lastLine: TIMED_OUT, retrying: true }
    })
    const notFound = 'bash: line 1: floti: command not found'
    expect(treeHostPhase(BUILD_BOX, { state: 'missing', lastLine: notFound }, WAS_LIVE)).toEqual({
      kind: 'reconnecting',
      failure: { reason: 'build-box’s login has no floti to run', lastLine: notFound, retrying: true }
    })
  })

  it('is unreachable when the link fails before the host was ever live, and Floti keeps trying', () => {
    expect(treeHostPhase(BUILD_BOX, { state: 'failed', lastLine: TIMED_OUT }, NEVER)).toEqual({
      kind: 'unreachable',
      failure: { reason: 'Can’t reach build-box', lastLine: TIMED_OUT, retrying: true }
    })
    expect(treeHostPhase(BUILD_BOX, { state: 'failed', lastLine: 'something nobody has seen' }, NEVER)).toEqual({
      kind: 'unreachable',
      failure: { reason: 'Floti could not connect to build-box', lastLine: 'something nobody has seen', retrying: true }
    })
    const notFound = 'bash: line 1: floti: command not found'
    expect(treeHostPhase(BUILD_BOX, { state: 'missing', lastLine: notFound }, NEVER)).toEqual({
      kind: 'unreachable',
      failure: { reason: 'build-box’s login has no floti to run', lastLine: notFound, retrying: true }
    })
  })

  it('is unreachable, with nothing to retry, when the host proved another key or its floti refused this one, live before or not', () => {
    for (const listing of [NEVER, WAS_LIVE]) {
      expect(treeHostPhase(BUILD_BOX, { state: 'keyChanged' }, listing)).toEqual({
        kind: 'unreachable',
        failure: { reason: 'build-box proved another key than the one this PC pinned for it', retrying: false }
      })
      expect(treeHostPhase(BUILD_BOX, { state: 'tooOld', host: '3.1', client: '3.2' }, listing)).toEqual({
        kind: 'unreachable',
        failure: { reason: 'build-box’s floti is too old for this Floti', retrying: false }
      })
      expect(treeHostPhase(BUILD_BOX, { state: 'newer', host: '4.0', client: '3.2' }, listing)).toEqual({
        kind: 'unreachable',
        failure: { reason: 'build-box’s floti speaks protocol 4.0, newer than this Floti’s 3.2', retrying: false }
      })
    }
  })

  it('is connecting, saying why, while the host waits for somebody to sign in there', () => {
    expect(treeHostPhase(BUILD_BOX, { state: 'startsAtSignIn' }, NEVER)).toEqual({
      kind: 'connecting',
      note: 'Its floti starts when somebody next signs in there.'
    })
  })
})
