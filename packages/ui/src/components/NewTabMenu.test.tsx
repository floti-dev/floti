import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  EMPTY_INVENTORY,
  type HistorySession,
  type LaunchAgent,
  type LaunchAgents,
  type Profile,
  type Project
} from '@helm/core/types'
import { NewTabMenu, type NewTabMenuProps } from './NewTabMenu'

const HOME = 'C:\\Users\\someone'
const HARNESS = `${HOME}\\.harness\\dev`
const NOW = Date.UTC(2026, 9, 3, 12, 0, 0)
const HOUR = 3_600_000

function project(name: string, path = `${HARNESS}\\repos\\${name}`): Project {
  return {
    path,
    name,
    kind: 'repo',
    harnessPath: HARNESS,
    hasClaudeDir: true,
    inventory: EMPTY_INVENTORY,
    git: null
  }
}

const dev: Project = { ...project('dev', HARNESS), kind: 'harness' }
const helm = project('helm')
const timeclick = project('timeclick')
const notes = project('notes')
const PROJECTS = [dev, helm, timeclick, notes]

function profile(id: number, name: string, overlays: string[], patch: Partial<Profile> = {}): Profile {
  return {
    id,
    name,
    root: HARNESS,
    overlays,
    access: overlays,
    model: null,
    effort: null,
    permissionMode: null,
    agent: null,
    mcp: [],
    openingPrompt: null,
    pinnedOrder: null,
    createdAt: '',
    updatedAt: '',
    ...patch
  }
}

const devProfile = profile(1, 'dev', [HARNESS], { permissionMode: 'auto' })
const timeclickProfile = profile(2, 'Timeclick', [HARNESS, timeclick.path], { pinnedOrder: 0 })
const PROFILES = [devProfile, timeclickProfile]

function conversation(sessionId: string, title: string, lastAt: number): HistorySession {
  return {
    sessionId,
    project: helm.path,
    projectName: 'helm',
    promptCount: 3,
    firstAt: lastAt - HOUR,
    lastAt,
    firstPrompt: title,
    title,
    titleFallback: false,
    label: null,
    transcriptFile: null,
    transcriptBytes: null,
    projectExists: true,
    archive: null,
    archivedMessages: null
  }
}

let anchor: HTMLButtonElement

afterEach(() => anchor.remove())

function renderMenu(overrides: Partial<NewTabMenuProps> = {}) {
  anchor = document.createElement('button')
  anchor.textContent = '+'
  document.body.append(anchor)
  anchor.focus()
  const props: NewTabMenuProps = {
    anchor,
    projects: PROJECTS,
    recency: new Map([
      [timeclick.path.toLowerCase(), NOW - 5 * 60_000],
      [helm.path.toLowerCase(), NOW - 2 * HOUR],
      [dev.path.toLowerCase(), NOW - 26 * HOUR],
      [notes.path.toLowerCase(), NOW - 50 * HOUR]
    ]),
    live: [],
    profiles: PROFILES,
    home: HOME,
    initialPath: helm.path,
    resumable: new Map(),
    onShowing: vi.fn(),
    busy: false,
    error: null,
    now: NOW,
    onStart: vi.fn(),
    onProfile: vi.fn(),
    onBrowser: vi.fn(),
    onTerminal: vi.fn(),
    onDismiss: vi.fn(),
    ...overrides
  }
  const view = render(<NewTabMenu {...props} />)
  return { props, ...view }
}

async function chooseSession(): Promise<HTMLElement> {
  await userEvent.click(within(screen.getByRole('menu', { name: 'New tab' })).getByText('Session'))
  return screen.getByRole('dialog', { name: 'New session' })
}

describe('NewTabMenu: the kinds', () => {
  it('asks what kind of tab first', () => {
    renderMenu()
    const menu = screen.getByRole('menu', { name: 'New tab' })
    expect(within(menu).getAllByRole('menuitem').map((item) => item.textContent)).toEqual([
      'SessionCtrl N',
      'Profile session',
      'Agent session',
      'Terminal',
      'Browser tab'
    ])
  })

  it('opens a browser tab straight away and closes', async () => {
    const { props } = renderMenu()
    await userEvent.click(screen.getByText('Browser tab'))
    expect(props.onBrowser).toHaveBeenCalledOnce()
    expect(props.onDismiss).toHaveBeenCalledOnce()
  })

  it('offers no profile session when there are no profiles', async () => {
    const { props } = renderMenu({ profiles: [] })
    const item = screen.getByText('Profile session').closest('[role="menuitem"]')!
    expect(item.getAttribute('aria-disabled')).toBe('true')
    await userEvent.click(item)
    expect(screen.queryByRole('menu', { name: 'Start a profile' })).toBeNull()
    expect(props.onDismiss).not.toHaveBeenCalled()
  })

  it('closes on Escape, handing the focus back to the button', async () => {
    const { props } = renderMenu()
    await userEvent.keyboard('{Escape}')
    expect(props.onDismiss).toHaveBeenCalledOnce()
  })
})

describe('NewTabMenu: profile session', () => {
  it('lists the profiles, pinned first, and one click starts one', async () => {
    const { props } = renderMenu()
    await userEvent.click(screen.getByText('Profile session'))
    const menu = screen.getByRole('menu', { name: 'Start a profile' })
    expect(within(menu).getAllByRole('menuitem').map((item) => item.textContent)).toEqual(['Timeclickdev', 'devdev'])
    await userEvent.click(within(menu).getByText('dev', { selector: 'span.truncate' }))
    expect(props.onProfile).toHaveBeenCalledWith(devProfile)
    expect(props.onDismiss).toHaveBeenCalledOnce()
  })
})

describe('NewTabMenu: terminal', () => {
  it('lists the pane’s folder first, then the rest by recency, and one click opens a terminal there', async () => {
    const { props } = renderMenu({ initialPath: dev.path })
    await userEvent.click(screen.getByText('Terminal'))
    const menu = screen.getByRole('menu', { name: 'Open a terminal in' })
    expect(within(menu).getAllByRole('menuitem').map((item) => item.textContent)).toEqual([
      'dev',
      'timeclick',
      'helm',
      'notes'
    ])
    await userEvent.click(within(menu).getByText('helm'))
    expect(props.onTerminal).toHaveBeenCalledWith(helm)
    expect(props.onStart).not.toHaveBeenCalled()
  })

  it('offers no terminal when there are no folders', async () => {
    renderMenu({ projects: [] })
    const item = screen.getByText('Terminal').closest('[role="menuitem"]')!
    expect(item.getAttribute('aria-disabled')).toBe('true')
    await userEvent.click(item)
    expect(screen.queryByRole('menu', { name: 'Open a terminal in' })).toBeNull()
  })
})

describe('NewTabMenu: session', () => {
  it('opens on the folder the pane is about, with the profile most about it, and says what will run', async () => {
    const { props } = renderMenu({ initialPath: timeclick.path })
    const popover = await chooseSession()
    expect(within(popover).getByRole('combobox', { name: 'Folder' })).toHaveProperty('value', timeclick.path)
    expect(within(popover).getByRole('combobox', { name: 'Profile' })).toHaveProperty('value', '2')
    expect(popover.querySelector('[data-launch-sentence]')?.textContent).toBe(
      'Runs claude in ~\\.harness\\dev\\repos\\timeclick with the Timeclick profile, composing dev and timeclick.'
    )
    expect(props.onShowing).toHaveBeenCalledWith([timeclick.path])
  })

  it('opens on the most recent folder when the pane is about none', async () => {
    renderMenu({ initialPath: null })
    const popover = await chooseSession()
    expect(within(popover).getByRole('combobox', { name: 'Folder' })).toHaveProperty('value', timeclick.path)
  })

  it('starts what it shows, never beside, with the profile following the folder until one is chosen', async () => {
    const { props } = renderMenu()
    const popover = await chooseSession()
    await userEvent.selectOptions(within(popover).getByRole('combobox', { name: 'Folder' }), dev.path)
    expect(within(popover).getByRole('combobox', { name: 'Profile' })).toHaveProperty('value', '1')
    await userEvent.selectOptions(within(popover).getByRole('combobox', { name: 'Profile' }), '')
    await userEvent.selectOptions(within(popover).getByRole('combobox', { name: 'Folder' }), helm.path)
    expect(within(popover).getByRole('combobox', { name: 'Profile' })).toHaveProperty('value', '')
    await userEvent.click(within(popover).getByRole('button', { name: 'Start session' }))
    expect(props.onStart).toHaveBeenCalledWith({
      project: helm,
      profileId: null,
      permissionMode: null,
      resume: null,
      beside: false,
      agent: null,
      backend: null,
      chosen: { model: null, effort: null }
    })
  })

  it('starts on Enter from the folder, and carries the profile’s mode', async () => {
    const { props } = renderMenu({ initialPath: dev.path })
    const popover = await chooseSession()
    within(popover).getByRole('combobox', { name: 'Folder' }).focus()
    await userEvent.keyboard('{Enter}')
    expect(props.onStart).toHaveBeenCalledWith({
      project: dev,
      profileId: 1,
      permissionMode: 'auto',
      resume: null,
      beside: false,
      agent: null,
      backend: null,
      chosen: { model: null, effort: null }
    })
  })

  it('offers the folders worked in last, one click each with the profile each would start with', async () => {
    const { props } = renderMenu()
    const popover = await chooseSession()
    const recent = within(popover).getByRole('list', { name: 'Recent' })
    const rows = within(recent).getAllByRole('button')
    expect(rows.map((row) => row.textContent)).toEqual(['timeclickTimeclick5m', 'helm2h', 'devdev1d'])
    expect(rows[0]!.getAttribute('title')).toContain('with the Timeclick profile')
    await userEvent.click(rows[2]!)
    expect(props.onStart).toHaveBeenCalledWith({
      project: dev,
      profileId: 1,
      permissionMode: 'auto',
      resume: null,
      beside: false
    })
  })

  it('resumes one of the folder’s conversations from Resume', async () => {
    const payroll = conversation('a1', 'payroll export fix', NOW - 3 * HOUR)
    const { props } = renderMenu({ resumable: new Map([[helm.path.toLowerCase(), [payroll]]]) })
    const popover = await chooseSession()
    await userEvent.click(within(popover).getByRole('button', { name: 'Resume…' }))
    const menu = screen.getByRole('menu', { name: 'Resume a conversation' })
    await userEvent.click(within(menu).getByText('payroll export fix'))
    expect(props.onStart).toHaveBeenCalledWith({
      project: helm,
      profileId: null,
      permissionMode: null,
      resume: payroll,
      beside: false,
      agent: null,
      backend: null,
      chosen: { model: null, effort: null }
    })
  })

  it('has nothing to resume until the folder’s conversations are read, nor where there are none', async () => {
    const { rerender, props } = renderMenu()
    const popover = await chooseSession()
    const resume = within(popover).getByRole('button', { name: 'Resume…' })
    expect(resume).toHaveProperty('disabled', true)
    expect(resume.getAttribute('title')).toBe('Reading the conversations in helm…')
    rerender(<NewTabMenu {...props} resumable={new Map([[helm.path.toLowerCase(), []]])} />)
    expect(resume.getAttribute('title')).toBe('Nothing in helm can be reopened')
  })

  it('says why a launch failed, and closes on Escape', async () => {
    const { props } = renderMenu({ error: 'claude is not on PATH' })
    const popover = await chooseSession()
    expect(within(popover).getByRole('alert').textContent).toBe('claude is not on PATH')
    await userEvent.keyboard('{Escape}')
    expect(props.onDismiss).toHaveBeenCalledOnce()
  })
})

describe('NewTabMenu: more than one host', () => {
  const HOSTS = [
    { id: 'local', name: 'This PC', remote: false },
    { id: 'h-build', name: 'build-box', remote: true }
  ]

  it('names the host it opens on and switches to another, staying open', async () => {
    const onHostChange = vi.fn()
    const { props } = renderMenu({ hosts: HOSTS, host: 'local', onHostChange })
    const menu = screen.getByRole('menu', { name: 'New tab' })
    const choices = within(menu).getAllByRole('menuitemcheckbox')
    expect(choices.map((item) => [item.textContent, item.getAttribute('aria-checked')])).toEqual([
      ['This PC', 'true'],
      ['build-box', 'false']
    ])
    await userEvent.click(within(menu).getByText('build-box'))
    expect(onHostChange).toHaveBeenCalledWith('h-build')
    expect(props.onDismiss).not.toHaveBeenCalled()
    expect(screen.getByRole('menu', { name: 'New tab' })).toBeTruthy()
  })

  it('on build-box, its session popover says so and starts in the folders it is given', async () => {
    const box = project('app', '/srv/app')
    const { props } = renderMenu({ hosts: HOSTS, host: 'h-build', projects: [box], initialPath: null, recency: new Map() })
    const popover = await chooseSession()
    expect(within(popover).getByText('build-box').closest('[data-popover-host]')?.textContent).toBe('on build-box')
    await userEvent.click(within(popover).getByRole('button', { name: 'Start session' }))
    expect(props.onStart).toHaveBeenCalledWith(expect.objectContaining({ project: box }))
  })

  it('names no host with only this PC', () => {
    renderMenu({ hosts: [HOSTS[0]!], host: 'local' })
    expect(screen.queryByRole('menuitemcheckbox')).toBeNull()
  })
})

// ---------------------------------------------------------------------------
// Agent session, and the popover's Agent picker (A21, A22)
// ---------------------------------------------------------------------------

function launchAgent(name: string, patch: Partial<LaunchAgent> = {}): LaunchAgent {
  return {
    name,
    description: `${name} does its work.`,
    file: `${name}.md`,
    backends: ['claude-code'],
    listedBackends: ['claude-code'],
    models: {},
    efforts: {},
    permission: null,
    profile: null,
    allowedTools: [],
    disallowedTools: [],
    worktree: false,
    canStart: false,
    cannotRun: null,
    ...patch
  }
}

const researchLead = launchAgent('research-lead', {
  description: 'Researches a new project with you.',
  models: { 'claude-code': 'opus' },
  efforts: { 'claude-code': 'xhigh' },
  permission: 'plan',
  allowedTools: ['Read'],
  canStart: true
})
const AGENTS: LaunchAgents = {
  agents: [
    launchAgent('docs-writer', { backends: [], listedBackends: ['codex'] }),
    launchAgent('lead', { description: 'Delegates work and keeps Trackr true.' }),
    researchLead
  ],
  bad: [{ name: 'deploy-check', file: 'deploy-check.md', problem: 'permission must be plan' }],
  backends: [{ id: 'claude-code', name: 'Claude Code' }]
}

describe('NewTabMenu: agent session', () => {
  it('lists the host’s agents with what each is, one that cannot run here disabled with why, and leaves a bad file out', async () => {
    renderMenu({ agents: AGENTS })
    await userEvent.click(screen.getByText('Agent session'))
    const menu = screen.getByRole('menu', { name: 'Start an agent session' })
    expect(menu.textContent).toContain('Agent session in helm')
    const items = within(menu).getAllByRole('menuitem')
    expect(items.map((item) => item.textContent)).toEqual([
      'leadDelegates work and keeps Trackr true.',
      'research-leadResearches a new project with you.',
      'docs-writerRuns only on codex, which This PC does not have.'
    ])
    expect(items[2]!.getAttribute('aria-disabled')).toBe('true')
    expect(menu.textContent).not.toContain('deploy-check')
  })

  it('turns into the session popover with the agent picked, in the pane’s folder, and starts as the agent', async () => {
    const { props } = renderMenu({ agents: AGENTS })
    await userEvent.click(screen.getByText('Agent session'))
    await userEvent.click(screen.getByText('research-lead'))
    const popover = screen.getByRole('dialog', { name: 'New session' })
    const pickers = within(popover).getAllByRole('combobox')
    // Agent sits between Folder and Profile.
    expect(pickers.map((picker) => picker.id.split('-').at(-1))).toEqual(['folder', 'agent', 'profile'])
    expect(within(popover).getByRole('combobox', { name: 'Folder' })).toHaveProperty('value', helm.path)
    expect(within(popover).getByRole('combobox', { name: 'Agent' })).toHaveProperty('value', 'research-lead')
    expect(popover.querySelector('[data-launch-sentence]')?.textContent).toBe(
      'Runs claude in ~\\.harness\\dev\\repos\\helm as the research-lead agent, and --model opus --effort xhigh --permission-mode plan and its tool rules. It can start agents of its own.'
    )
    expect(popover.querySelector('[data-agent-note]')?.textContent).toBe(
      "Its model, effort and permissions are research-lead's; Ctrl+N changes them."
    )
    await userEvent.click(within(popover).getByRole('button', { name: 'Start session' }))
    expect(props.onStart).toHaveBeenCalledWith({
      project: helm,
      profileId: null,
      permissionMode: 'plan',
      resume: null,
      beside: false,
      agent: 'research-lead',
      backend: 'claude-code',
      chosen: { model: 'opus', effort: 'xhigh' }
    })
  })

  it('is disabled while the host’s agents are being read, saying so, which is not having none', () => {
    renderMenu({ agents: null })
    const item = screen.getByText('Agent session').closest('[role="menuitem"]')!
    expect(item.getAttribute('aria-disabled')).toBe('true')
    expect(item.getAttribute('title')).toBe("Reading This PC's agents…")
  })

  it('is disabled, saying why, when the host has no agents', async () => {
    renderMenu({ agents: { agents: [], bad: [], backends: [] } })
    const item = screen.getByText('Agent session').closest('[role="menuitem"]')!
    expect(item.getAttribute('aria-disabled')).toBe('true')
    expect(item.getAttribute('title')).toBe('This PC has no agents yet.')
    await userEvent.click(item)
    expect(screen.queryByRole('menu', { name: 'Start an agent session' })).toBeNull()
  })

  it('opens the session popover at No agent, and a Recent row starts with No agent', async () => {
    const { props } = renderMenu({ agents: AGENTS })
    const popover = await chooseSession()
    expect(within(popover).getByRole('combobox', { name: 'Agent' })).toHaveProperty('value', '')
    await userEvent.selectOptions(within(popover).getByRole('combobox', { name: 'Agent' }), 'research-lead')
    const recent = within(popover).getByRole('list', { name: 'Recent' })
    await userEvent.click(within(recent).getAllByRole('button')[1]!)
    const started = vi.mocked(props.onStart).mock.calls.at(-1)?.[0]
    expect(started?.project).toBe(helm)
    expect(started?.agent ?? null).toBeNull()
  })
})
