import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import {
  AgentsStep,
  HostStep,
  TourStep,
  WelcomeStep,
  WorkspaceStep,
  type AgentsStepProps,
  type HostStepProps,
  type WalkthroughAgent,
  type WorkspaceStepProps
} from './Walkthrough'

const READY: WalkthroughAgent = {
  id: 'claude-code',
  name: 'Claude Code',
  version: 'claude 2.1.319',
  installed: true,
  signedIn: true,
  install: 'irm https://claude.ai/install.ps1 | iex',
  login: 'claude'
}

const button = (name: string): HTMLElement => screen.getByRole('button', { name })
const progress = (): string[] =>
  [...document.querySelectorAll('[data-walkthrough-progress]')].map(
    (bar) => bar.getAttribute('data-walkthrough-progress') ?? ''
  )

describe('Step 1, Welcome', () => {
  it('says what Floti is, in the design’s words, and offers Skip setup and Get started', async () => {
    const onSkipSetup = vi.fn()
    const onGetStarted = vi.fn()
    render(<WelcomeStep onSkipSetup={onSkipSetup} onGetStarted={onGetStarted} />)
    expect(screen.getByRole('heading', { name: 'Welcome to Floti' })).toBeTruthy()
    expect(screen.getByText('Step 1 of 5')).toBeTruthy()
    expect(progress()).toEqual(['done', 'todo', 'todo', 'todo', 'todo'])
    expect(screen.getByText('Be the captain of your fleet.')).toBeTruthy()
    expect(
      screen.getByText('Run your coding agents side by side, on this PC or your other computers. A few steps and you’re ready.')
    ).toBeTruthy()
    // Get started has the focus, so Enter moves on.
    expect(document.activeElement).toBe(button('Get started'))
    await userEvent.click(button('Skip setup'))
    await userEvent.click(button('Get started'))
    expect(onSkipSetup).toHaveBeenCalledTimes(1)
    expect(onGetStarted).toHaveBeenCalledTimes(1)
  })
})

describe('Step 2, Your agents', () => {
  function renderAgents(overrides: Partial<AgentsStepProps> = {}): AgentsStepProps {
    const props: AgentsStepProps = {
      agents: [READY],
      error: null,
      checking: false,
      onCheckAgain: vi.fn(),
      onCopy: vi.fn(),
      onLocate: vi.fn(),
      locateProblem: null,
      onSkip: vi.fn(),
      onBack: vi.fn(),
      onContinue: vi.fn(),
      ...overrides
    }
    render(<AgentsStep {...props} />)
    return props
  }
  const row = (): HTMLElement => document.querySelector('[data-walkthrough-agent="claude-code"]') as HTMLElement

  it('shows an agent that is installed and signed in with its version, and no Check again', () => {
    renderAgents()
    expect(screen.getByRole('heading', { name: 'Your agents' })).toBeTruthy()
    expect(screen.getByText('Floti runs the agents you already have.')).toBeTruthy()
    expect(within(row()).getByText('Claude Code')).toBeTruthy()
    expect(within(row()).getByText('claude 2.1.319')).toBeTruthy()
    expect(within(row()).getByText('Installed')).toBeTruthy()
    expect(within(row()).getByText('Signed in')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Check again' })).toBeNull()
    expect(document.querySelector('[data-walkthrough-command]')).toBeNull()
  })

  it('names the line that installs a missing agent, with a copy button and Check again', async () => {
    const props = renderAgents({ agents: [{ ...READY, version: null, installed: false, signedIn: 'unknown' }] })
    expect(within(row()).getByText('Not installed')).toBeTruthy()
    expect(within(row()).getByText('irm https://claude.ai/install.ps1 | iex')).toBeTruthy()
    await userEvent.click(screen.getByRole('button', { name: 'Copy irm https://claude.ai/install.ps1 | iex' }))
    expect(props.onCopy).toHaveBeenCalledWith('irm https://claude.ai/install.ps1 | iex')
    await userEvent.click(button('Check again'))
    expect(props.onCheckAgain).toHaveBeenCalledTimes(1)
  })

  it('offers Locate… on the install line of a missing agent, after its copy button', async () => {
    const props = renderAgents({ agents: [{ ...READY, version: null, installed: false, signedIn: 'unknown' }] })
    const line = within(row()).getByText('irm https://claude.ai/install.ps1 | iex').parentElement as HTMLElement
    const locate = within(line).getByRole('button', { name: 'Locate…' })
    expect([...line.querySelectorAll('button')].at(-1)).toBe(locate)
    await userEvent.click(locate)
    expect(props.onLocate).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('offers no Locate… for an agent that is installed, signed out or Unknown', () => {
    const installed = render(<AgentsStep {...renderAgentsProps()} />)
    expect(screen.queryByRole('button', { name: 'Locate…' })).toBeNull()
    installed.unmount()
    const signedOut = render(<AgentsStep {...{ ...renderAgentsProps(), agents: [{ ...READY, signedIn: false }] }} />)
    expect(screen.queryByRole('button', { name: 'Locate…' })).toBeNull()
    signedOut.unmount()
    renderAgents({ agents: [{ ...READY, version: null, installed: 'unknown', signedIn: 'unknown' }] })
    expect(screen.queryByRole('button', { name: 'Locate…' })).toBeNull()
  })

  it('says under the rows why a picked file was not kept, and holds Locate… while a check runs', () => {
    renderAgents({
      agents: [{ ...READY, version: null, installed: false, signedIn: 'unknown' }],
      locateProblem: 'That program did not answer `claude --version`.',
      checking: true
    })
    const problem = screen.getByRole('alert')
    expect(problem.textContent).toBe('That program did not answer `claude --version`.')
    expect(problem.className).toContain('text-warn')
    expect(row().contains(problem)).toBe(false)
    expect((button('Locate…') as HTMLButtonElement).disabled).toBe(true)
  })

  it('names the command that signs in a signed-out agent', () => {
    renderAgents({ agents: [{ ...READY, signedIn: false }] })
    expect(within(row()).getByText('Signed out')).toBeTruthy()
    expect(within(row()).getByText('claude', { selector: 'code' })).toBeTruthy()
    expect(button('Check again')).toBeTruthy()
  })

  it('keeps Unknown as Unknown: a sign-in no check can tell, and a check that could not look', () => {
    const { unmount } = render(
      <AgentsStep {...{ ...renderAgentsProps(), agents: [{ ...READY, signedIn: 'unknown' }] }} />
    )
    expect(screen.getByText('Sign-in unknown')).toBeTruthy()
    expect(screen.queryByText('Signed out')).toBeNull()
    expect(screen.queryByRole('button', { name: 'Check again' })).toBeNull()
    unmount()
    renderAgents({ agents: null, error: 'the host did not answer' })
    expect(screen.getByText('Unknown: the host did not answer')).toBeTruthy()
    expect(screen.queryByText('Not installed')).toBeNull()
    expect(button('Check again')).toBeTruthy()
  })

  it('can be skipped, gone back from and left', async () => {
    const props = renderAgents()
    await userEvent.click(button('Skip'))
    await userEvent.click(button('Back'))
    await userEvent.click(button('Continue'))
    expect(props.onSkip).toHaveBeenCalledTimes(1)
    expect(props.onBack).toHaveBeenCalledTimes(1)
    expect(props.onContinue).toHaveBeenCalledTimes(1)
  })
})

function renderAgentsProps(): AgentsStepProps {
  return {
    agents: [READY],
    error: null,
    checking: false,
    onCheckAgain: vi.fn(),
    onCopy: vi.fn(),
    onLocate: vi.fn(),
    locateProblem: null,
    onSkip: vi.fn(),
    onBack: vi.fn(),
    onContinue: vi.fn()
  }
}

describe('Step 3, The host on this computer', () => {
  function renderHost(overrides: Partial<HostStepProps> = {}): HostStepProps {
    const props: HostStepProps = {
      host: { state: 'running', version: '3.0.0' },
      quit: 'end',
      onQuit: vi.fn(),
      others: [],
      onAddHost: vi.fn(),
      onSkip: vi.fn(),
      onBack: vi.fn(),
      onContinue: vi.fn(),
      ...overrides
    }
    render(<HostStep {...props} />)
    return props
  }

  it('says the host is running, and that quitting ends this computer’s sessions unless set otherwise', async () => {
    const props = renderHost()
    expect(screen.getByRole('heading', { name: 'The host on this computer' })).toBeTruthy()
    expect(screen.getByText('Your sessions run in the background on this PC.')).toBeTruthy()
    const host = document.querySelector('[data-walkthrough-host="local"]') as HTMLElement
    expect(within(host).getByText('This PC')).toBeTruthy()
    expect(within(host).getByText('floti 3.0.0')).toBeTruthy()
    expect(within(host).getByText('Running')).toBeTruthy()

    const quit = screen.getByRole('radiogroup', { name: 'When you quit Floti' })
    const end = within(quit).getByRole('radio', { name: /End my sessions/ })
    expect(end.getAttribute('aria-checked')).toBe('true')
    expect(end.textContent).toContain('Default')
    expect(end.textContent).toContain('Like closing a terminal.')
    const keep = within(quit).getByRole('radio', { name: /Keep them running/ })
    expect(keep.textContent).toContain('Pick them up when you open Floti again.')
    expect(screen.getByText('You can change this in Settings.')).toBeTruthy()

    await userEvent.click(keep)
    expect(props.onQuit).toHaveBeenCalledWith('keep')
  })

  it('offers Add a host…, and lists a host added beside this PC', async () => {
    const props = renderHost({ others: [{ id: 'h1', name: 'build-box', quit: 'keep' }] })
    expect(screen.getByText('Optional: run sessions on a server or another PC.')).toBeTruthy()
    await userEvent.click(button('Add a host…'))
    expect(props.onAddHost).toHaveBeenCalledTimes(1)
    const added = document.querySelector('[data-walkthrough-other-host="h1"]') as HTMLElement
    expect(within(added).getByText('build-box')).toBeTruthy()
  })

  it('says the host is starting, or Unknown with why, until it answers', () => {
    const { unmount } = render(
      <HostStep
        host={{ state: 'starting' }}
        quit="end"
        onQuit={vi.fn()}
        others={[]}
        onAddHost={vi.fn()}
        onSkip={vi.fn()}
        onBack={vi.fn()}
        onContinue={vi.fn()}
      />
    )
    expect(screen.getByText('Starting…')).toBeTruthy()
    unmount()
    renderHost({ host: { state: 'unknown', reason: 'The host did not start.' } })
    expect(screen.getByText('Unknown')).toBeTruthy()
    expect(screen.getByText('The host did not start.')).toBeTruthy()
  })
})

describe('Step 4, A workspace', () => {
  function renderWorkspace(overrides: Partial<WorkspaceStepProps> = {}): WorkspaceStepProps {
    const props: WorkspaceStepProps = {
      choice: 'new',
      onChoice: vi.fn(),
      name: 'my-work',
      onName: vi.fn(),
      parent: 'C:\\Users\\you\\Projects',
      onChooseParent: vi.fn(),
      folder: '',
      onChooseFolder: vi.fn(),
      folders: [],
      onAddFolders: vi.fn(),
      problems: [],
      busy: false,
      onSkip: vi.fn(),
      onBack: vi.fn(),
      onSubmit: vi.fn(),
      ...overrides
    }
    render(<WorkspaceStep {...props} />)
    return props
  }

  it('offers the three ways in the design’s words, creating a new one from a name and a folder', async () => {
    const props = renderWorkspace()
    expect(screen.getByRole('heading', { name: 'A workspace' })).toBeTruthy()
    expect(screen.getByText('A folder for your repositories and the instructions agents follow there.')).toBeTruthy()
    const choices = within(screen.getByRole('radiogroup', { name: 'A workspace' })).getAllByRole('radio')
    expect(choices.map((choice) => choice.textContent)).toEqual([
      'Create a new oneA fresh folder, ready for your repositories.',
      'Use a folder I haveNothing in it moves or changes.',
      'Find the ones I havePoint Floti at where they are.'
    ])
    expect(choices[0]?.getAttribute('aria-checked')).toBe('true')
    expect(screen.getByLabelText('Create it inside')).toHaveProperty('value', 'C:\\Users\\you\\Projects')
    await userEvent.click(button('Create workspace'))
    expect(props.onSubmit).toHaveBeenCalledTimes(1)
    await userEvent.click(choices[2] as HTMLElement)
    expect(props.onChoice).toHaveBeenCalledWith('find')
  })

  it('cannot create without a name, and cannot use a folder before one is chosen', () => {
    const { unmount } = render(<WorkspaceStep {...renderWorkspaceProps({ name: '  ' })} />)
    expect(button('Create workspace')).toHaveProperty('disabled', true)
    unmount()
    renderWorkspace({ choice: 'convert' })
    expect(button('Use this folder')).toHaveProperty('disabled', true)
  })

  it('lists the folders to scan once chosen, and says what went wrong', () => {
    renderWorkspace({ choice: 'find', folders: ['C:\\src'], problems: ['C:\\src is not there any more.'] })
    expect(document.querySelector('[data-walkthrough-scan-folder="C:\\\\src"]')).toBeTruthy()
    expect(button('Continue')).toHaveProperty('disabled', false)
    expect(screen.getByRole('alert').textContent).toBe('C:\\src is not there any more.')
  })
})

function renderWorkspaceProps(overrides: Partial<WorkspaceStepProps>): WorkspaceStepProps {
  return {
    choice: 'new',
    onChoice: vi.fn(),
    name: 'my-work',
    onName: vi.fn(),
    parent: 'C:\\work',
    onChooseParent: vi.fn(),
    folder: '',
    onChooseFolder: vi.fn(),
    folders: [],
    onAddFolders: vi.fn(),
    problems: [],
    busy: false,
    onSkip: vi.fn(),
    onBack: vi.fn(),
    onSubmit: vi.fn(),
    ...overrides
  }
}

describe('Step 5, Around the window', () => {
  it('names each place on the rail and History on Ctrl+H, a few words each', () => {
    render(<TourStep onSkip={vi.fn()} onBack={vi.fn()} onContinue={vi.fn()} />)
    expect(screen.getByRole('heading', { name: 'Around the window' })).toBeTruthy()
    expect(screen.getByText('A quick look around.')).toBeTruthy()
    expect(
      [...document.querySelectorAll('[data-walkthrough-tour]')].map((place) => place.textContent)
    ).toEqual([
      'SessionsWorkspaces and running agents',
      'ProfilesSaved launches',
      'AgentsRoles with their own instructions',
      'FilesNotes and docs, rendered',
      'BrowserPages for you and your agents',
      'PluginsPanels and tools you add',
      'SettingsHosts, themes and this tour',
      'HistoryCtrl+HFind and resume past sessions'
    ])
    expect(screen.getByText('Step 5 of 5')).toBeTruthy()
    expect(progress()).toEqual(['done', 'done', 'done', 'done', 'done'])
  })

  it('hands Skip, Back and Continue to the desktop', async () => {
    const onSkip = vi.fn()
    const onBack = vi.fn()
    const onContinue = vi.fn()
    render(<TourStep onSkip={onSkip} onBack={onBack} onContinue={onContinue} />)
    await userEvent.click(button('Continue'))
    await userEvent.click(button('Back'))
    await userEvent.click(button('Skip'))
    expect([onContinue, onBack, onSkip].map((handler) => handler.mock.calls.length)).toEqual([1, 1, 1])
  })
})
