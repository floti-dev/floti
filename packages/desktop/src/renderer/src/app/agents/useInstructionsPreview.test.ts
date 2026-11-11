import { act, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { RenderedMarkdown } from '@helm/core/types'
import { LOCAL_HOST_KEY } from '@helm/core/types'
import { bridge, remoteBridge } from '../bridge.testkit'
import { INSTRUCTIONS_PREVIEW_PAUSE_MS, useInstructionsPreview } from './useInstructionsPreview'

vi.mock('../bridge', () => import('../bridge.testkit'))

const rendered = (text: string): RenderedMarkdown => ({ html: `<p>${text}</p>` }) as unknown as RenderedMarkdown

/** Lets the settled invokes' answers reach the hook. */
const settle = (): Promise<void> => act(async () => void (await vi.advanceTimersByTimeAsync(0)))

beforeEach(() => {
  vi.useFakeTimers()
  bridge.reset()
  remoteBridge('buildbox').reset()
})
afterEach(() => vi.useRealTimers())

describe('useInstructionsPreview', () => {
  it('renders the instructions on its host, again once typing pauses, keeping only the newest answer', async () => {
    const answers: Array<(value: RenderedMarkdown) => void> = []
    bridge.answer('agents:render', () => new Promise<RenderedMarkdown>((resolve) => answers.push(resolve)))
    const { result, rerender } = renderHook(({ text }) => useInstructionsPreview(LOCAL_HOST_KEY, text), {
      initialProps: { text: 'You plan.' }
    })
    expect(bridge.invoked('agents:render')).toEqual([{ text: 'You plan.' }])
    expect(result.current).toEqual({ rendered: null, pending: true })

    // Typing asks nothing until it pauses.
    rerender({ text: 'You plan' })
    rerender({ text: 'You plan it.' })
    await act(async () => void (await vi.advanceTimersByTimeAsync(INSTRUCTIONS_PREVIEW_PAUSE_MS - 1)))
    expect(bridge.invoked('agents:render')).toHaveLength(1)
    await act(async () => void (await vi.advanceTimersByTimeAsync(1)))
    expect(bridge.invoked('agents:render')).toEqual([{ text: 'You plan.' }, { text: 'You plan it.' }])

    // The first render answers late: it is not shown over the newer one.
    answers[1]?.(rendered('You plan it.'))
    await settle()
    answers[0]?.(rendered('You plan.'))
    await settle()
    expect(result.current).toEqual({ rendered: rendered('You plan it.'), pending: false })

    // A new edit marks the preview as behind until its render comes.
    rerender({ text: 'You plan it all.' })
    expect(result.current.pending).toBe(true)
    expect(result.current.rendered).toEqual(rendered('You plan it.'))
  })

  it('asks the host the page is on', async () => {
    const there = remoteBridge('buildbox')
    there.answer('agents:render', ({ text }) => rendered(text))
    const { result } = renderHook(() => useInstructionsPreview('buildbox', 'Review it.'))
    await settle()
    expect(there.invoked('agents:render')).toEqual([{ text: 'Review it.' }])
    expect(bridge.invoked('agents:render')).toEqual([])
    expect(result.current).toEqual({ rendered: rendered('Review it.'), pending: false })
  })
})
