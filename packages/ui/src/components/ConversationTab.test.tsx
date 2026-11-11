import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { ArchiveMessage, ArchivedConversation } from '@helm/core'
import { ConversationTab, readAsTurns, summariseTools, type ConversationTabProps } from './ConversationTab'

const message = (uuid: string, role: 'user' | 'assistant', text: string, minute: number): ArchiveMessage => ({
  uuid,
  role,
  text,
  at: new Date(2026, 10, 6, 15, minute).getTime()
})

const MESSAGES = [
  message('u1', 'user', 'add rate limits to the public endpoints', 48),
  message('t1', 'assistant', '[tool: Read]\n\n[tool: Read]', 49),
  message('t2', 'assistant', '[tool: Grep]', 50),
  message('a1', 'assistant', '120 a minute holds; the rate limit stays', 52),
  message('a2', 'assistant', 'In server/app.ts the rate limit runs before auth', 58)
]

const CONVERSATION: ArchivedConversation = {
  sessionId: 'c1',
  sourceFile: 'c1.jsonl',
  state: 'archived',
  firstAt: MESSAGES[0]!.at,
  lastAt: MESSAGES[4]!.at,
  messageCount: 214,
  rawBytes: 1,
  storedBytes: 1,
  capturedAt: '2026-11-06T17:30:00Z',
  evictedAt: null,
  messages: MESSAGES
}

beforeEach(() => {
  Element.prototype.scrollIntoView = vi.fn()
})

function renderTab(overrides: Partial<ConversationTabProps> = {}): ConversationTabProps {
  const props: ConversationTabProps = {
    title: 'Rate limits on the public endpoints',
    project: 'C:\\code\\atlas-api',
    facts: [],
    conversation: CONVERSATION,
    onRename: vi.fn(),
    ...overrides
  }
  render(<ConversationTab {...props} />)
  return props
}

describe('a conversation read back', () => {
  it('folds a run of tool calls into one line, counting repeats', () => {
    const turns = readAsTurns(MESSAGES)
    expect(turns.map((turn) => turn.kind)).toEqual(['message', 'tools', 'message', 'message'])
    expect(summariseTools(['Read', 'Read', 'Grep'])).toBe('Read ×2 · Grep')
  })

  it('is named by its title, says it is read only, and draws each turn with who said it', () => {
    renderTab({ agent: 'researcher' })
    expect(screen.getByRole('heading', { name: 'Rate limits on the public endpoints' })).toBeTruthy()
    expect(document.body.textContent).toContain('Read only')
    expect(document.body.textContent).toContain('214 messages archived')
    expect(document.querySelectorAll('[data-transcript-message="user"]')).toHaveLength(1)
    expect(document.querySelector('[data-transcript-tools]')?.textContent).toBe('Read ×2 · Grep')
  })

  it('opens at the line it was asked for, counts the matches, and steps through them', async () => {
    renderTab({ find: 'rate limit', at: 'a2' })
    expect(document.querySelector('[data-conversation-count]')?.textContent).toBe('3 of 3')
    expect(document.querySelector('[data-current-match]')?.getAttribute('data-message')).toBe('a2')
    await userEvent.click(screen.getByRole('button', { name: 'Next match' }))
    expect(document.querySelector('[data-current-match]')?.getAttribute('data-message')).toBe('u1')
    expect(Element.prototype.scrollIntoView).toHaveBeenCalled()
  })

  it('renames from its pencil, an empty name going back', async () => {
    const props = renderTab()
    await userEvent.click(screen.getByRole('button', { name: 'Rename' }))
    const name = screen.getByRole('textbox', { name: 'Name' })
    await userEvent.clear(name)
    await userEvent.type(name, 'Limits per key{Enter}')
    expect(props.onRename).toHaveBeenCalledWith('Limits per key')
    await userEvent.click(screen.getByRole('button', { name: 'Rename' }))
    await userEvent.clear(screen.getByRole('textbox', { name: 'Name' }))
    await userEvent.type(screen.getByRole('textbox', { name: 'Name' }), '{Enter}')
    expect(props.onRename).toHaveBeenLastCalledWith(null)
  })

  it('has Resume and Show in Explorer only where they can be done', async () => {
    const onResume = vi.fn()
    const onReveal = vi.fn()
    renderTab({ onResume, onReveal })
    await userEvent.click(screen.getByRole('button', { name: 'Resume' }))
    await userEvent.click(screen.getByRole('button', { name: 'Show in Explorer' }))
    expect(onResume).toHaveBeenCalled()
    expect(onReveal).toHaveBeenCalled()
  })

  it('shows what was typed where the archive has no conversation', () => {
    renderTab({
      conversation: null,
      onResume: undefined,
      prompts: [{ sessionId: 'c1', seq: 1, text: 'add rate limits', at: MESSAGES[0]!.at }]
    })
    expect(document.body.textContent).toContain('Floti has no archive of this conversation. What you typed is kept.')
    expect(document.body.textContent).toContain('add rate limits')
    expect(screen.queryByRole('button', { name: 'Resume' })).toBeNull()
  })
})
