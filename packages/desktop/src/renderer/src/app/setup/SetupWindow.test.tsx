import { act, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { DEFAULT_SETTINGS, type AppSettings, type Walkthrough } from '@helm/core/types'
import type { ClaudeStatus, LocalHostStep } from '../../../../shared/ipc'
import { bridge } from '../bridge.testkit'
import type { LocalHostStepStore } from '../host/localHostStep'
import { SetupWindow } from './SetupWindow'
import { claudeAgent } from './walkthrough'

vi.mock('../bridge', () => import('../bridge.testkit'))

/**
 * The setup window as main opens it on a first start: steps 1 to 4, each move
 * written to the `walkthrough` setting, and the hand-over to the shell.
 */

const PATIENCE = { timeout: 2000 }

const CLAUDE: ClaudeStatus = {
  path: 'C:\\Users\\you\\.local\\bin\\claude.exe',
  source: 'discovered',
  version: '2.1.319 (Claude Code)',
  semver: '2.1.319',
  tested: true,
  testedRange: { min: '2.1.277', max: '2.2.0' },
  configDir: 'C:\\Users\\you\\.claude',
  configDirExists: true,
  auth: 'authenticated',
  authSignal: '.credentials.json exists',
  error: null
}

/** Main's steps for this PC's host, held still at `step`. */
function hostAt(step: LocalHostStep | null): LocalHostStepStore {
  return { get: () => step, subscribe: () => () => undefined }
}

let settings: AppSettings

function openAt(walkthrough: Walkthrough | null, patch: Partial<AppSettings> = {}): void {
  settings = { ...DEFAULT_SETTINGS, walkthrough, ...patch }
  bridge.answer('settings:read', () => settings)
  bridge.answer('settings:write', (written) => {
    settings = { ...settings, ...written }
    return settings
  })
  render(<SetupWindow bridge={bridge} hostStep={hostAt({ step: 'ready', since: 0 })} />)
}

const written = (): Array<Partial<AppSettings>> => bridge.invoked('settings:write')
const lastStep = (): Walkthrough | null | undefined =>
  written()
    .filter((patch) => 'walkthrough' in patch)
    .at(-1)?.walkthrough

beforeEach(() => {
  bridge.reset()
  bridge.answer('setup:status', () => CLAUDE)
  bridge.answer('app:info', () => ({ version: '3.0.0', windowsBuild: 26100, home: 'C:\\Users\\you' }) as never)
  bridge.answer('roots:suggest', () => ['C:\\Users\\you\\Projects'])
  bridge.answer('setup:complete', () => settings)
  bridge.answer('walkthrough:openShell', () => undefined)
})

describe('the setup window', () => {
  it('opens on the step it was closed on, and writes every move so closing resumes there', async () => {
    openAt({ step: 3, workspace: null })
    expect(await screen.findByRole('heading', { name: 'The host on this computer' })).toBeTruthy()
    await userEvent.click(screen.getByRole('button', { name: 'Back' }))
    expect(await screen.findByRole('heading', { name: 'Your agents' })).toBeTruthy()
    await waitFor(() => expect(lastStep()).toEqual({ step: 2, workspace: null }), PATIENCE)
    await userEvent.click(screen.getByRole('button', { name: 'Skip' }))
    await waitFor(() => expect(lastStep()).toEqual({ step: 3, workspace: null }), PATIENCE)
  })

  it('skips setup on step 1: done, stamped on the host, and the shell opens with nothing set up', async () => {
    openAt({ step: 1, workspace: null })
    await userEvent.click(await screen.findByRole('button', { name: 'Skip setup' }))
    await waitFor(() => expect(bridge.invoked('walkthrough:openShell')).toHaveLength(1), PATIENCE)
    expect(lastStep()).toBeNull()
    expect(bridge.invoked('setup:complete')).toHaveLength(1)
    expect(bridge.invoked('workspace:create')).toEqual([])
    expect(bridge.invoked('roots:accept')).toEqual([])
  })

  it('reads the agents through setup:status and names the Windows install line for one that is missing', async () => {
    bridge.answer('setup:status', () => ({ ...CLAUDE, path: null, version: null, semver: null, auth: 'unknown' }))
    openAt({ step: 2, workspace: null })
    expect(await screen.findByText('Not installed')).toBeTruthy()
    expect(await screen.findByText('irm https://claude.ai/install.ps1 | iex')).toBeTruthy()
    // It reads no file of its own: the host's check is all it asks.
    expect(bridge.invocations.map((call) => call.channel)).not.toContain('files:read')
  })

  it('says the host is running once main pushes its ready step, after starting', async () => {
    let step: LocalHostStep | null = { step: 'starting', since: 0 }
    const listeners = new Set<() => void>()
    const store: LocalHostStepStore = {
      get: () => step,
      subscribe: (listener) => {
        listeners.add(listener)
        return () => listeners.delete(listener)
      }
    }
    settings = { ...DEFAULT_SETTINGS, walkthrough: { step: 3, workspace: null } }
    bridge.answer('settings:read', () => settings)
    render(<SetupWindow bridge={bridge} hostStep={store} />)
    expect(await screen.findByText('Starting…')).toBeTruthy()
    expect(bridge.invoked('app:info')).toEqual([])

    act(() => {
      step = { step: 'ready', since: 1 }
      for (const listener of listeners) listener()
    })
    expect(await screen.findByText('Running')).toBeTruthy()
    expect(await screen.findByText('floti 3.0.0')).toBeTruthy()
  })

  it('opens the add-a-host dialog from step 3, and cancelling it adds no host', async () => {
    bridge.answer('remoteHosts:sshHosts', () => ({ found: true, hosts: ['build-box'] }))
    bridge.answer('remoteHosts:wslDistros', () => [])
    openAt({ step: 3, workspace: null })
    await userEvent.click(await screen.findByRole('button', { name: 'Add a host…' }))
    const dialog = await screen.findByRole('dialog', { name: 'Add a host' })
    await userEvent.click(within(dialog).getAllByRole('button', { name: 'Cancel' })[0] as HTMLElement)
    await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Add a host' })).toBeNull(), PATIENCE)
    expect(bridge.invoked('remoteHosts:add')).toEqual([])
    expect(written().some((patch) => 'hosts' in patch)).toBe(false)
    expect(document.querySelector('[data-walkthrough-other-host]')).toBeNull()
  })

  it('sets what quitting does on this PC, End until changed', async () => {
    openAt({ step: 3, workspace: null })
    const end = await screen.findByRole('radio', { name: /End my sessions/ })
    expect(end.getAttribute('aria-checked')).toBe('true')
    expect(await screen.findByText('floti 3.0.0')).toBeTruthy()
    await userEvent.click(screen.getByRole('radio', { name: /Keep them running/ }))
    await waitFor(() => expect(written().at(-1)).toEqual({ quitSessions: { local: 'keep' } }), PATIENCE)
  })

  it('creates the workspace from Starter and hands over at step 5 with it chosen', async () => {
    bridge.answer('workspace:create', () => ({
      path: 'C:\\Users\\you\\Projects\\my-work',
      created: ['workspace.yaml', 'AGENTS.md', 'repos'],
      roots: ['C:\\Users\\you\\Projects\\my-work'],
      problems: []
    }))
    openAt({ step: 4, workspace: null })
    await waitFor(() => expect(screen.getByLabelText('Create it inside')).toHaveProperty('value', 'C:\\Users\\you\\Projects'), PATIENCE)
    await userEvent.click(screen.getByRole('button', { name: 'Create workspace' }))
    await waitFor(() => expect(bridge.invoked('walkthrough:openShell')).toHaveLength(1), PATIENCE)
    expect(bridge.invoked('workspace:create')).toEqual([
      { mode: 'new', dir: 'C:\\Users\\you\\Projects', name: 'my-work', template: 'starter' }
    ])
    expect(lastStep()).toEqual({ step: 5, workspace: 'C:\\Users\\you\\Projects\\my-work' })
    expect(bridge.invoked('setup:complete')).toHaveLength(1)
  })

  it('stays on step 4 and says why when the workspace cannot be made', async () => {
    bridge.answer('workspace:create', () => ({ path: null, created: [], roots: [], problems: ['my-work is already there.'] }))
    openAt({ step: 4, workspace: null })
    await waitFor(() => expect(screen.getByLabelText('Create it inside')).toHaveProperty('value', 'C:\\Users\\you\\Projects'), PATIENCE)
    await userEvent.click(screen.getByRole('button', { name: 'Create workspace' }))
    expect((await screen.findByRole('alert')).textContent).toBe('my-work is already there.')
    expect(bridge.invoked('walkthrough:openShell')).toEqual([])
  })

  it('adds the folders to scan for Find the ones I have, and hands over with no workspace chosen', async () => {
    bridge.answer('path:chooseDirectory', () => ({ path: 'D:\\src' }))
    bridge.answer('roots:accept', () => ['D:\\src'])
    openAt({ step: 4, workspace: null })
    await userEvent.click(await screen.findByRole('radio', { name: /Find the ones I have/ }))
    await userEvent.click(screen.getByRole('button', { name: 'Choose…' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Continue' }))
    await waitFor(() => expect(bridge.invoked('walkthrough:openShell')).toHaveLength(1), PATIENCE)
    expect(bridge.invoked('roots:accept')).toEqual([{ path: 'D:\\src' }])
    expect(lastStep()).toEqual({ step: 5, workspace: null })
  })

  it('hands over at once when opened with no step for it to show', async () => {
    openAt(null)
    await waitFor(() => expect(bridge.invoked('walkthrough:openShell')).toHaveLength(1), PATIENCE)
    expect(written()).toEqual([])
  })
})

describe('claudeAgent', () => {
  it('reads installed from the version command and signed in from the artefacts, Unknown kept Unknown', () => {
    expect(claudeAgent(CLAUDE, true)).toMatchObject({ installed: true, signedIn: true, version: 'claude 2.1.319' })
    expect(claudeAgent({ ...CLAUDE, auth: 'unauthenticated' }, false)).toMatchObject({ signedIn: false, login: 'claude' })
    expect(claudeAgent({ ...CLAUDE, auth: 'unknown' }, false).signedIn).toBe('unknown')
    expect(claudeAgent({ ...CLAUDE, path: null, version: null }, false)).toMatchObject({
      installed: false,
      version: null,
      install: 'curl -fsSL https://claude.ai/install.sh | bash'
    })
  })
})
