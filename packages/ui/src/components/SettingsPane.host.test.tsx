import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useState, useSyncExternalStore, type JSX } from 'react'
import { describe, expect, it, vi } from 'vitest'
import { DEFAULT_SETTINGS, type AppSettings } from '@floti/core/types'
import type { HostChoiceHost } from './HostSwitch'
import { SettingsPane, type SettingsPaneProps } from './SettingsPane'
import { SettingsSections, type SettingsSectionId } from './SettingsSections'

/**
 * Settings' Floti host group across hosts (the design's S1-S8): the sidebar's
 * host switch and the pane together, over a store per host as each host keeps
 * its settings and tells every window attached to it. The window's own wiring
 * of a host's bridge is tested with `useHostWorkspaces`.
 */

type HostValues = Pick<
  AppSettings,
  'sessionAgents' | 'sessionMcp' | 'desktopMcp' | 'browserMcp' | 'agentDepth' | 'transcriptArchiveMaxBytes'
>

/** One host's settings as it keeps them: every window attached to it hears each change. Null while out of reach. */
function hostStore(initial: HostValues | null) {
  let values = initial
  const listeners = new Set<() => void>()
  return {
    get: (): HostValues | null => values,
    subscribe: (listener: () => void): (() => void) => {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    write: vi.fn((patch: Partial<HostValues>) => {
      if (values === null) throw new Error('out of reach')
      values = { ...values, ...patch }
      for (const listener of listeners) listener()
    })
  }
}
type HostStore = ReturnType<typeof hostStore>

const values = (patch: Partial<HostValues> = {}): HostValues => ({
  sessionAgents: DEFAULT_SETTINGS.sessionAgents,
  sessionMcp: DEFAULT_SETTINGS.sessionMcp,
  desktopMcp: DEFAULT_SETTINGS.desktopMcp,
  browserMcp: DEFAULT_SETTINGS.browserMcp,
  agentDepth: DEFAULT_SETTINGS.agentDepth,
  transcriptArchiveMaxBytes: DEFAULT_SETTINGS.transcriptArchiveMaxBytes,
  ...patch
})

const NAMES: Record<string, string> = { local: 'This PC', h1: 'buildbox', h2: 'mac-air', h3: 'lab-pi' }

function base(): SettingsPaneProps {
  const s = DEFAULT_SETTINGS
  return {
    section: 'tools',
    status: null,
    checking: false,
    onRecheck: vi.fn(),
    onLocateClaude: vi.fn(),
    onClearClaudeOverride: vi.fn(),
    onShowWalkthrough: vi.fn(),
    roots: [],
    projectCount: 0,
    scanning: false,
    onAddRoot: vi.fn(),
    onRemoveRoot: vi.fn(),
    pinnedProjects: [],
    onUnpinProject: vi.fn(),
    appearance: {
      theme: s.theme,
      themeDark: s.themeDark,
      themeLight: s.themeLight,
      paneGap: s.paneGap,
      cornerRadius: s.cornerRadius,
      density: s.density,
      accentColor: s.accentColor
    },
    onAppearanceChange: vi.fn(),
    themes: null,
    themeState: null,
    onOpenThemesFolder: vi.fn(),
    onDuplicateTheme: vi.fn(),
    updateCheck: true,
    onUpdateCheckChange: vi.fn(),
    appVersion: null,
    releasesUrl: null,
    update: null,
    updateError: null,
    updateChecking: false,
    onInstallUpdate: vi.fn(),
    onCheckForUpdate: vi.fn(),
    onOpenReleases: vi.fn(),
    terminal: {
      terminalFontFamily: s.terminalFontFamily,
      terminalFontSize: s.terminalFontSize,
      terminalCursorStyle: s.terminalCursorStyle,
      terminalCursorBlink: s.terminalCursorBlink,
      terminalScrollback: s.terminalScrollback,
      projectShellHeightPct: s.projectShellHeightPct
    },
    onTerminalChange: vi.fn(),
    terminalFontStack: 'monospace',
    terminalShell: null,
    onTerminalShellChange: vi.fn(),
    shells: [],
    onLocateShell: vi.fn(),
    onShellPath: vi.fn(async () => null),
    archiveStats: null,
    transcriptArchiveMaxBytes: s.transcriptArchiveMaxBytes,
    onTranscriptArchiveMaxBytesChange: vi.fn(),
    templateCount: 0,
    templatesDir: 'C:\\templates',
    onManageTemplates: vi.fn(),
    onRevealTemplates: vi.fn(),
    filesWrap: s.filesWrap,
    onFilesWrapChange: vi.fn(),
    browserReach: s.browserReach,
    browserSearch: s.browserSearch,
    onBrowserSearchChange: vi.fn(),
    onBrowserReachChange: vi.fn(),
    browserMcp: s.browserMcp,
    onBrowserMcpChange: vi.fn(),
    browserMcpLocalOnly: s.browserMcpLocalOnly,
    onBrowserMcpLocalOnlyChange: vi.fn(),
    sessionMcp: s.sessionMcp,
    onSessionMcpChange: vi.fn(),
    sessionAgents: s.sessionAgents,
    onSessionAgentsChange: vi.fn(),
    desktopMcp: s.desktopMcp,
    onDesktopMcpChange: vi.fn(),
    agentLimits: {
      agentChildrenPerParent: s.agentChildrenPerParent,
      agentCommandsPerParent: s.agentCommandsPerParent,
      agentSessionsPerHost: s.agentSessionsPerHost,
      agentDepth: s.agentDepth,
      agentStartsPerParent: s.agentStartsPerParent
    },
    onAgentLimitsChange: vi.fn(),
    showChildrenInParentTab: s.showChildrenInParentTab,
    onShowChildrenInParentTabChange: vi.fn(),
    restoreWithoutAsking: false,
    onRestoreWithoutAskingChange: vi.fn(),
    quitSessions: 'end',
    onQuitSessionsChange: vi.fn(),
    onStopHost: vi.fn(),
    hostLogFile: null,
    hosts: [],
    onAddHost: vi.fn(),
    onRemoveHost: vi.fn(),
    onHostQuitChange: vi.fn(),
    hostInstall: { check: vi.fn(), install: vi.fn(), cancel: vi.fn() },
    onHostInstalled: vi.fn(),
    devices: { hosts: [], onDisconnect: vi.fn() }
  }
}

/**
 * One window: its sidebar and its pane, picking a host as App does, reading
 * that host's store and writing to it alone. This desktop's own settings are
 * `own`, which no host pick changes.
 */
function Window({
  stores,
  section = 'tools',
  own = { browserMcpLocalOnly: false }
}: {
  stores: Readonly<Record<string, HostStore>>
  section?: SettingsSectionId
  own?: { browserMcpLocalOnly: boolean }
}): JSX.Element {
  const [host, setHost] = useState('local')
  const [current, setCurrent] = useState<SettingsSectionId>(section)
  const store = stores[host]!
  const kept = useSyncExternalStore(store.subscribe, store.get)
  const ids = Object.keys(stores)
  const hosts: HostChoiceHost[] = ids.map((id) => ({
    id,
    name: NAMES[id]!,
    remote: id !== 'local',
    away: stores[id]!.get() === null
  }))
  const connected = hosts.filter((each) => each.away !== true).length
  const shown = kept ?? values()
  return (
    <>
      <SettingsSections
        current={current}
        onSelect={setCurrent}
        host={{
          name: NAMES[host]!,
          choice: connected < 2 && host === 'local' ? undefined : { hosts, host, onHostChange: setHost }
        }}
      />
      <SettingsPane
        {...base()}
        section={current}
        host={{
          name: NAMES[host]!,
          remote: host !== 'local',
          unknown: kept === null ? { kind: 'away', failedAt: '14:02' } : null
        }}
        onShowSection={setCurrent}
        sessionAgents={shown.sessionAgents}
        onSessionAgentsChange={(sessionAgents) => store.write({ sessionAgents })}
        sessionMcp={shown.sessionMcp}
        onSessionMcpChange={(sessionMcp) => store.write({ sessionMcp })}
        desktopMcp={shown.desktopMcp}
        onDesktopMcpChange={(desktopMcp) => store.write({ desktopMcp })}
        browserMcp={shown.browserMcp}
        onBrowserMcpChange={(browserMcp) => store.write({ browserMcp })}
        agentLimits={{ ...base().agentLimits, agentDepth: shown.agentDepth }}
        onAgentLimitsChange={(patch) => store.write(patch as Partial<HostValues>)}
        transcriptArchiveMaxBytes={shown.transcriptArchiveMaxBytes}
        browserMcpLocalOnly={own.browserMcpLocalOnly}
      />
    </>
  )
}

const agentsTick = (host: string): HTMLInputElement =>
  within(screen.getByRole('region', { name: `What sessions on ${host} get` })).getByRole('checkbox', {
    name: 'Sessions can start agents'
  }) as HTMLInputElement

describe('Settings across hosts', () => {
  it('M1: picks a host with the switch on the Floti host label, and every host section shows that host’s values', async () => {
    const stores = { local: hostStore(values({ sessionAgents: true })), h1: hostStore(values({ sessionAgents: false })) }
    render(<Window stores={stores} />)

    expect(agentsTick('This PC').checked).toBe(true)
    expect(document.querySelector('[data-settings-host-line="This PC"]')).toBeTruthy()

    await userEvent.click(screen.getByRole('radio', { name: 'buildbox' }))
    expect(agentsTick('buildbox').checked).toBe(false)
    expect(document.querySelector('[data-settings-host-line="buildbox"]')?.textContent).toBe(
      'On buildbox. Every window attached to it shows the same values.'
    )
    expect(screen.getByRole('region', { name: 'What sessions on buildbox get' }).textContent).toContain('Off on buildbox.')

    // The pick holds for every section in the group.
    await userEvent.click(screen.getByRole('button', { name: 'Agents' }))
    expect(screen.getByRole('region', { name: 'Limits' }).textContent).toContain('buildbox enforces each of these')
  })

  it('a change lands on the picked host only, and every window attached to it shows the new value', async () => {
    const stores = { local: hostStore(values({ sessionAgents: true })), h1: hostStore(values({ sessionAgents: true })) }
    const first = render(<Window stores={stores} />)
    const second = render(<Window stores={stores} />)
    const inFirst = within(first.container)
    const inSecond = within(second.container)

    await userEvent.click(inFirst.getByRole('radio', { name: 'buildbox' }))
    await userEvent.click(inSecond.getByRole('radio', { name: 'buildbox' }))
    await userEvent.click(inFirst.getByRole('checkbox', { name: 'Sessions can start agents' }))

    expect(stores.h1.write).toHaveBeenCalledWith({ sessionAgents: false })
    expect(stores.local.write).not.toHaveBeenCalled()
    expect(stores.h1.get()?.sessionAgents).toBe(false)
    expect(stores.local.get()?.sessionAgents).toBe(true)
    expect((inSecond.getByRole('checkbox', { name: 'Sessions can start agents' }) as HTMLInputElement).checked).toBe(false)
  })

  it('S4: with one host there is no switch, and its name sits beside the label', () => {
    render(<Window stores={{ local: hostStore(values()) }} />)
    expect(screen.queryByRole('radiogroup', { name: 'Settings host' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Settings host' })).toBeNull()
    expect(document.querySelector('[data-settings-host-name]')?.textContent).toBe('This PC')
  })

  it('S5: a picked host out of reach shows its settings as Unknown under one sentence, never defaults, rows in their places', async () => {
    const stores = { local: hostStore(values()), h1: hostStore(values()), h2: hostStore(null) }
    render(<Window stores={stores} />)

    await userEvent.click(screen.getByRole('radio', { name: 'mac-air' }))
    const notice = screen.getByRole('status')
    expect(notice.textContent).toBe(
      'Can’t reach mac-airIts link failed at 14:02. Floti keeps trying, and mac-air’s settings show here when it is back. Settings › Hosts says more.'
    )
    const group = screen.getByRole('region', { name: 'What sessions on mac-air get' })
    expect(within(group).queryAllByRole('checkbox')).toHaveLength(0)
    expect(within(group).getAllByText('Unknown')).toHaveLength(4)
    expect(
      [...group.querySelectorAll('[data-settings-unknown-row]')].map((row) => row.getAttribute('data-settings-unknown-row'))
    ).toEqual([
      'Let Claude see the other sessions',
      "Sessions can use Floti's window",
      'Let Claude drive the browser',
      'Sessions can start agents'
    ])

    await userEvent.click(screen.getByRole('button', { name: 'Agents' }))
    expect(within(screen.getByRole('region', { name: 'Limits' })).queryAllByRole('textbox')).toHaveLength(0)
    expect(within(screen.getByRole('region', { name: 'Limits' })).getAllByText('Unknown')).toHaveLength(5)

    // Settings › Hosts says more, one press away.
    await userEvent.click(within(screen.getByRole('status')).getByRole('button', { name: 'Settings › Hosts' }))
    expect(screen.getByRole('heading', { level: 1 }).textContent).toBe('Hosts')
  })

  it('S7: a Floti desktop section never follows the pick: Browser keeps this desktop’s localhost switch and points at Tools', async () => {
    const stores = { local: hostStore(values()), h1: hostStore(values({ browserMcp: false })) }
    render(<Window stores={stores} section="browser" own={{ browserMcpLocalOnly: true }} />)
    await userEvent.click(screen.getByRole('radio', { name: 'buildbox' }))

    expect(document.querySelector('[data-settings-host-line]')).toBeNull()
    const claude = screen.getByRole('region', { name: 'Claude in this browser' })
    expect((within(claude).getByRole('checkbox', { name: 'Hold Claude to localhost' }) as HTMLInputElement).checked).toBe(true)
    expect(screen.queryByRole('checkbox', { name: 'Let Claude drive the browser' })).toBeNull()

    await userEvent.click(within(claude).getByRole('button', { name: 'Floti host › Tools' }))
    expect(screen.getByRole('region', { name: 'What sessions on buildbox get' })).toBeTruthy()
  })
})

describe('host sections on a host on another computer', () => {
  const remote = { name: 'buildbox', remote: true, unknown: null } as const

  it('Programs: no file picker for its claude or its shell, and a typed shell path is checked there', async () => {
    const onShellPath = vi.fn(async () => null)
    render(<SettingsPane {...base()} section="programs" host={remote} onLocateClaude={undefined} onLocateShell={undefined} onShellPath={onShellPath} />)
    const locate = screen.getByRole('button', { name: 'Locate manually…' })
    expect(locate).toHaveProperty('disabled', true)
    expect(locate.getAttribute('title')).toBe('The file picker browses this PC, so it cannot choose a claude on buildbox')
    expect(screen.queryByRole('button', { name: 'Choose…' })).toBeNull()

    await userEvent.type(screen.getByRole('textbox', { name: 'Shell path' }), '/usr/bin/fish{Enter}')
    expect(onShellPath).toHaveBeenCalledWith('/usr/bin/fish')
  })

  it('Workspace: a folder is typed for it, and this PC’s templates are not offered', async () => {
    const onAddRootPath = vi.fn(async (path: string) => (path === '/nope' ? 'Nothing is at /nope.' : null))
    render(<SettingsPane {...base()} section="workspace" host={remote} roots={['/srv/work']} onAddRootPath={onAddRootPath} />)
    expect(screen.queryByRole('button', { name: 'Add a folder' })).toBeNull()
    expect(screen.queryByRole('region', { name: 'Workspace templates' })).toBeNull()

    const field = screen.getByRole('textbox', { name: 'Folder on buildbox' })
    await userEvent.type(field, '/nope{Enter}')
    expect(await screen.findByRole('alert')).toBeTruthy()
    await userEvent.clear(field)
    await userEvent.type(field, '/srv/more{Enter}')
    expect(onAddRootPath).toHaveBeenLastCalledWith('/srv/more')
  })

  it('Updates: shows that host’s own tick and sends the rest to Settings › Hosts', async () => {
    const onUpdateCheckChange = vi.fn()
    const onShowSection = vi.fn()
    render(
      <SettingsPane
        {...base()}
        section="updates"
        host={remote}
        updateCheck={false}
        onUpdateCheckChange={onUpdateCheckChange}
        onShowSection={onShowSection}
      />
    )
    expect(screen.queryByRole('button', { name: 'Check now' })).toBeNull()
    await userEvent.click(screen.getByRole('checkbox', { name: 'Look for a newer floti once a day and install it' }))
    expect(onUpdateCheckChange).toHaveBeenCalledWith(true)
    await userEvent.click(screen.getByRole('button', { name: 'Settings › Hosts' }))
    expect(onShowSection).toHaveBeenCalledWith('hosts')
  })
})
