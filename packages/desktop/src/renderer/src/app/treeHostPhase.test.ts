import { describe, expect, it } from 'vitest'
import type { RemoteHost } from '@helm/core/types'
import { treeHostPhase } from './treeHostPhase'

const BUILD_BOX: Pick<RemoteHost, 'name' | 'transport'> = { name: 'build-box', transport: { kind: 'ssh', target: 'build-box' } }

describe('treeHostPhase', () => {
  it('is connecting while nothing has been heard from the host, and while it retries before it was ever live', () => {
    expect(treeHostPhase(BUILD_BOX, null, false)).toEqual({ kind: 'connecting' })
    expect(treeHostPhase(BUILD_BOX, { state: 'retrying' }, false)).toEqual({ kind: 'connecting' })
  })

  it('is loading once connected until the host has listed its sessions, then live', () => {
    expect(treeHostPhase(BUILD_BOX, { state: 'connected', version: '2.5.0' }, false)).toEqual({ kind: 'loading' })
    expect(treeHostPhase(BUILD_BOX, { state: 'connected', version: '2.5.0' }, true)).toEqual({ kind: 'live' })
  })

  it('is reconnecting when the link retries after the host was live', () => {
    expect(treeHostPhase(BUILD_BOX, { state: 'retrying' }, true)).toEqual({ kind: 'reconnecting' })
  })

  it('is unreachable when the link fails, with the reason every surface gives and the line ssh last wrote', () => {
    const lastLine = 'ssh: connect to host build-box port 22: Connection timed out'
    for (const listed of [false, true]) {
      expect(treeHostPhase(BUILD_BOX, { state: 'failed', lastLine }, listed)).toEqual({
        kind: 'unreachable',
        reason: 'Can’t reach build-box',
        lastLine
      })
    }
    expect(treeHostPhase(BUILD_BOX, { state: 'failed', lastLine: 'something nobody has seen' }, false)).toEqual({
      kind: 'unreachable',
      reason: 'Floti could not connect to build-box',
      lastLine: 'something nobody has seen'
    })
  })

  it('is unreachable when the login has no floti, saying so with what its shell said', () => {
    const lastLine = 'bash: line 1: floti: command not found'
    expect(treeHostPhase(BUILD_BOX, { state: 'missing', lastLine }, false)).toEqual({
      kind: 'unreachable',
      reason: 'build-box’s login has no floti to run',
      lastLine
    })
  })

  it('is unreachable when the host proved another key, or its floti refused this one, with no last line', () => {
    expect(treeHostPhase(BUILD_BOX, { state: 'keyChanged' }, true)).toEqual({
      kind: 'unreachable',
      reason: 'build-box proved another key than the one this PC pinned for it'
    })
    expect(treeHostPhase(BUILD_BOX, { state: 'tooOld', host: '3.1', client: '3.2' }, false)).toEqual({
      kind: 'unreachable',
      reason: 'build-box’s floti is too old for this Floti'
    })
    expect(treeHostPhase(BUILD_BOX, { state: 'newer', host: '4.0', client: '3.2' }, false)).toEqual({
      kind: 'unreachable',
      reason: 'build-box’s floti speaks protocol 4.0, newer than this Floti’s 3.2'
    })
  })

  it('is connecting, saying why, while the host waits for somebody to sign in there', () => {
    expect(treeHostPhase(BUILD_BOX, { state: 'startsAtSignIn' }, false)).toEqual({
      kind: 'connecting',
      note: 'Its floti starts when somebody next signs in there.'
    })
  })
})
