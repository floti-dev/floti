import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { AppSettings, Walkthrough } from '@helm/core/types'
import type { ClaudeStatus } from '../../../../shared/ipc'
import { bridge } from '../bridge.testkit'
import { ShellWalkthrough, type ShellWalkthroughProps } from './ShellWalkthrough'

vi.mock('../bridge', () => import('../bridge.testkit'))

/** Steps 5 and 6 as the shell draws them in its main area. */

const PATIENCE = { timeout: 2000 }
const WORKSPACE = 'C:\\Users\\you\\Projects\\my-work'

const INSTALLED = { path: 'C:\\bin\\claude.exe', version: '2.1.319', error: null } as ClaudeStatus

function renderAt(walkthrough: Walkthrough, start: ShellWalkthroughProps['start'] = vi.fn(async () => undefined)) {
  const writes: Array<Partial<AppSettings>> = []
  const onDone = vi.fn()
  const view = render(
    <ShellWalkthrough
      walkthrough={walkthrough}
      bridge={bridge}
      write={(patch) => writes.push(patch)}
      start={start}
      onDone={onDone}
    />
  )
  return { ...view, writes, onDone, start }
}

beforeEach(() => {
  bridge.reset()
  bridge.answer('agents:list', () => ({
    agents: [],
    bad: [],
    backends: [{ id: 'claude-code', name: 'Claude Code' }],
    folder: null,
    readOnly: false,
    shipped: []
  }) as never)
  bridge.answer('setup:status', () => INSTALLED)
})

describe('the walkthrough in the shell', () => {
  it('tours the window at step 5, then moves to step 6 keeping the workspace', async () => {
    const { writes } = renderAt({ step: 5, workspace: WORKSPACE })
    expect(screen.getByRole('heading', { name: 'Around the window' })).toBeTruthy()
    await userEvent.click(screen.getByRole('button', { name: 'Continue' }))
    expect(writes).toEqual([{ walkthrough: { step: 6, workspace: WORKSPACE } }])
  })

  it('goes back from step 5 to step 4 in the setup window, which main opens again', async () => {
    bridge.answer('walkthrough:open', () => undefined)
    const { writes } = renderAt({ step: 5, workspace: WORKSPACE })
    await userEvent.click(screen.getByRole('button', { name: 'Back' }))
    expect(bridge.invoked('walkthrough:open')).toEqual([{ step: 4 }])
    expect(writes).toEqual([])
  })

  it('starts the agent picked in the workspace with the first message, then marks the walkthrough done', async () => {
    const { writes, onDone, start } = renderAt({ step: 6, workspace: WORKSPACE })
    expect(screen.getByText('Start an agent in my-work.')).toBeTruthy()
    await waitFor(() => expect(screen.getByRole('combobox', { name: 'Agent' })).toHaveProperty('value', 'claude-code'), PATIENCE)
    await userEvent.type(screen.getByRole('textbox', { name: /First message/ }), 'Read AGENTS.md.')
    await userEvent.click(screen.getByRole('button', { name: 'Start session' }))
    await waitFor(() => expect(onDone).toHaveBeenCalledWith(WORKSPACE, true), PATIENCE)
    expect(start).toHaveBeenCalledWith({ cwd: WORKSPACE, backend: 'claude-code', openingPrompt: 'Read AGENTS.md.' })
    expect(writes).toEqual([{ walkthrough: null }])
  })

  it('does not start an agent that is no longer installed, and says so', async () => {
    bridge.answer('setup:status', () => ({ ...INSTALLED, path: null, version: null, error: null }))
    const { writes, start } = renderAt({ step: 6, workspace: WORKSPACE })
    await waitFor(() => expect(screen.getByRole('combobox', { name: 'Agent' })).toHaveProperty('value', 'claude-code'), PATIENCE)
    await userEvent.click(screen.getByRole('button', { name: 'Start session' }))
    expect((await screen.findByRole('alert')).textContent).toBe('Claude Code is not installed on this PC.')
    expect(start).not.toHaveBeenCalled()
    expect(writes).toEqual([])
  })

  it('says why a session did not start, and stays on step 6', async () => {
    const { writes } = renderAt({ step: 6, workspace: WORKSPACE }, async () => {
      throw new Error('The host refused the launch.')
    })
    await waitFor(() => expect(screen.getByRole('combobox', { name: 'Agent' })).toHaveProperty('value', 'claude-code'), PATIENCE)
    await userEvent.click(screen.getByRole('button', { name: 'Start session' }))
    expect((await screen.findByRole('alert')).textContent).toBe('The host refused the launch.')
    expect(writes).toEqual([])
  })

  it('is skipped on step 6: done, with the workspace left selected and no session', async () => {
    const { writes, onDone, start } = renderAt({ step: 6, workspace: WORKSPACE })
    await userEvent.click(screen.getByRole('button', { name: 'Skip' }))
    expect(writes).toEqual([{ walkthrough: null }])
    expect(onDone).toHaveBeenCalledWith(WORKSPACE, false)
    expect(start).not.toHaveBeenCalled()
  })
})
