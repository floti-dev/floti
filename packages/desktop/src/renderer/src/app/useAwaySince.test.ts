import { renderHook } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { useAwaySince } from './useAwaySince'

describe('useAwaySince', () => {
  it('times each host from when the window saw it go out of reach, keeps that time while it stays out, and times a later drop afresh', () => {
    let now = 1_000
    const clock = (): number => now
    const { result, rerender } = renderHook(({ away }) => useAwaySince(away, clock), {
      initialProps: { away: [] as string[] }
    })
    expect([...result.current]).toEqual([])

    now = 2_000
    rerender({ away: ['h1'] })
    expect(result.current.get('h1')).toBe(2_000)

    // Another host drops later: the first keeps its own time.
    now = 3_000
    rerender({ away: ['h2', 'h1'] })
    expect(result.current.get('h1')).toBe(2_000)
    expect(result.current.get('h2')).toBe(3_000)

    // Back in reach, then out again: timed from the new drop.
    now = 4_000
    rerender({ away: ['h2'] })
    expect(result.current.has('h1')).toBe(false)
    now = 5_000
    rerender({ away: ['h1', 'h2'] })
    expect(result.current.get('h1')).toBe(5_000)
    expect(result.current.get('h2')).toBe(3_000)
  })

  it('times a host out of reach from the window’s start from its first render', () => {
    const { result } = renderHook(() => useAwaySince(['h1'], () => 7_000))
    expect(result.current.get('h1')).toBe(7_000)
  })
})
