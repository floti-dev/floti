import { fireEvent, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { HistorySession, LaunchAgents, RestorableSession } from '@helm/core'
import { historySession } from '../lib/historyPalette.testkit'
import { HistoryPalette, type HistoryAnswer, type HistoryPaletteProps } from './HistoryPalette'

const MINUTE = 60_000

/** Ten sessions worked in last, newest first, titled by their agents. */
const RECENT: HistorySession[] = Array.from({ length: 12 }, (_, n) =>
  historySession({
    sessionId: `s${String(n)}`,
    agentTitle: `Session number ${String(n)}`,
    lastAt: Date.now() - (n + 1) * 5 * MINUTE,
    firstAt: Date.now() - (n + 2) * 5 * MINUTE
  })
)

const answer = (sessions: HistorySession[], total = sessions.length): HistoryAnswer => ({ sessions, total })

function renderPalette(overrides: Partial<HistoryPaletteProps> = {}): HistoryPaletteProps {
  const props: HistoryPaletteProps = {
    query: '',
    onQueryChange: vi.fn(),
    recent: answer(RECENT, 806),
    total: 806,
    readError: null,
    titles: null,
    conversations: null,
    children: new Map(),
    onUnfold: vi.fn(),
    running: new Map(),
    agents: { agents: [{ name: 'lead' }, { name: 'researcher' }], bad: [] } as unknown as LaunchAgents,
    profileName: (id) => (id === 3 ? 'Atlas' : null),
    stopped: null,
    onResumeStopped: vi.fn(),
    onDismissStopped: vi.fn(),
    onResume: vi.fn(),
    onShow: vi.fn(),
    onRead: vi.fn(),
    onRename: vi.fn(),
    onRetry: vi.fn(),
    onDismiss: vi.fn(),
    ...overrides
  }
  const view = render(<HistoryPalette {...props} />)
  Object.assign(props, { rerender: (next: Partial<HistoryPaletteProps>) => view.rerender(<HistoryPalette {...props} {...next} />) })
  return props
}

const field = (): HTMLElement => screen.getByRole('combobox', { name: 'Search History' })
const options = (): HTMLElement[] => screen.queryAllByRole('option')
const selected = (): HTMLElement => options().find((option) => option.getAttribute('aria-selected') === 'true')!
const preview = (): string => document.querySelector('[data-history-preview]')?.textContent ?? ''
const keys = (): string => document.querySelector('[data-history-keys]')?.textContent ?? ''
const press = (key: string, init: Partial<KeyboardEventInit> = {}): void => {
  fireEvent.keyDown(field(), { key, ...init })
}

afterEach(() => vi.restoreAllMocks())

describe('History with nothing typed', () => {
  it('lists the 10 sessions worked in last by title, the first highlighted, and says how many there are', () => {
    renderPalette()
    expect(screen.getByRole('dialog', { name: 'History' })).toBeTruthy()
    expect(options()).toHaveLength(10)
    expect(options()[0]?.textContent).toContain('Session number 0')
    expect(options()[9]?.textContent).toContain('Session number 9')
    expect(selected().textContent).toContain('Session number 0')
    expect(document.querySelector('[data-history-foot]')?.textContent).toBe('The 10 you worked in last. Type to search all 806.')
  })

  it('says what Enter runs, and Enter resumes the highlighted session', () => {
    const props = renderPalette({
      recent: answer([historySession({ agentTitle: 'Tide flags', agent: 'researcher', profileId: 3, branch: 'main' })])
    })
    expect(preview()).toContain('C:\\code\\atlas-api')
    expect(preview()).toContain('› main')
    expect(preview()).toContain('researcher agent')
    expect(preview()).toContain('claude --resume in C:\\code\\atlas-api with the Atlas profile as the researcher agent')
    press('Enter')
    expect(props.onResume).toHaveBeenCalledWith(expect.objectContaining({ sessionId: 'a1' }), { withoutAgent: false })
  })

  it('gives its agent’s last reply, and none for a session running now', () => {
    const reply = 'The new --since flag replaces --after.'
    const { rerender } = renderPalette({ recent: answer([historySession({ lastReply: reply })]) }) as HistoryPaletteProps & {
      rerender: (next: Partial<HistoryPaletteProps>) => void
    }
    expect(preview()).toContain(`Last reply. ${reply}`)
    rerender({ running: new Map([['a1', { working: false, here: false }]]) })
    expect(preview()).not.toContain('Last reply')
  })

  it('moves with the arrows, and a click highlights without starting anything', async () => {
    const props = renderPalette()
    press('ArrowDown')
    expect(selected().textContent).toContain('Session number 1')
    await userEvent.click(options()[4]!)
    expect(selected().textContent).toContain('Session number 4')
    expect(props.onResume).not.toHaveBeenCalled()
    await userEvent.dblClick(options()[4]!)
    expect(props.onResume).toHaveBeenCalledWith(expect.objectContaining({ sessionId: 's4' }), { withoutAgent: false })
  })

  it('names a session from before titles by its first prompt in quotes, and its agent by its tag', () => {
    renderPalette({ recent: answer([historySession({ agent: 'researcher' })]) })
    expect(options()[0]?.textContent).toContain('“add rate limits to the public endpoints”')
    expect(options()[0]?.querySelector('[data-agent-tag="researcher"]')).not.toBeNull()
  })

  it('shows a session running now, Enter shows it, and Ctrl+Enter does nothing for it', () => {
    const props = renderPalette({
      recent: answer([historySession({ sessionId: 'LIVE' })]),
      running: new Map([['live', { working: true, here: true }]])
    })
    expect(options()[0]?.textContent).toContain('working')
    expect(keys()).toContain('Show')
    expect(keys()).not.toContain('Read')
    press('Enter', { ctrlKey: true })
    expect(props.onRead).not.toHaveBeenCalled()
    press('Enter')
    expect(props.onShow).toHaveBeenCalledWith(expect.objectContaining({ sessionId: 'LIVE' }))
    expect(props.onResume).not.toHaveBeenCalled()
  })

  it('reads a session that cannot be resumed, saying why', () => {
    const props = renderPalette({ recent: answer([historySession({ transcriptFile: null })]) })
    expect(options()[0]?.textContent).toContain('archived')
    expect(preview()).toContain('Claude Code removed its transcript')
    press('Enter')
    expect(props.onRead).toHaveBeenCalledWith(expect.objectContaining({ sessionId: 'a1' }), null)
  })

  it('names a gone agent and resumes without it', () => {
    const props = renderPalette({ recent: answer([historySession({ agent: 'writer' })]) })
    expect(options()[0]?.textContent).toContain('agent gone')
    expect(preview()).toContain('The writer agent is gone. Enter resumes it without the agent.')
    press('Enter')
    expect(props.onResume).toHaveBeenCalledWith(expect.anything(), { withoutAgent: true })
  })

  it('opens a session read-only on Ctrl+Enter', () => {
    const props = renderPalette()
    press('Enter', { ctrlKey: true })
    expect(props.onRead).toHaveBeenCalledWith(expect.objectContaining({ sessionId: 's0' }), null)
  })
})

describe('a lead and its children', () => {
  const lead = historySession({ sessionId: 'lead', agentTitle: 'Invoice export', agent: 'lead', children: 2 })
  const done = historySession({
    sessionId: 'kid1',
    agentTitle: 'Export retry path',
    agent: 'reviewer',
    parent: 'lead',
    endedBy: 'exited',
    exitCode: 0,
    lastReport: { state: 'done', text: 'Reviewed.', at: '' }
  })
  const stuck = historySession({
    sessionId: 'kid2',
    agentTitle: 'Queue worker',
    agent: 'lead',
    parent: 'lead',
    endedBy: 'floti',
    lastReport: { state: 'stuck', text: 'The runner needs the lock.', at: '' }
  })

  it('counts its children, unfolds them under it on →, and folds them on ←', () => {
    const props = renderPalette({ recent: answer([lead]) })
    expect(options()[0]?.textContent).toContain('2 children')
    press('ArrowRight')
    expect(props.onUnfold).toHaveBeenCalledWith(lead)
    ;(props as unknown as { rerender: (next: Partial<HistoryPaletteProps>) => void }).rerender({
      children: new Map([['lead', answer([done, stuck])]])
    })
    expect(options()).toHaveLength(3)
    expect(options()[1]?.textContent).toContain('Export retry path')
    expect(options()[1]?.textContent).toMatch(/Done \d\d:\d\d/)
    expect(options()[2]?.textContent).toMatch(/Ended with it \d\d:\d\d/)
    // Each ended child has the ended dot the Sessions tree gives it.
    expect(options().slice(1).map((option) => option.querySelector('[data-history-dot]')?.getAttribute('data-history-dot'))).toEqual(['ended', 'ended'])
    press('ArrowDown')
    press('ArrowDown')
    expect(preview()).toContain('Started by Invoice export')
    expect(preview()).toContain('Reported stuck. The runner needs the lock.')
    expect(keys()).toContain('Fold')
    press('ArrowLeft')
    expect(options()).toHaveLength(1)
    expect(selected().textContent).toContain('Invoice export')
  })
})

describe('History typed into', () => {
  const titled = historySession({ sessionId: 't1', agentTitle: 'Rate limits on the public endpoints' })
  const said = historySession({
    sessionId: 'c1',
    agentTitle: 'Auth middleware',
    matches: [
      { uuid: 'm1', at: new Date(2026, 10, 6, 16, 3).getTime(), text: 'the rate limit middleware runs before auth' },
      { uuid: 'm2', at: new Date(2026, 10, 6, 16, 9).getTime(), text: 'the general rate limit is unchanged' }
    ],
    matchCount: 6
  })

  it('gives two counted lists, opens on the one with matches, and Tab moves between them', () => {
    renderPalette({ query: 'rate limit', titles: answer([titled]), conversations: answer([said], 9) })
    const lists = screen.getByRole('tablist', { name: 'History lists' })
    expect(within(lists).getByRole('tab', { selected: true }).textContent).toBe('Titles and prompts1')
    expect(lists.textContent).toContain('Conversations9')
    expect(options()[0]?.querySelector('.text-accent-text')?.textContent).toBe('Rate')
    press('Tab')
    expect(within(lists).getByRole('tab', { selected: true }).textContent).toBe('Conversations9')
    expect(options().map((option) => option.dataset.historyRow)).toEqual(['session', 'line', 'line'])
    expect(selected().textContent).toContain('16:03')
    expect(document.body.textContent).toContain('and 4 more in this conversation')
  })

  it('opens on Conversations when only they match, on its first matched line, and Ctrl+Enter on a line opens it there', () => {
    const props = renderPalette({ query: 'rate limit', titles: answer([]), conversations: answer([said]) })
    expect(selected().textContent).toContain('16:03')
    expect(preview()).toContain('opens this conversation read-only at 16:03')
    press('ArrowUp')
    expect(selected().dataset.historyRow).toBe('session')
    press('ArrowDown')
    press('Enter', { ctrlKey: true })
    expect(props.onRead).toHaveBeenCalledWith(expect.objectContaining({ sessionId: 'c1' }), said.matches?.[0])
  })

  it('says No match while the other list has some, and Unknown when the archive could not be searched', () => {
    renderPalette({ query: 'sqlite wal', titles: answer([]), conversations: { error: 'database is locked' } })
    expect(document.querySelector('[data-history-state]')?.getAttribute('data-history-state')).toBe('no-match')
    // No match has nothing to try again; the archive that could not be searched does.
    expect(keys()).not.toContain('Try again')
    expect(screen.getByRole('tablist').textContent).toContain('ConversationsUnknown')
    press('Tab')
    expect(document.body.textContent).toContain('The archive could not be searched')
    expect(document.body.textContent).toContain('database is locked')
    expect(keys()).toContain('Try again')
  })
})

describe('F2 renames in place', () => {
  it('keeps the name on Enter, goes back on an empty one, and Escape leaves it without closing History', async () => {
    const props = renderPalette({ recent: answer([historySession({ agentTitle: 'Rate limits' })]) })
    press('F2')
    const name = screen.getByRole('textbox', { name: 'Name' })
    expect(preview()).toContain('Your name replaces “Rate limits”')
    await userEvent.clear(name)
    await userEvent.type(name, 'Limits per key{Enter}')
    expect(props.onRename).toHaveBeenCalledWith(expect.objectContaining({ sessionId: 'a1' }), 'Limits per key')

    press('F2')
    await userEvent.clear(screen.getByRole('textbox', { name: 'Name' }))
    await userEvent.type(screen.getByRole('textbox', { name: 'Name' }), '{Enter}')
    expect(props.onRename).toHaveBeenLastCalledWith(expect.anything(), null)

    press('F2')
    await userEvent.type(screen.getByRole('textbox', { name: 'Name' }), 'x{Escape}')
    expect(props.onRename).toHaveBeenCalledTimes(2)
    expect(props.onDismiss).not.toHaveBeenCalled()
    expect(screen.queryByRole('textbox', { name: 'Name' })).toBeNull()
  })
})

describe('the states', () => {
  it('reads, is empty, and could not read the history file', () => {
    const { unmount } = render(<HistoryPalette {...renderPropsOnly({ recent: null, total: null })} />)
    expect(screen.getAllByText('Reading the history file…')).toHaveLength(1)
    unmount()
    document.body.innerHTML = ''
    render(<HistoryPalette {...renderPropsOnly({ recent: answer([]), total: 0 })} />)
    expect(document.body.textContent).toContain('No sessions yet')
    document.body.innerHTML = ''
    const props = renderPropsOnly({
      recent: answer([]),
      total: null,
      readError: { file: 'C:\\Users\\you\\.claude\\history.jsonl', message: 'The process cannot access the file.' }
    })
    render(<HistoryPalette {...props} />)
    expect(document.body.textContent).toContain('Could not read the history file')
    expect(document.body.textContent).toContain('C:\\Users\\you\\.claude\\history.jsonl')
    press('Enter')
    expect(props.onRetry).toHaveBeenCalled()
  })
})

describe('after a crash', () => {
  const lost = (id: number, patch: Partial<RestorableSession> = {}): RestorableSession => ({
    id,
    name: `tab ${String(id)}`,
    cwd: `C:\\code\\p${String(id)}`,
    branch: null,
    profile: null,
    profileGone: false,
    lastAt: null,
    blocked: null,
    conversationId: `c${String(id)}`,
    title: `Stopped work ${String(id)}`,
    ...patch
  })
  const group = {
    at: new Date(2026, 10, 7, 9, 41).getTime(),
    sessions: [lost(1), lost(2), lost(3, { agent: 'writer', agentProblem: 'The writer agent is gone.' }), lost(4, { blocked: 'C:\\code\\p4 is no longer on disk.' })]
  }

  it('starts with the Stopped group: a row resuming every one that can come back, each session, and Dismiss', async () => {
    const props = renderPalette({ stopped: group })
    expect(document.querySelector('[data-history-section="stopped"]')?.textContent).toContain('Stopped at 09:41')
    expect(selected().textContent).toContain('Resume the 2 that can come back')
    expect(keys()).toContain('Resume 2')
    press('Enter')
    expect(props.onResumeStopped).toHaveBeenCalledWith([group.sessions[0], group.sessions[1]])
    expect(options()[3]?.textContent).toContain('agent gone')
    expect(options()[4]?.textContent).toContain('folder gone')
    await userEvent.click(screen.getByRole('button', { name: 'Dismiss' }))
    expect(props.onDismissStopped).toHaveBeenCalled()
  })
})

function renderPropsOnly(overrides: Partial<HistoryPaletteProps>): HistoryPaletteProps {
  return {
    query: '',
    onQueryChange: vi.fn(),
    recent: null,
    total: null,
    readError: null,
    titles: null,
    conversations: null,
    children: new Map(),
    onUnfold: vi.fn(),
    running: new Map(),
    agents: null,
    profileName: () => null,
    stopped: null,
    onResumeStopped: vi.fn(),
    onDismissStopped: vi.fn(),
    onResume: vi.fn(),
    onShow: vi.fn(),
    onRead: vi.fn(),
    onRename: vi.fn(),
    onRetry: vi.fn(),
    onDismiss: vi.fn(),
    ...overrides
  }
}
