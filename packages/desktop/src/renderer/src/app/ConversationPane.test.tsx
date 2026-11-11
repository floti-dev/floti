import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ConversationRef, LaunchAgents } from '@helm/core'
import { bridge } from './bridge.testkit'
import { ConversationPane, type ConversationPaneProps } from './ConversationPane'

vi.mock('./bridge', () => import('./bridge.testkit'))

const TAB: ConversationRef = {
  kind: 'conversation',
  sessionId: 'c1',
  title: 'Rate limits',
  project: 'C:\\code\\atlas-api',
  agent: 'lead'
}

beforeEach(() => {
  Element.prototype.scrollIntoView = vi.fn()
  bridge.whenUnanswered('pending')
  bridge.answer('archive:conversation', () => null)
})

afterEach(() => bridge.reset())

function renderPane(patch: Partial<ConversationPaneProps> = {}): ConversationPaneProps {
  const props: ConversationPaneProps = {
    tab: TAB,
    onResume: vi.fn(() => Promise.resolve(null)),
    onShow: vi.fn(),
    resuming: false,
    onRename: vi.fn(() => Promise.resolve(null)),
    onReadChild: vi.fn(),
    runsNow: () => false,
    ...patch
  }
  render(<ConversationPane {...props} />)
  return props
}

const listing = (...names: string[]): LaunchAgents => ({ agents: names.map((name) => ({ name })), bad: [] }) as unknown as LaunchAgents

describe('a conversation tab’s Resume', () => {
  it('shows the session instead while its conversation runs', async () => {
    const props = renderPane({ runsNow: (_tab, id) => id === 'c1' })
    expect(screen.queryByRole('button', { name: 'Resume' })).toBeNull()
    await userEvent.click(screen.getByRole('button', { name: 'Show' }))
    expect(props.onShow).toHaveBeenCalledWith(TAB)
    expect(props.onResume).not.toHaveBeenCalled()
  })

  it('resumes as its agent while the host lists it, and without it once it is gone', async () => {
    bridge.answer('agents:list', () => listing('lead'))
    const props = renderPane()
    await waitFor(() => expect(bridge.invoked('agents:list')).toHaveLength(1))
    await userEvent.click(screen.getByRole('button', { name: 'Resume' }))
    expect(props.onResume).toHaveBeenLastCalledWith(TAB, 'lead')
  })

  it('resumes without an agent the host no longer has', async () => {
    bridge.answer('agents:list', () => listing('worker'))
    const props = renderPane()
    await waitFor(() => expect(bridge.invoked('agents:list')).toHaveLength(1))
    await Promise.resolve()
    await userEvent.click(screen.getByRole('button', { name: 'Resume' }))
    expect(props.onResume).toHaveBeenLastCalledWith(TAB, undefined)
  })

  it('says a refused rename with its sentence', async () => {
    renderPane({ onRename: vi.fn(() => Promise.resolve('The host refused the name.')) })
    await userEvent.click(screen.getByRole('button', { name: 'Rename' }))
    await userEvent.type(screen.getByRole('textbox', { name: 'Name' }), 'x{Enter}')
    expect((await screen.findByRole('alert')).textContent).toBe('The host refused the name.')
  })
})
