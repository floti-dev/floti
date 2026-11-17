import { fireEvent, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { EMPTY_INVENTORY, type DiscoveryResult, type Project } from '@floti/core/types'
import {
  SessionTree,
  sessionTreeHostProps,
  type SessionTreeProps, type TreeHost, type TreeHostActions, type TreeSession } from './SessionTree'

const project = (path: string, workspacePath: string | null, kind: Project['kind'] = 'repo'): Project => ({
  path,
  name: path.split(/[\\/]/).pop() ?? path,
  kind,
  workspacePath,
  hasClaudeDir: false,
  inventory: EMPTY_INVENTORY,
  git: null
})

const discovery = (roots: string[], workspace: string, repos: string[]): DiscoveryResult => ({
  roots,
  workspaces: [{ path: workspace, name: workspace.split(/[\\/]/).pop() ?? workspace, template: null, version: null, repoPaths: repos }],
  projects: [project(workspace, workspace, 'workspace'), ...repos.map((repo) => project(repo, workspace))],
  errors: [],
  scannedAt: '2026-11-03T00:00:00.000Z',
  durationMs: 1
})

const TIMED_OUT = 'ssh: connect to host build-box port 22: Connection timed out'

const THIS_PC = discovery(['C:\\work'], 'C:\\work\\my-work', ['C:\\work\\my-work\\repos\\notes'])
const BUILD_BOX = discovery(['/home/you/work'], '/home/you/work/infra', ['/home/you/work/infra/repos/api'])

const session = (id: string, label: string): TreeSession => ({
  id,
  label,
  state: 'idle',
  note: '',
  hint: '',
  outside: false,
  shown: null
})

const buildBox = (overrides: Partial<TreeHost> = {}): TreeHost => ({
  id: 'h1',
  name: 'build-box',
  phase: { kind: 'live' },
  discovery: BUILD_BOX,
  scanning: false,
  pinnedPaths: [],
  sessionsByPath: new Map([['/home/you/work/infra/repos/api', [session('host:h1/session:3', 'deploy')]]]),
  elsewhere: [],
  selectedPath: null,
  launchingPath: null,
  ...overrides
})

const actions = (): TreeHostActions => ({
  onSelect: vi.fn(),
  onLaunch: vi.fn(),
  onOpenSession: vi.fn(),
  onTogglePin: vi.fn(),
  onNewSession: vi.fn(),
  onNewWorkspace: vi.fn(),
  onRescan: vi.fn(),
  onAddHost: vi.fn()
})

function renderTree(overrides: Partial<SessionTreeProps> = {}) {
  const props: SessionTreeProps = {
    discovery: THIS_PC,
    scanning: false,
    selectedPath: null,
    pinnedPaths: [],
    onTogglePin: vi.fn(),
    sessionsByPath: new Map([['c:\\work\\my-work\\repos\\notes', [session('session:3', 'my-work')]]]),
    elsewhere: [],
    onSelect: vi.fn(),
    onLaunch: vi.fn(),
    launchingPath: null,
    onOpenSession: vi.fn(),
    onNewSession: vi.fn(),
    ...overrides
  }
  const view = render(<SessionTree {...props} />)
  return { props, view, rerender: (next: Partial<SessionTreeProps>) => view.rerender(<SessionTree {...props} {...next} />) }
}

const group = (container: HTMLElement, host: string): HTMLElement => {
  const found = container.querySelector<HTMLElement>(`[data-host-group="${host}"]`)
  if (found === null) throw new Error(`no group for ${host}`)
  return found
}

describe('SessionTree with hosts on other computers', () => {
  it('with only this PC, lists its projects and sessions as it always has: no host header and no Add a host line', () => {
    const { view } = renderTree({ hosts: [] })
    expect(view.container.querySelector('[data-host-header]')).toBeNull()
    expect(view.container.querySelector('[data-host-group]')).toBeNull()
    expect(screen.queryByRole('button', { name: 'Add a host' })).toBeNull()
    expect(screen.getByTitle('C:\\work\\my-work\\repos\\notes')).toBeTruthy()
    expect(view.container.querySelector('[data-session-row="session:3"]')).not.toBeNull()
  })

  it('lists this PC first, then build-box under its name with its own rows, and ends with Add a host', async () => {
    const hostActions = actions()
    const { view } = renderTree({ hosts: [buildBox()], hostActions, localName: 'This PC', localKey: 'local' })

    const headers = [...view.container.querySelectorAll('[data-host-header]')].map((header) => header.textContent)
    expect(headers[0]).toMatch(/^This PC/)
    expect(headers[1]).toMatch(/^build-box/)
    const local = group(view.container, 'local')
    const remote = group(view.container, 'h1')
    expect(within(local).getByTitle('C:\\work\\my-work\\repos\\notes')).toBeTruthy()
    expect(within(local).queryByTitle('/home/you/work/infra/repos/api')).toBeNull()
    expect(within(remote).getByTitle('/home/you/work/infra/repos/api')).toBeTruthy()
    expect(within(remote).getByRole('button', { name: /^deploy, / })).toBeTruthy()
    // The host's name is machine data, and a remote host's is set in mono.
    expect(remote.querySelector('[data-host-header] .font-mono')?.textContent).toBe('build-box')

    const nav = screen.getByRole('navigation', { name: 'Projects and sessions' })
    const last = [...nav.children].at(-1)
    expect(last?.textContent).toBe('Add a host')
    await userEvent.click(screen.getByRole('button', { name: 'Add a host' }))
    expect(hostActions.onAddHost).toHaveBeenCalledOnce()

    await userEvent.click(within(remote).getByRole('button', { name: 'Start a session in api' }))
    expect(hostActions.onLaunch).toHaveBeenCalledWith('h1', expect.objectContaining({ path: '/home/you/work/infra/repos/api' }))
    await userEvent.click(within(remote).getByRole('button', { name: /^deploy, / }))
    expect(hostActions.onOpenSession).toHaveBeenCalledWith('h1', 'host:h1/session:3')
  })

  it('filters rows in every group', async () => {
    const { view } = renderTree({ hosts: [buildBox()], hostActions: actions() })
    await userEvent.type(screen.getByRole('textbox', { name: 'Filter projects and sessions' }), 'deploy')
    expect(within(group(view.container, 'h1')).getByTitle('/home/you/work/infra/repos/api')).toBeTruthy()
    expect(within(group(view.container, 'local')).queryByTitle('C:\\work\\my-work\\repos\\notes')).toBeNull()

    await userEvent.clear(screen.getByRole('textbox', { name: 'Filter projects and sessions' }))
    await userEvent.type(screen.getByRole('textbox', { name: 'Filter projects and sessions' }), 'notes')
    expect(within(group(view.container, 'local')).getByTitle('C:\\work\\my-work\\repos\\notes')).toBeTruthy()
    expect(within(group(view.container, 'h1')).queryByTitle('/home/you/work/infra/repos/api')).toBeNull()
  })

  it('says build-box is reconnecting after a drop, keeps its last rows dimmed, and shows them live once it is back', () => {
    const { view, rerender } = renderTree({ hosts: [buildBox({ phase: { kind: 'reconnecting' } })], hostActions: actions() })
    const remote = (): HTMLElement => group(view.container, 'h1')
    const header = (): HTMLElement => remote().querySelector('[data-host-header]') as HTMLElement
    expect(header().getAttribute('data-host-state')).toBe('reconnecting')
    const word = within(header()).getByText('reconnecting')
    expect(word.className).toContain('text-warn')
    expect(word.getAttribute('title')).toBe('The connection to build-box dropped; trying again. Its sessions there run on.')
    expect(remote().querySelector('[data-host-phase="reconnecting"]')?.textContent).toBe('Reconnecting to build-box…')
    expect(remote().querySelector('[data-host-rows]')?.getAttribute('data-host-rows')).toBe('dimmed')
    expect(within(remote()).getByTitle('/home/you/work/infra/repos/api')).toBeTruthy()
    expect(within(remote()).getByRole('button', { name: /^deploy, / })).toBeTruthy()

    rerender({ hosts: [buildBox({ phase: { kind: 'live' } })] })
    expect(header().getAttribute('data-host-state')).toBe('live')
    expect(header().querySelector('[data-host-phase-word]')).toBeNull()
    expect(remote().querySelector('[data-host-phase]')).toBeNull()
    expect(remote().querySelector('[data-host-rows]')?.getAttribute('data-host-rows')).toBe('live')
  })

  it('says build-box is connecting before anything is heard from it, with no empty state under it', () => {
    const { view } = renderTree({
      hosts: [buildBox({ phase: { kind: 'connecting' }, discovery: null, sessionsByPath: new Map() })],
      hostActions: actions()
    })
    const remote = group(view.container, 'h1')
    const header = remote.querySelector('[data-host-header]') as HTMLElement
    expect(header.getAttribute('data-host-state')).toBe('connecting')
    expect(within(header).getByText('connecting').className).toContain('text-fg-subtle')
    expect(remote.querySelector('[data-host-phase="connecting"]')?.textContent).toBe('Connecting to build-box…')
    expect(remote.querySelector('[data-empty-state]')).toBeNull()
    expect(within(remote).queryByText(/No workspaces on/)).toBeNull()
    expect(within(remote).queryByText('Scanning…')).toBeNull()
  })

  it('says why a connecting host waits when there is more to say', () => {
    const { view } = renderTree({
      hosts: [
        buildBox({
          phase: { kind: 'connecting', note: 'Its floti starts when somebody next signs in there.' },
          discovery: null,
          sessionsByPath: new Map()
        })
      ],
      hostActions: actions()
    })
    const remote = group(view.container, 'h1')
    expect(remote.querySelector('[data-host-phase="connecting"]')?.textContent).toBe(
      'Connecting to build-box…Its floti starts when somebody next signs in there.'
    )
    expect(within(remote).getByText('connecting').getAttribute('title')).toBe(
      'Connecting to build-box: Its floti starts when somebody next signs in there.'
    )
  })

  it('says build-box’s sessions are loading once connected, over the projects it has cached, with no empty state', () => {
    const { view, rerender } = renderTree({
      hosts: [buildBox({ phase: { kind: 'loading' }, sessionsByPath: new Map() })],
      hostActions: actions()
    })
    const remote = (): HTMLElement => group(view.container, 'h1')
    const header = (): HTMLElement => remote().querySelector('[data-host-header]') as HTMLElement
    expect(header().getAttribute('data-host-state')).toBe('loading')
    expect(within(header()).getByText('loading').getAttribute('title')).toBe('Connected to build-box; reading its sessions')
    expect(remote().querySelector('[data-host-phase="loading"]')?.textContent).toBe('Loading sessions…')
    // What it has read of its folders is real and stays readable.
    expect(remote().querySelector('[data-host-rows]')?.getAttribute('data-host-rows')).toBe('live')
    expect(within(remote()).getByTitle('/home/you/work/infra/repos/api')).toBeTruthy()

    // A host whose folders hold nothing still says loading, never that it has nothing.
    const empty: DiscoveryResult = { ...BUILD_BOX, workspaces: [], projects: [] }
    rerender({ hosts: [buildBox({ phase: { kind: 'loading' }, discovery: empty, sessionsByPath: new Map() })] })
    expect(remote().querySelector('[data-host-phase="loading"]')).not.toBeNull()
    expect(remote().querySelector('[data-empty-state]')).toBeNull()

    rerender({ hosts: [buildBox({ phase: { kind: 'live' }, discovery: empty, sessionsByPath: new Map() })] })
    expect(remote().querySelector('[data-host-phase]')).toBeNull()
    expect(remote().querySelector('[data-empty-state="host-empty:h1"]')).not.toBeNull()
  })

  it('says build-box can’t be reached yet in its place, that Floti keeps trying, with its link’s last line and Try again', async () => {
    const hostActions = { ...actions(), onReconnect: vi.fn() }
    const { view } = renderTree({
      hosts: [
        buildBox({
          phase: { kind: 'unreachable', failure: { reason: 'Can’t reach build-box', lastLine: TIMED_OUT, retrying: true } },
          discovery: null,
          sessionsByPath: new Map()
        })
      ],
      hostActions
    })
    const remote = group(view.container, 'h1')
    const header = remote.querySelector('[data-host-header]') as HTMLElement
    expect(header.getAttribute('data-host-state')).toBe('unreachable')
    const word = within(header).getByText('unreachable')
    expect(word.className).toContain('text-danger')
    expect(word.getAttribute('title')).toBe('Can’t reach build-box. Floti keeps trying.')
    const line = remote.querySelector('[data-host-phase="unreachable"]') as HTMLElement
    // All of the line is the plain fg-subtle of "Scanning…".
    expect(line.className).toContain('text-fg-subtle')
    expect(line.querySelector('.text-fg-muted')).toBeNull()
    expect(line.querySelector('[data-host-reason]')?.textContent).toBe('Can’t reach build-box. Floti keeps trying.')
    expect(line.querySelector('[data-host-last-line]')?.textContent).toBe(TIMED_OUT)
    expect(remote.querySelector('[data-empty-state]')).toBeNull()
    await userEvent.click(within(line).getByRole('button', { name: 'Try again' }))
    expect(hostActions.onReconnect).toHaveBeenCalledWith('h1')
    expect(hostActions.onRescan).not.toHaveBeenCalled()
  })

  it('offers no Try again, and does not say Floti keeps trying, where only the user can fix what stops a host', () => {
    const { view } = renderTree({
      hosts: [
        buildBox({
          phase: {
            kind: 'unreachable',
            failure: { reason: 'build-box proved another key than the one this PC pinned for it', retrying: false }
          }
        })
      ],
      hostActions: { ...actions(), onReconnect: vi.fn() }
    })
    const remote = group(view.container, 'h1')
    const line = remote.querySelector('[data-host-phase="unreachable"]') as HTMLElement
    expect(line.querySelector('[data-host-reason]')?.textContent).toBe(
      'build-box proved another key than the one this PC pinned for it.'
    )
    expect(line.querySelector('[data-host-last-line]')).toBeNull()
    expect(within(line).queryByRole('button', { name: 'Try again' })).toBeNull()
    // The line is not inside the dimmed rows: it is what the host says now.
    expect(line.closest('[data-host-rows]')).toBeNull()
    expect(remote.querySelector('[data-host-rows]')?.getAttribute('data-host-rows')).toBe('dimmed')
    expect(within(remote).getByRole('button', { name: /^deploy, / })).toBeTruthy()
  })

  it('keeps saying build-box is reconnecting while its retries fail after it was live, with why, its last line and Try again', async () => {
    const hostActions = { ...actions(), onReconnect: vi.fn() }
    const { view } = renderTree({
      hosts: [
        buildBox({
          phase: {
            kind: 'reconnecting',
            failure: { reason: 'Can’t reach build-box', lastLine: TIMED_OUT, retrying: true }
          }
        })
      ],
      hostActions
    })
    const remote = group(view.container, 'h1')
    const header = remote.querySelector('[data-host-header]') as HTMLElement
    expect(header.getAttribute('data-host-state')).toBe('reconnecting')
    expect(within(header).queryByText('unreachable')).toBeNull()
    expect(within(header).getByText('reconnecting').className).toContain('text-warn')
    const line = remote.querySelector('[data-host-phase="reconnecting"]') as HTMLElement
    expect(line.className).toContain('text-fg-subtle')
    expect(within(line).getByText('Reconnecting to build-box…')).toBeTruthy()
    expect(line.querySelector('[data-host-reason]')?.textContent).toBe('Can’t reach build-box.')
    expect(line.querySelector('[data-host-last-line]')?.textContent).toBe(TIMED_OUT)
    expect(remote.querySelector('[data-host-rows]')?.getAttribute('data-host-rows')).toBe('dimmed')
    await userEvent.click(within(line).getByRole('button', { name: 'Try again' }))
    expect(hostActions.onReconnect).toHaveBeenCalledWith('h1')
  })

  it('says Unknown, never loading for ever, when build-box’s sessions could not be listed, and Try again lists them again', async () => {
    const hostActions = { ...actions(), onRelist: vi.fn(), onReconnect: vi.fn() }
    const { view } = renderTree({
      hosts: [buildBox({ phase: { kind: 'unknown', reason: 'The request timed out.' }, sessionsByPath: new Map() })],
      hostActions
    })
    const remote = group(view.container, 'h1')
    const header = remote.querySelector('[data-host-header]') as HTMLElement
    expect(header.getAttribute('data-host-state')).toBe('unknown')
    expect(within(header).getByText('unknown').className).toContain('text-warn')
    expect(remote.querySelector('[data-host-phase="loading"]')).toBeNull()
    const unknown = remote.querySelector('[data-empty-state="host-unlisted:h1"]') as HTMLElement
    expect(within(unknown).getByText('Unknown')).toBeTruthy()
    expect(within(unknown).getByText('Floti could not list the sessions on build-box. The request timed out.')).toBeTruthy()
    // Its folders are real and stay readable.
    expect(remote.querySelector('[data-host-rows]')?.getAttribute('data-host-rows')).toBe('live')
    await userEvent.click(within(unknown).getByRole('button', { name: 'Try again' }))
    expect(hostActions.onRelist).toHaveBeenCalledWith('h1')
    expect(hostActions.onReconnect).not.toHaveBeenCalled()
  })

  it('says Unknown for this PC when its sessions could not be listed, and Try again lists them again', async () => {
    const onRelistSessions = vi.fn()
    const none: DiscoveryResult = { ...THIS_PC, roots: [], workspaces: [], projects: [] }
    const { view } = renderTree({
      discovery: none,
      sessionsByPath: new Map(),
      sessionsListed: false,
      sessionsListError: 'This PC’s host did not answer.',
      onRelistSessions
    })
    expect(view.container.querySelector('[data-host-phase="loading"]')).toBeNull()
    const unknown = view.container.querySelector('[data-empty-state="host-unlisted:local"]') as HTMLElement
    expect(within(unknown).getByText('Floti could not list the sessions on This PC. This PC’s host did not answer.')).toBeTruthy()
    expect(screen.queryByText('No sessions yet')).toBeNull()
    await userEvent.click(within(unknown).getByRole('button', { name: 'Try again' }))
    expect(onRelistSessions).toHaveBeenCalledOnce()
  })

  it('says this PC’s sessions are loading until its host has listed them, in its group and with no host groups', () => {
    const { view, rerender } = renderTree({
      hosts: [buildBox()],
      hostActions: actions(),
      sessionsListed: false,
      sessionsByPath: new Map()
    })
    const local = (): HTMLElement => group(view.container, 'local')
    expect(local().querySelector('[data-host-header]')?.getAttribute('data-host-state')).toBe('loading')
    expect(local().querySelector('[data-host-phase="loading"]')?.textContent).toBe('Loading sessions…')

    rerender({ sessionsListed: true })
    expect(local().querySelector('[data-host-header]')?.getAttribute('data-host-state')).toBe('live')
    expect(local().querySelector('[data-host-phase]')).toBeNull()

    rerender({ hosts: [], hostActions: undefined, sessionsListed: false })
    expect(view.container.querySelector('[data-host-phase="loading"]')?.textContent).toBe('Loading sessions…')
  })

  it('says this PC’s host is updating, not loading or Unknown, while Floti restarts it, and lists its sessions once it is back', () => {
    const { view, rerender } = renderTree({
      hosts: [buildBox()],
      hostActions: actions(),
      sessionsListed: false,
      sessionsListError: 'This PC’s host did not answer.',
      localHostUpdating: true,
      sessionsByPath: new Map()
    })
    const local = (): HTMLElement => group(view.container, 'local')
    const header = local().querySelector('[data-host-header]') as HTMLElement
    expect(header.getAttribute('data-host-state')).toBe('updating')
    expect(local().querySelector('[data-host-phase="updating"]')?.textContent).toBe(
      'Updating the host on This PC…Its sessions carry over.'
    )
    expect(view.container.querySelector('[data-empty-state="host-unlisted:local"]')).toBeNull()
    expect(view.container.querySelector('[data-host-phase="loading"]')).toBeNull()

    rerender({ sessionsListed: true, sessionsListError: null, localHostUpdating: false })
    expect(local().querySelector('[data-host-header]')?.getAttribute('data-host-state')).toBe('live')
    expect(local().querySelector('[data-host-phase]')).toBeNull()
  })

  it('says this PC’s group waits for its host while the host cannot be used, and fills in once it answers', () => {
    const { view, rerender } = renderTree({
      hosts: [buildBox()],
      hostActions: actions(),
      sessionsListed: false,
      localHostWaiting: true,
      // Floti's own settings came without the host; its folders were never read.
      discovery: null,
      pinnedPaths: ['C:\\work\\my-work\\repos\\notes'],
      sessionsByPath: new Map()
    })
    const local = (): HTMLElement => group(view.container, 'local')
    const header = local().querySelector('[data-host-header]') as HTMLElement
    expect(header.getAttribute('data-host-state')).toBe('waiting')
    expect(header.textContent).toContain('waiting')
    expect(local().querySelector('[data-host-phase="waiting"]')?.textContent).toBe(
      'Waiting for the host on This PC…Its sessions show here once it answers.'
    )
    // The notice says why, once: the tree repeats no refusal and offers no fix of its own.
    expect(view.container.querySelector('[data-empty-state="host-unlisted:local"]')).toBeNull()
    expect(within(local()).queryByRole('button', { name: 'Try again' })).toBeNull()
    // Could not look is not nothing there: no pin is called gone before the folders are read.
    expect(within(local()).queryByText('folder gone')).toBeNull()

    rerender({ sessionsListed: true, localHostWaiting: false, discovery: THIS_PC })
    expect(local().querySelector('[data-host-header]')?.getAttribute('data-host-state')).toBe('live')
    expect(local().querySelector('[data-host-phase]')).toBeNull()
  })

  it('says this PC’s host is connecting, and why it waits, through each start-up step until its sessions are listed', () => {
    const { view, rerender } = renderTree({
      hosts: [buildBox()],
      hostActions: actions(),
      sessionsListed: false,
      localHostStarting: 'preparing',
      sessionsByPath: new Map()
    })
    const local = (): HTMLElement => group(view.container, 'local')
    const state = (): string | null | undefined =>
      local().querySelector('[data-host-header]')?.getAttribute('data-host-state')
    const line = (): string | null | undefined => local().querySelector('[data-host-phase="connecting"]')?.textContent
    expect(state()).toBe('connecting')
    expect(line()).toBe('Connecting to This PC…Preparing floti.')

    rerender({ localHostStarting: 'connecting' })
    expect(state()).toBe('connecting')
    expect(line()).toBe('Connecting to This PC…')

    rerender({ localHostStarting: 'starting' })
    expect(line()).toBe('Connecting to This PC…Starting its host.')

    // A host that cannot be used says it waits, through main's retries, never connecting or reading its sessions.
    rerender({ localHostStarting: 'connecting', localHostWaiting: true })
    expect(state()).toBe('waiting')
    expect(local().querySelector('[data-host-phase="connecting"]')).toBeNull()

    // Answered: its sessions load, then are listed.
    rerender({ localHostStarting: null })
    expect(state()).toBe('loading')
    expect(local().querySelector('[data-host-phase="loading"]')?.textContent).toBe('Loading sessions…')

    rerender({ sessionsListed: true })
    expect(state()).toBe('live')
    expect(local().querySelector('[data-host-phase]')).toBeNull()
  })

  it('shows this PC’s empty state only once its sessions are listed', () => {
    const none: DiscoveryResult = { ...THIS_PC, roots: [], workspaces: [], projects: [] }
    const { view, rerender } = renderTree({ discovery: none, sessionsByPath: new Map(), sessionsListed: false })
    expect(view.container.querySelector('[data-host-phase="loading"]')).not.toBeNull()
    expect(screen.queryByText('No sessions yet')).toBeNull()

    rerender({ sessionsListed: true })
    expect(view.container.querySelector('[data-host-phase]')).toBeNull()
    expect(screen.getByText('No sessions yet')).toBeTruthy()
  })

  it('says on build-box’s header that its floti is older, with both versions, and a press starts its update', async () => {
    const hostActions = { ...actions(), onUpdateFloti: vi.fn() }
    const { view } = renderTree({
      hosts: [buildBox({ floti: { verdict: 'older', host: '2.4.0', client: '2.5.0' } })],
      hostActions
    })
    const header = group(view.container, 'h1').querySelector('[data-host-header]') as HTMLElement
    const older = within(header).getByRole('button', { name: 'older floti: update floti on build-box' })
    expect(older.textContent).toBe('older floti')
    expect(older.getAttribute('title')).toBe('It runs floti 2.4.0; this Floti carries 2.5.0. Press to update floti on build-box.')
    expect(header.getAttribute('data-host-state')).toBe('live')
    await userEvent.click(older)
    expect(hostActions.onUpdateFloti).toHaveBeenCalledWith('h1')
  })

  it('says a floti too old or newer in place of its phase’s word, and offers no update for a newer one', () => {
    const hostActions = { ...actions(), onUpdateFloti: vi.fn() }
    const refused = {
      kind: 'unreachable',
      failure: { reason: 'build-box’s floti is too old for this Floti', retrying: false }
    } as const
    const { view, rerender } = renderTree({
      hosts: [buildBox({ phase: refused, floti: { verdict: 'tooOld', host: '3.1', client: '3.2' } })],
      hostActions
    })
    const header = (): HTMLElement => group(view.container, 'h1').querySelector('[data-host-header]') as HTMLElement
    expect(header().querySelector('[data-host-phase-word]')).toBeNull()
    expect(within(header()).getByRole('button', { name: 'floti too old: update floti on build-box' })).toBeTruthy()

    rerender({ hosts: [buildBox({ phase: refused, floti: { verdict: 'newer', host: '4.0', client: '3.2' } })] })
    const newer = within(header()).getByText('newer floti')
    expect(newer.tagName).toBe('SPAN')
    expect(newer.getAttribute('title')).toBe('It speaks protocol 4.0; this Floti speaks 3.2. Update Floti on this PC.')
    expect(header().querySelector('[data-host-phase-word]')).toBeNull()
  })

  it('offers New session, New workspace and Rescan on each host header, under the pointer and in its right-click menu', async () => {
    const hostActions = actions()
    const { view } = renderTree({ hosts: [buildBox()], hostActions })
    const header = group(view.container, 'h1').querySelector<HTMLElement>('[data-host-header]')!

    await userEvent.click(within(header).getByRole('button', { name: 'New session on build-box' }))
    expect(hostActions.onNewSession).toHaveBeenCalledWith('h1')
    await userEvent.click(within(header).getByRole('button', { name: 'Rescan build-box' }))
    expect(hostActions.onRescan).toHaveBeenCalledWith('h1')

    fireEvent.contextMenu(header, { clientX: 20, clientY: 30 })
    await userEvent.click(screen.getByRole('menuitem', { name: 'New workspace' }))
    expect(hostActions.onNewWorkspace).toHaveBeenCalledWith('h1')

    const localHeader = group(view.container, 'local').querySelector<HTMLElement>('[data-host-header]')!
    await userEvent.click(within(localHeader).getByRole('button', { name: 'New workspace on This PC' }))
    expect(hostActions.onNewWorkspace).toHaveBeenLastCalledWith('local')
  })

  it('shows the empty state under a host whose scan found nothing, offering New workspace there', async () => {
    const hostActions = actions()
    const empty: DiscoveryResult = { ...BUILD_BOX, workspaces: [], projects: [] }
    const { view } = renderTree({
      hosts: [buildBox({ discovery: empty, sessionsByPath: new Map() })],
      hostActions
    })
    const remote = group(view.container, 'h1')
    expect(remote.querySelector('[data-empty-state="host-empty:h1"]')).not.toBeNull()
    expect(within(remote).getByText('No workspaces on build-box yet')).toBeTruthy()
    expect(within(remote).getByText('Its sessions run on build-box.')).toBeTruthy()
    await userEvent.click(within(remote).getByRole('button', { name: 'New workspace' }))
    expect(hostActions.onNewWorkspace).toHaveBeenCalledWith('h1')
  })

  it('with nothing on this PC, says No sessions yet and offers New session, which opens the launcher', async () => {
    const none: DiscoveryResult = { ...THIS_PC, roots: [], workspaces: [], projects: [] }
    const { props, view } = renderTree({ discovery: none, sessionsByPath: new Map() })
    const empty = view.container.querySelector('[data-empty-state="sessions-empty"]') as HTMLElement
    expect(within(empty).getByText('No sessions yet')).toBeTruthy()
    expect(within(empty).getByText('Start an agent in one of your workspaces.')).toBeTruthy()
    const button = within(empty).getByRole('button', { name: 'New session' })
    expect(button.textContent).toMatch(/(Ctrl\+|⌘)N$/)
    await userEvent.click(button)
    expect(props.onNewSession).toHaveBeenCalledOnce()
  })

  it('says Unknown with Try again, never No sessions yet, while this PC’s folders cannot be read', async () => {
    const hostActions = actions()
    const none: DiscoveryResult = { ...THIS_PC, roots: [], workspaces: [], projects: [] }
    const { view } = renderTree({
      discovery: none,
      sessionsByPath: new Map(),
      scanError: 'The scan roots could not be read.',
      hosts: [buildBox()],
      hostActions
    })
    const local = group(view.container, 'local')
    expect(local.querySelector('[data-empty-state="sessions-empty"]')).toBeNull()
    const unknown = local.querySelector('[data-empty-state="sessions-unknown"]') as HTMLElement
    expect(within(unknown).getByText('Unknown')).toBeTruthy()
    expect(within(unknown).getByText('Floti could not read the folders on This PC.')).toBeTruthy()
    await userEvent.click(within(unknown).getByRole('button', { name: 'Try again' }))
    expect(hostActions.onRescan).toHaveBeenCalledWith('local')
  })

  it('says it is scanning, never No sessions yet, before this PC’s first scan has answered', () => {
    renderTree({ discovery: null, sessionsByPath: new Map() })
    expect(screen.queryByText('No sessions yet')).toBeNull()
    expect(screen.getByText('Scanning…')).toBeTruthy()
  })

  it('says Unknown and why, never the empty state, while the host’s folders cannot be read', async () => {
    const hostActions = actions()
    const { view } = renderTree({
      hosts: [buildBox({ discovery: null, sessionsByPath: new Map(), scanError: 'Its scan roots could not be read.' })],
      hostActions
    })
    const remote = group(view.container, 'h1')
    expect(remote.querySelector('[data-empty-state^="host-empty"]')).toBeNull()
    expect(within(remote).getByText('Unknown')).toBeTruthy()
    expect(within(remote).getByText(/Its scan roots could not be read\./)).toBeTruthy()
    await userEvent.click(within(remote).getByRole('button', { name: 'Try again' }))
    expect(hostActions.onRescan).toHaveBeenCalledWith('h1')
  })

  it('S20: the Add a host line at the end of the list opens the add-a-host dialog', async () => {
    const hostActions = actions()
    renderTree({ hosts: [buildBox()], hostActions })
    await userEvent.click(screen.getByRole('button', { name: 'Add a host' }))
    expect(hostActions.onAddHost).toHaveBeenCalledOnce()
  })

  it('S21: with this PC the only host, the list’s right-click menu has Add a host…, which opens the dialog', async () => {
    const onAddHostMenu = vi.fn()
    renderTree({ hosts: [], onAddHostMenu })
    fireEvent.contextMenu(screen.getByRole('navigation', { name: 'Projects and sessions' }), { clientX: 40, clientY: 80 })
    const item = await screen.findByRole('menuitem', { name: 'Add a host…' })
    await userEvent.click(item)
    expect(onAddHostMenu).toHaveBeenCalledOnce()
    expect(screen.queryByRole('menuitem', { name: 'Add a host…' })).toBeNull()
  })

  it('S21: with no menu handler given, the list’s right-click menu has no Add a host… item', () => {
    renderTree({ hosts: [] })
    fireEvent.contextMenu(screen.getByRole('navigation', { name: 'Projects and sessions' }), { clientX: 40, clientY: 80 })
    expect(screen.queryByRole('menuitem', { name: 'Add a host…' })).toBeNull()
  })
})

describe('S1: what the window gives the tree about hosts', () => {
  const hostProps = (hosts: readonly TreeHost[], onAddHost = vi.fn()) => ({
    onAddHost,
    // The window always gives the tree its host actions and this PC's name.
    props: {
      hostActions: actions(),
      localName: 'This PC',
      localKey: 'local',
      ...sessionTreeHostProps({ hosts, onAddHost })
    }
  })
  const rightClickList = () =>
    fireEvent.contextMenu(screen.getByRole('navigation', { name: 'Projects and sessions' }), { clientX: 40, clientY: 80 })

  it('S1: with this PC the only host, right-clicking the list offers Add a host…, which opens the dialog', async () => {
    const { onAddHost, props } = hostProps([])
    renderTree(props)
    rightClickList()
    await userEvent.click(await screen.findByRole('menuitem', { name: 'Add a host…' }))
    expect(onAddHost).toHaveBeenCalledOnce()
  })

  it('S1: with another host added, the list shows it, ends with Add a host and its right-click menu has none', () => {
    const { props } = hostProps([buildBox()])
    renderTree(props)
    rightClickList()
    expect(screen.queryByRole('menuitem', { name: 'Add a host…' })).toBeNull()
    expect(screen.getByRole('button', { name: 'Add a host' })).toBeTruthy()
    expect(screen.getAllByText('build-box').length).toBeGreaterThan(0)
  })
})
