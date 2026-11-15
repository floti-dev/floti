import { act, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { bridge } from './bridge.testkit'
import { useBrowserPointerCommands } from './useBrowserPointerCommands'

vi.mock('./bridge', () => import('./bridge.testkit'))

/** The Browser tab, with a box jsdom does not lay out. */
let tab: HTMLElement

beforeEach(() => {
  bridge.reset()
  tab = document.createElement('div')
  tab.setAttribute('data-browser-tab', '')
  tab.getBoundingClientRect = () => ({ left: 300, top: 100, right: 1000, bottom: 700, x: 300, y: 100, width: 700, height: 600, toJSON: () => ({}) })
  document.body.append(tab)
})

afterEach(() => tab.remove())

describe('the mouse back and forward buttons', () => {
  it('act on the page in front when the pointer is over the Browser tab, wherever the focus is', () => {
    const run = vi.fn()
    // `shown` is the tab in front of a visible pane, focused or not: a session may hold the focus.
    renderHook(() => useBrowserPointerCommands(true, 7, run))
    act(() => bridge.emit('browser:pointerCommand', { command: 'back', at: { x: 600, y: 400 } }))
    act(() => bridge.emit('browser:pointerCommand', { command: 'forward', at: { x: 300, y: 100 } }))
    expect(run.mock.calls).toEqual([
      [7, 'back'],
      [7, 'forward']
    ])
  })

  it('do nothing with the pointer outside the Browser tab', () => {
    const run = vi.fn()
    renderHook(() => useBrowserPointerCommands(true, 7, run))
    for (const at of [{ x: 100, y: 400 }, { x: 1000, y: 400 }, { x: 600, y: 99 }, { x: 600, y: 700 }]) {
      act(() => bridge.emit('browser:pointerCommand', { command: 'back', at }))
    }
    expect(run).not.toHaveBeenCalled()
  })

  it('do nothing while the Browser tab is in front of no visible pane, or has no page in front', () => {
    const run = vi.fn()
    const hidden = renderHook(() => useBrowserPointerCommands(false, 7, run))
    act(() => bridge.emit('browser:pointerCommand', { command: 'back', at: { x: 600, y: 400 } }))
    hidden.unmount()
    renderHook(() => useBrowserPointerCommands(true, null, run))
    act(() => bridge.emit('browser:pointerCommand', { command: 'back', at: { x: 600, y: 400 } }))
    expect(run).not.toHaveBeenCalled()
  })
})
