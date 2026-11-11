import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import {
  EMPTY_INVENTORY,
  type HistorySession,
  type LaunchAgent,
  type LaunchAgents,
  type LiveSession,
  type Profile,
  type Project
} from '@helm/core/types'
import { NewSessionDialog, type NewSessionDialogProps } from './NewSessionDialog'

const HOME = 'C:\\Users\\someone'
const HARNESS = `${HOME}\\.harness\\dev`
const NOW = Date.UTC(2026, 9, 2, 12, 0, 0)
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
const timeclick = project('timeclick')
const builder = project('TimeClick-Builder')
const helm = project('helm')
const PROJECTS = [dev, helm, builder, timeclick]

function profile(id: number, name: string, overlays: string[], patch: Partial<Profile> = {}): Profile {
  return {
    id,
    name,
    root: HARNESS,
    overlays,
    access: overlays,
    model: 'opus',
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
const timeclickProfile = profile(2, 'Timeclick', [HARNESS, timeclick.path], {
  permissionMode: 'bypassPermissions'
})
const PROFILES = [timeclickProfile, devProfile]

function conversation(sessionId: string, title: string, lastAt: number): HistorySession {
  return {
    sessionId,
    project: timeclick.path,
    projectName: 'timeclick',
    promptCount: 3,
    firstAt: lastAt - HOUR,
    lastAt,
    firstPrompt: title,
    title,
    titleFallback: false,
    label: null,
    transcriptFile: `${HOME}\\.claude\\projects\\x\\${sessionId}.jsonl`,
    transcriptBytes: 2048,
    projectExists: true,
    archive: null,
    archivedMessages: null
  }
}

const payroll = conversation('a1', 'payroll export fix', NOW - 20 * HOUR)
const breaks = conversation('a2', 'named breaks rollout', NOW - 4 * 24 * HOUR)

function renderDialog(overrides: Partial<NewSessionDialogProps> = {}): NewSessionDialogProps {
  const props: NewSessionDialogProps = {
    projects: PROJECTS,
    recency: new Map([
      [helm.path.toLowerCase(), NOW - HOUR],
      [timeclick.path.toLowerCase(), NOW - 2 * HOUR]
    ]),
    live: [],
    profiles: PROFILES,
    home: HOME,
    initialPath: null,
    resumable: new Map([[timeclick.path.toLowerCase(), [payroll, breaks]]]),
    onShowing: vi.fn(),
    busy: false,
    error: null,
    now: NOW,
    onStart: vi.fn(),
    onDismiss: vi.fn(),
    ...overrides
  }
  lastView = render(<NewSessionDialog {...props} />)
  return props
}

/** The last dialog rendered, for a test that hands it new props. */
let lastView: ReturnType<typeof render> | null = null

const dialog = (): HTMLElement => screen.getByRole('dialog', { name: 'New session' })
/** The folder list - the pickers' own options are options too. */
const list = (): HTMLElement => within(dialog()).getByRole('listbox', { name: 'Folders' })
const options = (): string[] =>
  within(list())
    .getAllByRole('option')
    .map((option) => option.getAttribute('aria-label') ?? '')
const highlighted = (): string | null =>
  within(list())
    .getAllByRole('option')
    .find((option) => option.getAttribute('aria-selected') === 'true')
    ?.getAttribute('aria-label') ?? null
const sentence = (): string => dialog().querySelector('[data-launch-sentence]')?.textContent ?? ''
const picker = (name: string): HTMLSelectElement =>
  within(dialog()).getByRole('combobox', { name }) as HTMLSelectElement

describe('NewSessionDialog', () => {
  it('opens on the field, with the folder it was opened on first and the rest by when they were last used', () => {
    const props = renderDialog({ initialPath: dev.path })
    expect(document.activeElement).toBe(within(dialog()).getByRole('combobox', { name: 'Folder' }))
    expect(options().slice(0, 4)).toEqual(['dev', 'helm', 'timeclick', 'TimeClick-Builder'])
    expect(highlighted()).toBe('dev')
    expect(props.onShowing).toHaveBeenLastCalledWith([dev.path, helm.path, timeclick.path, builder.path])
  })

  it('narrows to what is typed, marks the match, and starts the first folder with the profile most about it', async () => {
    const props = renderDialog()
    await userEvent.keyboard('time')
    expect(options()).toEqual([
      'timeclick',
      'Resume payroll export fix',
      'Resume named breaks rollout',
      'TimeClick-Builder'
    ])
    const first = within(list()).getByRole('option', { name: 'timeclick' })
    expect(first.querySelector('.text-accent-text')?.textContent).toBe('time')

    expect(picker('Profile').selectedOptions[0]?.textContent).toBe('Timeclick')
    expect(picker('Permissions').value).toBe('bypassPermissions')
    expect(sentence()).toBe(
      'Runs claude in ~\\.harness\\dev\\repos\\timeclick with the Timeclick profile, composing dev and timeclick, and --model opus --permission-mode bypassPermissions.'
    )

    await userEvent.keyboard('{Enter}')
    expect(props.onStart).toHaveBeenCalledWith({
      project: timeclick,
      profileId: timeclickProfile.id,
      permissionMode: 'bypassPermissions',
      resume: null,
      beside: false,
      agent: null,
      backend: null,
      // What the Model and Effort pickers show is what is sent: the profile's.
      chosen: { model: 'opus', effort: null }
    })
  })

  it('walks into a folder’s conversations with the arrows and reopens one', async () => {
    const props = renderDialog()
    await userEvent.keyboard('timeclick{ArrowDown}{ArrowDown}')
    expect(highlighted()).toBe('Resume named breaks rollout')
    expect(within(list()).getByRole('option', { name: 'Resume named breaks rollout' }).textContent).toContain(
      '4d'
    )
    expect(sentence()).toMatch(/^Reopens “named breaks rollout” with claude --resume in ~\\\.harness/)

    await userEvent.keyboard('{ArrowDown}{ArrowDown}{ArrowUp}{ArrowUp}{ArrowUp}{ArrowUp}{ArrowUp}')
    expect(highlighted()).toBe('timeclick')
    await userEvent.keyboard('{ArrowDown}{Enter}')
    expect(props.onStart).toHaveBeenCalledWith(
      expect.objectContaining({ project: timeclick, resume: payroll, profileId: timeclickProfile.id })
    )
  })

  it('shows conversations only under the highlighted folder, which moves them as it moves', async () => {
    renderDialog({ initialPath: helm.path })
    expect(options()).not.toContain('Resume payroll export fix')
    await userEvent.keyboard('{ArrowDown}')
    expect(highlighted()).toBe('timeclick')
    expect(options().slice(0, 4)).toEqual([
      'helm',
      'timeclick',
      'Resume payroll export fix',
      'Resume named breaks rollout'
    ])
  })

  it('keeps a profile that was picked for whatever folder is highlighted next, and the mode follows it until changed', async () => {
    const props = renderDialog({ initialPath: helm.path })
    expect(picker('Profile').value).toBe('')
    expect(picker('Permissions').value).toBe('')
    expect(sentence()).toBe('Runs claude in ~\\.harness\\dev\\repos\\helm.')

    await userEvent.selectOptions(picker('Profile'), 'dev')
    expect(picker('Permissions').value).toBe('auto')
    await userEvent.click(within(list()).getByRole('option', { name: 'timeclick' }))
    expect(picker('Profile').selectedOptions[0]?.textContent).toBe('dev')

    await userEvent.selectOptions(picker('Permissions'), 'Plan')
    await userEvent.selectOptions(picker('Profile'), 'Timeclick')
    expect(picker('Permissions').value).toBe('plan')

    // Enter from a picker starts it too, with what the pickers say.
    picker('Profile').focus()
    await userEvent.keyboard('{Enter}')
    expect(props.onStart).toHaveBeenLastCalledWith(
      expect.objectContaining({ project: timeclick, profileId: timeclickProfile.id, permissionMode: 'plan' })
    )

    await userEvent.selectOptions(picker('Profile'), 'No profile')
    await userEvent.selectOptions(picker('Permissions'), 'Default')
    await userEvent.click(within(dialog()).getByRole('button', { name: /Start$/ }))
    expect(props.onStart).toHaveBeenLastCalledWith(
      expect.objectContaining({ profileId: null, permissionMode: null, beside: false })
    )
  })

  it('starts beside with Ctrl+Enter or its button, and only highlights on a click', async () => {
    const props = renderDialog()
    await userEvent.click(within(list()).getByRole('option', { name: 'TimeClick-Builder' }))
    expect(props.onStart).not.toHaveBeenCalled()
    expect(highlighted()).toBe('TimeClick-Builder')
    // The click left the field focused, so the keyboard carries on from there.
    await userEvent.keyboard('{Control>}{Enter}{/Control}')
    expect(props.onStart).toHaveBeenLastCalledWith(expect.objectContaining({ project: builder, beside: true }))

    await userEvent.click(within(dialog()).getByRole('button', { name: /Start beside/ }))
    expect(props.onStart).toHaveBeenCalledTimes(2)

    await userEvent.dblClick(within(list()).getByRole('option', { name: 'dev' }))
    expect(props.onStart).toHaveBeenLastCalledWith(
      expect.objectContaining({ project: dev, profileId: devProfile.id, beside: false })
    )
  })

  it('opens a terminal in the highlighted folder with Alt+Enter or its button, beside with Ctrl', async () => {
    const onTerminal = vi.fn()
    const props = renderDialog({ onTerminal })
    await userEvent.click(within(list()).getByRole('option', { name: 'TimeClick-Builder' }))
    await userEvent.keyboard('{Alt>}{Enter}{/Alt}')
    expect(onTerminal).toHaveBeenLastCalledWith(builder, false)
    await userEvent.keyboard('{Control>}{Alt>}{Enter}{/Alt}{/Control}')
    expect(onTerminal).toHaveBeenLastCalledWith(builder, true)
    await userEvent.click(within(dialog()).getByRole('button', { name: /Terminal$/ }))
    expect(onTerminal).toHaveBeenCalledTimes(3)
    expect(props.onStart).not.toHaveBeenCalled()
  })

  it('offers no terminal when nothing handles one', () => {
    renderDialog()
    expect(within(dialog()).queryByRole('button', { name: /Terminal$/ })).toBeNull()
  })

  it('says what is already running in the folder, and why a launch failed', () => {
    const live = [
      { pid: 41, cwd: timeclick.path.toUpperCase(), name: 'accruals report', helmSessionId: 3 },
      { pid: 42, cwd: timeclick.path, name: null, helmSessionId: null }
    ] as LiveSession[]
    renderDialog({ initialPath: timeclick.path, live, error: 'Claude Code CLI not found.' })

    expect(within(list()).getByRole('option', { name: 'timeclick' }).textContent).toContain('2 running')
    expect(within(dialog()).getByRole('note').textContent).toBe(
      'Already running here: accruals report, pid 42. Another one means two sessions in one working tree.'
    )
    expect(within(dialog()).getByRole('alert').textContent).toBe('Claude Code CLI not found.')
  })

  it('starts nothing while a launch is in flight', async () => {
    const props = renderDialog({ busy: true })
    expect(within(dialog()).getByRole('button', { name: /Starting/ })).toHaveProperty('disabled', true)
    await userEvent.keyboard('{Enter}')
    expect(props.onStart).not.toHaveBeenCalled()
  })

  it('says when nothing matches, and how many more there are past the first few', async () => {
    const many = Array.from({ length: 9 }, (_, i) => project(`repo-${String(i)}`))
    renderDialog({ projects: many })
    expect(within(dialog()).getByText('3 more - keep typing to narrow them')).toBeTruthy()
    await userEvent.keyboard('zzz')
    expect(within(dialog()).getByText('Nothing is called “zzz”.')).toBeTruthy()
    expect(sentence()).toBe('')
  })

  it('closes on Escape and gives focus back to where it was', async () => {
    const before = document.createElement('button')
    document.body.append(before)
    before.focus()
    const props = renderDialog()
    expect(document.activeElement).not.toBe(before)
    await userEvent.keyboard('{Escape}')
    expect(props.onDismiss).toHaveBeenCalledTimes(1)
    expect(document.activeElement).toBe(before)
    before.remove()
  })
})

describe('NewSessionDialog with more than one host connected', () => {
  const HOSTS = [
    { id: 'local', name: 'This PC', remote: false },
    { id: 'h1', name: 'build-box', remote: true }
  ]

  it('names the host it starts on, lists only what it is given for that host, and switches to another', async () => {
    const onHostChange = vi.fn()
    renderDialog({ hosts: HOSTS, host: 'local', onHostChange })

    const switcher = within(dialog()).getByRole('radiogroup', { name: 'Host' })
    expect(within(switcher).getByRole('radio', { name: 'This PC' }).getAttribute('aria-checked')).toBe('true')
    expect(within(switcher).getByRole('radio', { name: 'build-box' }).getAttribute('aria-checked')).toBe('false')
    expect(within(dialog()).getByText(/Start a session on/)).toBeTruthy()

    await userEvent.click(within(switcher).getByRole('radio', { name: 'build-box' }))
    expect(onHostChange).toHaveBeenCalledWith('h1')
  })

  it('names no host and offers no switch with only this PC', () => {
    renderDialog({ hosts: [HOSTS[0]!], host: 'local' })
    expect(within(dialog()).queryByRole('radiogroup', { name: 'Host' })).toBeNull()
    expect(within(dialog()).getByText('Start a session in…')).toBeTruthy()
  })
})

// ---------------------------------------------------------------------------
// The Agent picker (A17-A20, A25)
// ---------------------------------------------------------------------------

function launchAgent(name: string, patch: Partial<LaunchAgent> = {}): LaunchAgent {
  return {
    name,
    description: `${name} does its work.`,
    file: `${name}.md`,
    hash: null,
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
  description: 'Researches a new project with you and writes the plan.',
  models: { 'claude-code': 'opus' },
  efforts: { 'claude-code': 'xhigh' },
  permission: 'plan',
  profile: 'dev',
  allowedTools: ['Read', 'Grep'],
  disallowedTools: ['Edit', 'Write'],
  canStart: true
})
const workerAgent = launchAgent('worker', {
  description: 'Does one piece of work in its own worktree.',
  permission: 'acceptEdits',
  worktree: true,
  disallowedTools: ['mcp__helm-plugin-trackr__create']
})
const docsWriter = launchAgent('docs-writer', { backends: [], listedBackends: ['codex'] })
const CLAUDE_BACKEND = { id: 'claude-code', name: 'Claude Code' }
const AGENTS: LaunchAgents = {
  agents: [docsWriter, researchLead, workerAgent],
  bad: [{ name: 'deploy-check', file: 'deploy-check.md', problem: 'permission must be plan or auto', problems: [], hash: null }],
  backends: [CLAUDE_BACKEND],
  folder: '',
  readOnly: null,
  shipped: []
}
const inGit = (of: Project): Project => ({
  ...of,
  git: { branch: 'main', detached: false, dirty: 0, ahead: 0, behind: 0 }
})

/** The Agent picker's rows: name, second line, and whether it is disabled. */
const agentRows = (): { name: string; detail: string; disabled: boolean }[] =>
  Array.from(picker('Agent').options).map((option) => ({
    name: option.label,
    detail: option.dataset.detail ?? '',
    disabled: option.disabled
  }))

describe('NewSessionDialog: the Agent picker', () => {
  it('comes first at No agent, lists each agent with its description, one that cannot run here and a bad file disabled with why', () => {
    renderDialog({ agents: AGENTS })
    const footer = picker('Agent').closest('div')!
    expect(within(footer).getAllByRole('combobox')[0]).toBe(picker('Agent'))
    expect(picker('Agent').value).toBe('')
    expect(agentRows()).toEqual([
      { name: 'No agent', detail: 'A plain session, as now.', disabled: false },
      { name: 'docs-writer', detail: 'Runs only on codex, which This PC does not have.', disabled: true },
      { name: 'research-lead', detail: 'Researches a new project with you and writes the plan.', disabled: false },
      { name: 'worker', detail: 'Does one piece of work in its own worktree.', disabled: false },
      { name: 'deploy-check', detail: 'Cannot be loaded: permission must be plan or auto', disabled: true }
    ])
    // The bad file is set apart under a rule.
    expect(picker('Agent').querySelector('hr')?.nextElementSibling?.getAttribute('data-agent-option')).toBe(
      'deploy-check'
    )
    expect(dialog().querySelector('footer')?.textContent).toContain('Tab Agent')
    expect(dialog().querySelector('[data-agent-note]')).toBeNull()
  })

  it('fills the pickers with the agent’s values, says whose they are, and starts with them and the agent', async () => {
    const props = renderDialog({ agents: AGENTS, initialPath: timeclick.path })
    await userEvent.selectOptions(picker('Agent'), 'research-lead')
    expect(picker('Profile').selectedOptions[0]?.textContent).toBe('dev')
    expect(picker('Model').value).toBe('opus')
    expect(picker('Effort').value).toBe('xhigh')
    expect(picker('Permissions').value).toBe('plan')
    expect(dialog().querySelector('[data-agent-note]')?.textContent).toBe(
      "Model, effort, permissions and profile are research-lead's. Change any of them before starting."
    )
    expect(sentence()).toBe(
      "Runs claude in ~\\.harness\\dev\\repos\\timeclick as the research-lead agent with the dev profile, composing dev, and --model opus --effort xhigh --permission-mode plan --allowedTools Read,Grep --disallowedTools Edit,Write. Its instructions go in after the project's; nothing is typed into the terminal. It can start agents of its own."
    )
    await userEvent.keyboard('{Enter}')
    expect(props.onStart).toHaveBeenCalledWith({
      project: timeclick,
      profileId: devProfile.id,
      permissionMode: 'plan',
      resume: null,
      beside: false,
      agent: 'research-lead',
      backend: 'claude-code',
      chosen: { model: 'opus', effort: 'xhigh' }
    })
  })

  it('keeps a picker the person changed, and another agent changes only the untouched ones', async () => {
    const props = renderDialog({ agents: AGENTS, initialPath: timeclick.path })
    await userEvent.selectOptions(picker('Effort'), 'low')
    await userEvent.selectOptions(picker('Agent'), 'research-lead')
    expect(picker('Effort').value).toBe('low')
    expect(picker('Permissions').value).toBe('plan')
    expect(dialog().querySelector('[data-agent-note]')?.textContent).toBe(
      "Model, permissions and profile are research-lead's. Change any of them before starting."
    )
    await userEvent.selectOptions(picker('Permissions'), 'Default')
    await userEvent.selectOptions(picker('Agent'), '')
    // Back to the folder's profile and its values, but for what was changed.
    expect(picker('Profile').selectedOptions[0]?.textContent).toBe('Timeclick')
    expect(picker('Model').value).toBe('opus')
    expect(picker('Effort').value).toBe('low')
    expect(picker('Permissions').value).toBe('')
    await userEvent.click(within(dialog()).getByRole('button', { name: /Start$/ }))
    expect(props.onStart).toHaveBeenLastCalledWith(
      expect.objectContaining({ agent: null, permissionMode: null, chosen: { model: 'opus', effort: 'low' } })
    )
  })

  it('offers Model and Effort for every start, and Backend only while two or more backends are installed, narrowed to the agent’s', async () => {
    renderDialog({ agents: { ...AGENTS, backends: [CLAUDE_BACKEND] }, initialPath: timeclick.path })
    expect(picker('Model').value).toBe('opus')
    expect(picker('Effort').value).toBe('')
    expect(within(dialog()).queryByRole('combobox', { name: 'Backend' })).toBeNull()
  })

  it('draws the Backend picker with two backends installed, and an agent narrows it to its own', async () => {
    const OTHER = { id: 'other-agent', name: 'Other Agent' }
    renderDialog({ agents: { ...AGENTS, backends: [CLAUDE_BACKEND, OTHER] } })
    expect(Array.from(picker('Backend').options).map((option) => option.textContent)).toEqual([
      'Claude Code',
      'Other Agent'
    ])
    expect(picker('Backend').value).toBe('claude-code')
    await userEvent.selectOptions(picker('Agent'), 'research-lead')
    // One backend left: there is nothing to choose, so no picker.
    expect(within(dialog()).queryByRole('combobox', { name: 'Backend' })).toBeNull()
  })

  it('names the new worktree of an agent that works in one, and outside git refuses with Start off', async () => {
    const repoFolder = inGit(timeclick)
    const props = renderDialog({ agents: AGENTS, projects: [repoFolder, helm], initialPath: repoFolder.path })
    await userEvent.selectOptions(picker('Agent'), 'worker')
    expect(sentence()).toBe(
      "Runs claude in a new worktree, ~\\.harness\\dev\\repos\\timeclick\\.floti\\worktrees\\timeclick on branch agent/timeclick cut from main, as the worker agent with the Timeclick profile, composing dev and timeclick, and --model opus --permission-mode acceptEdits --disallowedTools mcp__helm-plugin-trackr__create. Its instructions go in after the project's; nothing is typed into the terminal. It gets no floti-agents tools."
    )

    await userEvent.click(within(list()).getByRole('option', { name: 'helm' }))
    expect(dialog().querySelector('[data-launch-sentence]')).toBeNull()
    expect(dialog().querySelector('[data-launch-refused]')?.textContent).toBe(
      'worker works in a worktree of its own, and ~\\.harness\\dev\\repos\\helm is not in a git repository. Pick a folder in one, or No agent.'
    )
    expect(within(dialog()).getByRole('button', { name: /Start$/ })).toHaveProperty('disabled', true)
    expect(within(dialog()).getByRole('button', { name: /Start beside/ })).toHaveProperty('disabled', true)
    await userEvent.keyboard('{Enter}')
    expect(props.onStart).not.toHaveBeenCalled()
  })

  it('switched to a host without the agent goes back to No agent and says so', async () => {
    const hosts = [
      { id: 'local', name: 'This PC', remote: false },
      { id: 'buildbox', name: 'buildbox', remote: true }
    ]
    const props = renderDialog({ agents: AGENTS, hosts, host: 'local' })
    await userEvent.selectOptions(picker('Agent'), 'research-lead')
    expect(picker('Model').value).toBe('opus')
    // buildbox's agents arrive once it is switched to.
    lastView!.rerender(
      <NewSessionDialog {...props} host="buildbox" agents={{ ...AGENTS, agents: [workerAgent] }} />
    )
    expect(picker('Agent').value).toBe('')
    expect(dialog().querySelector('[data-agent-dropped]')?.textContent).toBe(
      'buildbox has no agent called research-lead, so Agent is back to No agent.'
    )
    // What the agent had set goes back to what it was before it.
    expect(picker('Effort').value).toBe('')
    // A host that has it keeps it.
    await userEvent.selectOptions(picker('Agent'), 'worker')
    lastView!.rerender(<NewSessionDialog {...props} host="local" agents={AGENTS} />)
    expect(picker('Agent').value).toBe('worker')
  })
})
