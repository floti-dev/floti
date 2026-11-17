import { act, renderHook } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { useWalkthroughHome } from './useWalkthroughHome'

const WORKSPACE = 'C:\\Users\\you\\Projects\\my work'

function mount(empty: boolean) {
  return renderHook((props: { empty: boolean }) => useWalkthroughHome(props.empty), { initialProps: { empty } })
}

describe('the home screen a finished walkthrough leaves', () => {
  it('selects nothing before the walkthrough is over', () => {
    expect(mount(true).result.current.selected).toBeNull()
  })

  it('keeps the workspace it chose selected while no tab is open', () => {
    const { result, rerender } = mount(true)
    act(() => result.current.finish(WORKSPACE))
    expect(result.current.selected).toBe(WORKSPACE)
    rerender({ empty: true })
    expect(result.current.selected).toBe(WORKSPACE)
  })

  it('selects nothing when step 4 chose no workspace', () => {
    const { result } = mount(true)
    act(() => result.current.finish(null))
    expect(result.current.selected).toBeNull()
  })

  it('lets go once a tab opens, and does not select it again when every tab closes', () => {
    const { result, rerender } = mount(true)
    act(() => result.current.finish(WORKSPACE))
    rerender({ empty: false })
    expect(result.current.selected).toBeNull()
    rerender({ empty: true })
    expect(result.current.selected).toBeNull()
  })

  it('selects nothing when it ends with tabs open, even after they close', () => {
    const { result, rerender } = mount(false)
    act(() => result.current.finish(WORKSPACE))
    expect(result.current.selected).toBeNull()
    rerender({ empty: true })
    expect(result.current.selected).toBeNull()
  })
})
