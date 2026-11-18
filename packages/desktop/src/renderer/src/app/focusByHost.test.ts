import { LOCAL_HOST_KEY, sessionRef, type PaneRef } from '@floti/core/types'
import { describe, expect, it } from 'vitest'
import { focusByHost, stillTold } from './focusByHost'

describe('what each host is told is on screen', () => {
  it('names each host’s own sessions in front of a pane, and tells this PC’s host even of none', () => {
    const visible: PaneRef[] = [sessionRef(3), { kind: 'settings' }, sessionRef(3, 'build-box'), sessionRef(9, 'build-box')]
    expect(focusByHost(visible, new Set())).toEqual(
      new Map([
        [LOCAL_HOST_KEY, [3]],
        ['build-box', [3, 9]]
      ])
    )
    expect(focusByHost([], new Set())).toEqual(new Map([[LOCAL_HOST_KEY, []]]))
  })

  it('tells a host told before that nothing of its is on screen now, and then stops telling it', () => {
    const first = focusByHost([sessionRef(4, 'build-box')], new Set([LOCAL_HOST_KEY]))
    const told = stillTold(first)
    expect([...told].sort()).toEqual(['build-box', LOCAL_HOST_KEY].sort())

    const second = focusByHost([sessionRef(1)], told)
    expect(second).toEqual(
      new Map([
        [LOCAL_HOST_KEY, [1]],
        ['build-box', []]
      ])
    )
    expect([...stillTold(second)]).toEqual([LOCAL_HOST_KEY])
  })
})
