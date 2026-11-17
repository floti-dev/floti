import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { AppSettings, Walkthrough } from '@floti/core/types'
import { bridge } from '../bridge.testkit'
import { ShellWalkthrough } from './ShellWalkthrough'

vi.mock('../bridge', () => import('../bridge.testkit'))

/** Step 5 as the shell draws it in its main area: the last step. */

const WORKSPACE = 'C:\\Users\\you\\Projects\\my-work'

function renderAt(walkthrough: Walkthrough) {
  const writes: Array<Partial<AppSettings>> = []
  const onDone = vi.fn()
  const view = render(
    <ShellWalkthrough walkthrough={walkthrough} bridge={bridge} write={(patch) => writes.push(patch)} onDone={onDone} />
  )
  return { ...view, writes, onDone }
}

beforeEach(() => {
  bridge.reset()
  bridge.answer('walkthrough:open', () => undefined)
})

describe('the walkthrough in the shell', () => {
  it('tours the window at step 5, the last of five', () => {
    renderAt({ step: 5, workspace: WORKSPACE })
    expect(screen.getByRole('heading', { name: 'Around the window' })).toBeTruthy()
    expect(screen.getByText('Step 5 of 5')).toBeTruthy()
  })

  it.each(['Continue', 'Skip'])('marks the walkthrough done on %s, keeping the workspace it chose', async (name) => {
    const { writes, onDone } = renderAt({ step: 5, workspace: WORKSPACE })
    await userEvent.click(screen.getByRole('button', { name }))
    expect(writes).toEqual([{ walkthrough: null }])
    expect(onDone).toHaveBeenCalledWith(WORKSPACE)
    expect(bridge.invoked('walkthrough:open')).toEqual([])
  })

  it('finishes with no workspace when step 4 chose none', async () => {
    const { writes, onDone } = renderAt({ step: 5, workspace: null })
    await userEvent.click(screen.getByRole('button', { name: 'Continue' }))
    expect(writes).toEqual([{ walkthrough: null }])
    expect(onDone).toHaveBeenCalledWith(null)
  })

  it('goes back from step 5 to step 4 in the setup window, which main opens again', async () => {
    const { writes, onDone } = renderAt({ step: 5, workspace: WORKSPACE })
    await userEvent.click(screen.getByRole('button', { name: 'Back' }))
    expect(bridge.invoked('walkthrough:open')).toEqual([{ step: 4 }])
    expect(writes).toEqual([])
    expect(onDone).not.toHaveBeenCalled()
  })
})
