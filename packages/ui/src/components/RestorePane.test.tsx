import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import type { RestorableSession, RestoreOffer } from '@helm/core/types'
import { RestorePane, RestoreUnreadPane, type RestorePaneProps } from './RestorePane'

const NOW = new Date(2026, 9, 2, 13, 0, 0).getTime()
const EARLIER = NOW - 20 * 60_000
const LAST_WEEK = new Date(2026, 8, 30, 9, 15).getTime()

function session(id: number, name: string, extra: Partial<RestorableSession> = {}): RestorableSession {
  return {
    id,
    name,
    cwd: `C:\\work\\${name}`,
    branch: 'main',
    profile: null,
    profileGone: false,
    lastAt: EARLIER,
    blocked: null,
    ...extra
  }
}

const QUIET_REASON = 'Claude Code has no record of a conversation in it to reopen.'

const OFFER: RestoreOffer = {
  sessions: [
    session(4, 'tab baseline', { cwd: 'C:\\work\\helm', branch: 'feat/tab-baseline', profile: 'dev' }),
    session(7, 'accruals report', { cwd: 'C:\\work\\reporting', profileGone: true, lastAt: LAST_WEEK }),
    session(9, 'quiet', { blocked: QUIET_REASON, lastAt: null })
  ],
  elsewhere: 0,
  layout: null
}

function renderPane(overrides: Partial<RestorePaneProps> = {}): RestorePaneProps {
  const props: RestorePaneProps = {
    offer: OFFER,
    withoutAsking: false,
    onWithoutAskingChange: vi.fn(),
    busy: false,
    onResume: vi.fn(),
    onNotNow: vi.fn(),
    now: NOW,
    ...overrides
  }
  render(<RestorePane {...props} />)
  return props
}

function row(id: number): HTMLElement {
  const found = document.querySelector<HTMLElement>(`[data-restore-row="${String(id)}"]`)
  if (found === null) throw new Error(`no row ${String(id)}`)
  return found
}

const text = (id: number): string => row(id).textContent ?? ''
const button = (name: string): HTMLElement => screen.getByRole('button', { name })

describe('RestorePane', () => {
  it('lists every session the crash took, ticked, and the one that cannot come back with its reason', () => {
    renderPane()
    screen.getByRole('region', { name: '3 sessions were running when Helm closed' })

    expect(within(row(4)).getByRole('checkbox', { name: 'tab baseline' })).toHaveProperty('checked', true)
    expect(text(4)).toContain('helm · feat/tab-baseline')
    expect(text(4)).toContain('dev')
    expect(text(4)).toContain(
      new Date(EARLIER).toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })
    )

    // A profile deleted since is said; an older session shows its date.
    expect(text(7)).toContain('Profile deleted')
    expect(text(7)).toContain(new Date(LAST_WEEK).toLocaleDateString(undefined, { month: 'short', day: 'numeric' }))

    const quiet = within(row(9)).getByRole('checkbox', { name: 'quiet' })
    expect(quiet).toHaveProperty('checked', false)
    expect(quiet).toHaveProperty('disabled', true)
    expect(text(9)).toContain(QUIET_REASON)
    expect(button('Resume 2 sessions')).toHaveProperty('disabled', false)
  })

  it('reopens what stays ticked, in list order', async () => {
    const props = renderPane()
    await userEvent.click(screen.getByRole('checkbox', { name: 'tab baseline' }))
    await userEvent.click(button('Resume 1 session'))
    expect(props.onResume).toHaveBeenCalledWith([{ id: 7 }])

    // The row is a label, so a click on its name ticks it back.
    await userEvent.click(within(row(4)).getByText('tab baseline'))
    await userEvent.click(button('Resume 2 sessions'))
    expect(props.onResume).toHaveBeenLastCalledWith([{ id: 4 }, { id: 7 }])
  })

  it('cannot resume nothing', async () => {
    renderPane()
    await userEvent.click(screen.getByRole('checkbox', { name: 'tab baseline' }))
    await userEvent.click(screen.getByRole('checkbox', { name: 'accruals report' }))
    expect(button('Resume 0 sessions')).toHaveProperty('disabled', true)
  })

  it('is busy while reopening', () => {
    renderPane({ busy: true })
    expect(button('Resuming…')).toHaveProperty('disabled', true)
    expect(button('Not now')).toHaveProperty('disabled', true)
  })

  it('answers not now, and ticks the setting from its own checkbox', async () => {
    const props = renderPane()
    await userEvent.click(screen.getByRole('checkbox', { name: 'Always resume without asking' }))
    expect(props.onWithoutAskingChange).toHaveBeenCalledWith(true)
    await userEvent.click(button('Not now'))
    expect(props.onNotNow).toHaveBeenCalledTimes(1)
  })

  it('names each session by what History calls it, and says Not now keeps them at the top of History', () => {
    renderPane({ offer: { ...OFFER, sessions: [session(4, 'tab baseline', { title: 'Tabs: keep the baseline' })] } })
    within(row(4)).getByRole('checkbox', { name: 'Tabs: keep the baseline' })
    expect(document.body.textContent).toContain('Not now keeps them at the top of History (Ctrl+H).')
  })

  it('says how many more are still running elsewhere, and reads one session in the singular', () => {
    renderPane({ offer: { ...OFFER, sessions: [OFFER.sessions[0]!], elsewhere: 2 } })
    screen.getByRole('heading', { name: '1 session was running when Helm closed' })
    expect(document.body.textContent).toContain('2 more are still running somewhere else, and left alone.')
  })

  it('names the host that stopped when the sessions went with it, not Helm', () => {
    renderPane({ offer: { ...OFFER, sessions: [OFFER.sessions[0]!] }, lostWith: 'this PC’s host' })
    screen.getByRole('region', { name: '1 session was running when this PC’s host stopped' })
    expect(document.body.textContent).toContain('This PC’s host didn’t shut down cleanly. Pick up where they left off.')
    expect(document.body.textContent).not.toContain('Helm')
  })
  it('offers a session whose agent cannot be composed again unticked but tickable, saying why, and restores it without its agent', async () => {
    const problem = 'research-lead is no longer among this PC’s agents.'
    const props = renderPane({
      offer: {
        ...OFFER,
        sessions: [
          session(4, 'orbit-api', { agent: 'lead' }),
          session(5, 'research', { agent: 'research-lead', agentProblem: problem })
        ]
      }
    })
    expect(row(4).querySelector('[data-agent-tag]')?.textContent).toBe('lead')
    expect(row(5).querySelector('[data-agent-tag]')?.getAttribute('data-agent-gone')).toBe('true')
    const box = within(row(5)).getByRole('checkbox', { name: 'research' })
    expect(box).toHaveProperty('checked', false)
    expect(box).toHaveProperty('disabled', false)
    expect(text(5)).toContain(`${problem} Restored, it comes back without its agent.`)
    expect(text(5)).not.toContain('research · main')

    await userEvent.click(button('Resume 1 session'))
    expect(props.onResume).toHaveBeenLastCalledWith([{ id: 4 }])
    await userEvent.click(box)
    await userEvent.click(button('Resume 2 sessions'))
    expect(props.onResume).toHaveBeenLastCalledWith([{ id: 4 }, { id: 5, withoutAgent: true }])
  })
})

describe('when what was running could not be read', () => {
  it('says it could not read it, never that there is nothing, and offers Try again and Not now', async () => {
    const onTryAgain = vi.fn()
    const onNotNow = vi.fn()
    render(
      <RestoreUnreadPane
        message="The sessions' titles could not be read."
        retrying={false}
        onTryAgain={onTryAgain}
        onNotNow={onNotNow}
        lostWith="this PC’s host"
      />
    )
    const pane = screen.getByRole('region', { name: 'Could not read what was running when this PC’s host stopped' })
    expect(pane.textContent).toContain("The sessions' titles could not be read.")
    expect(pane.textContent).not.toContain('Resume')
    await userEvent.click(button('Try again'))
    expect(onTryAgain).toHaveBeenCalledTimes(1)
    await userEvent.click(button('Not now'))
    expect(onNotNow).toHaveBeenCalledTimes(1)
  })

  it('holds Try again while the read it asked for is under way', () => {
    render(<RestoreUnreadPane message="Unreadable." retrying onTryAgain={vi.fn()} onNotNow={vi.fn()} />)
    expect(screen.getByRole('heading').textContent).toBe('Could not read what was running when Helm closed')
    expect(button('Reading…')).toHaveProperty('disabled', true)
  })
})
