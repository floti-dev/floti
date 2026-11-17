import { renderHook } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { useWalkthroughReveal } from './useWalkthroughReveal'

function mount(initial: { workspace: string | null; shown: boolean }) {
  const revealed: string[] = []
  const view = renderHook(
    (props: { workspace: string | null; shown: boolean }) =>
      useWalkthroughReveal(props.workspace, props.shown, (path) => revealed.push(path)),
    { initialProps: initial }
  )
  return { revealed, ...view }
}

describe('revealing the workspace the walkthrough chose', () => {
  it('reveals it once as the shell opens on it, not on later renders', () => {
    const { revealed, rerender } = mount({ workspace: '/work/my work', shown: true })
    expect(revealed).toEqual(['/work/my work'])
    rerender({ workspace: '/work/my work', shown: true })
    expect(revealed).toEqual(['/work/my work'])
  })

  it('reveals nothing before a workspace is chosen or while the walkthrough is not in the shell', () => {
    const { revealed, rerender } = mount({ workspace: null, shown: true })
    rerender({ workspace: '/work/a', shown: false })
    expect(revealed).toEqual([])
  })

  it('reveals again when another workspace is chosen, and when the shell is reached after the choice', () => {
    const { revealed, rerender } = mount({ workspace: '/work/a', shown: false })
    rerender({ workspace: '/work/a', shown: true })
    rerender({ workspace: '/work/b', shown: true })
    expect(revealed).toEqual(['/work/a', '/work/b'])
  })
})
