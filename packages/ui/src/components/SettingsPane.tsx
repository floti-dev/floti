import { useState, type JSX } from 'react'
import {
  ACCENT_SWATCHES,
  AGENT_CHILDREN_PER_PARENT,
  AGENT_COMMANDS_PER_PARENT,
  AGENT_DEPTH,
  AGENT_SESSIONS_PER_HOST,
  AGENT_STARTS_PER_PARENT,
  CORNER_RADIUS,
  DEFAULT_THEME_ID,
  DENSITY_MODES,
  deriveAccent,
  PANE_GAP,
  PROJECT_SHELL_HEIGHT_PCT,
  settingEntry,
  settingHint,
  TERMINAL_CURSOR_STYLES,
  TERMINAL_FONT_SIZE,
  TERMINAL_SCROLLBACK,
  type AppSettings,
  type ArchiveStats,
  browserSearchName,
  BROWSER_SEARCH_ENGINES,
  type BrowserReach,
  type BrowserSearch,
  type DetectedShell,
  type QuitSessions,
  type HostSystem,
  type SettingKey,
  type TerminalCursorStyle,
  type Density,
  type ThemeDefinition,
  type ThemeKind,
  type ThemeListing,
  type ThemeState,
  type ThemeTokens
} from '@floti/core/types'
import { cn } from '../lib/cn'
import { SETTINGS_SECTIONS, type SettingsSectionId } from './SettingsSections'
import { SEGMENT_ON } from '../lib/segmented'
import { formatAge, formatBytes } from '../lib/time'
import { Checkbox } from './Checkbox'
import {
  Action,
  Actions,
  CheckRow,
  Divider,
  Fact,
  Group,
  HostLine,
  NumberField,
  ReachNotice,
  Row,
  Select,
  SettingsPage,
  Stepper,
  UnknownCheckRow,
  UnknownValue,
  Verdict,
  useDraft
} from './SettingsKit'
import { CheckIcon, CloseIcon, PlusIcon, RefreshIcon } from './icons'
import type { SetupClaudeStatus } from '../lib/claudeStatus'
import { HostsSettings, type HostInstallActions, type HostsSettingsProps, type SettingsHost } from './HostsSettings'
import { DevicesSettings, type DevicesSettingsProps } from './DevicesSettings'

/**
 * Floti's own settings.
 *
 * Not the `.claude` trees Claude Code reads, which belong to Claude, are shared
 * with every other client on the machine, and are edited by Claude Code's own
 * commands. This is the app's own configuration, and it is the
 * permanent home for it - every setting a later feature adds lands here as
 * another row in an existing group or another group at the end.
 *
 * One scrolling page of grouped cards rather than sub-views. Six groups and
 * two dozen controls; a segmented navigator over that many rows is furniture
 * standing in for content. When a group outgrows the page, it earns its own
 * view then.
 *
 * The Terminal group is the appearance of the panes, not the terminal's own
 * ground: the xterm palette is fixed in both themes and pixel-asserted by the
 * fidelity checks (DESIGN.md par. 6), so colour is deliberately not a row here.
 *
 * The title bar's theme toggle writes the same setting this pane does. A
 * quick accessor beside the thing it changes is worth having, and this pane is
 * where to find the setting when you are not already looking at it. Both write
 * through `settings:write` and this pane renders whatever `settings:changed`
 * carries back, so the two cannot disagree.
 *
 * Internal state is not shown. `windowBounds` and `firstRunCompletedAt` live
 * in the same table but they are things Floti remembers, not things anyone
 * chose.
 */

/** What Settings › Agents says of the host's agents, and where Open Agents goes. */
export interface AgentsOverview {
  /** How many agents the host lists; null until it answers. */
  count: number | null
  hostName: string
  folder: string | null
  onOpen: () => void
}

/**
 * The host the Floti host group's sections show: its name, and whether its
 * settings are known. Every value a host section draws is that host's.
 */
export interface SettingsHostView {
  name: string
  /** A host on another computer: the file pickers browse this PC, so they are not offered for it. */
  remote: boolean
  /**
   * Why its settings are not known, and the host sections show Unknown in
   * their place, never defaults: it cannot be reached (`failedAt` is when its
   * link failed, as the window shows a time, or null when not seen), or it has
   * not answered yet. Null once they are known.
   */
  unknown: { kind: 'away'; failedAt: string | null } | { kind: 'reading' } | null
}

const THIS_PC: SettingsHostView = { name: 'This PC', remote: false, unknown: null }

export interface SettingsPaneProps {
  /** The one section on screen; the sidebar lists the rest (`SettingsSections`). */
  section: SettingsSectionId
  /** The host the Floti host group's sections show; This PC, known, when not given. */
  host?: SettingsHostView | undefined
  /** Opens another section: Settings › Hosts from a host out of reach, Floti host › Tools from Browser. */
  onShowSection?: ((id: SettingsSectionId) => void) | undefined
  /** What Floti found out about the CLI. Null until the first read lands. */
  status: SetupClaudeStatus | null
  /** Why the status could not be read, when it could not: shown as Unknown, never as not found. */
  statusError?: string | null
  /** A status read the user asked for, so it gets a spinner. */
  checking: boolean
  onRecheck: () => void
  /** This PC's file picker, for a `claude` Floti did not find. Absent for a host on another computer. */
  onLocateClaude?: (() => void) | undefined
  /** Writes `claudePath: null` - back to whatever discovery finds. */
  onClearClaudeOverride: () => void
  /** Starts the guided first run again at step 1, in its own window. */
  onShowWalkthrough: () => void

  roots: string[]
  /** What the current roots turned up, so "it worked" is visible here. */
  projectCount: number
  scanning: boolean
  onAddRoot: () => void
  /**
   * A folder on a host on another computer, typed, where this PC's picker
   * cannot browse: resolves with why it was refused, or null once it is scanned.
   */
  onAddRootPath?: ((path: string) => Promise<string | null>) | undefined
  onRemoveRoot: (path: string) => void

  /**
   * `pinnedProjects`, as stored: absolute paths, in the order the setting holds
   * them rather than the order the sidebar shows them.
   *
   * Pins are *made* on the sidebar's own rows, where the project is, and this
   * pane is where the whole set is legible at once - including a path whose
   * folder has gone, which is the one a person comes here to clear. Paths and
   * not names, deliberately: the setting is a list of paths and it is a list of
   * paths that a re-clone invalidates, so the pane shows the value rather than
   * a friendlier rendering of it.
   */
  pinnedProjects: string[]
  onUnpinProject: (path: string) => void

  /** Every setting the Appearance group writes. */
  appearance: AppearanceSettings
  onAppearanceChange: (patch: Partial<AppearanceSettings>) => void
  /** Built-in and user themes. Null until the first read lands. */
  themes: ThemeListing | null
  /** The theme on screen, as the main process resolved it. Null until it lands. */
  themeState: ThemeState | null
  /** Opens the user themes folder, creating it if it was deleted. */
  onOpenThemesFolder: () => void
  /** Writes a complete copy of a theme into that folder and shows it. */
  onDuplicateTheme: (id: string) => void

  updateCheck: boolean
  onUpdateCheckChange: (next: boolean) => void

  /** This build's version, from `app:info`. Null until that read lands. */
  appVersion: string | null
  /**
   * The releases page, from `app:info` rather than from a check's result.
   *
   * The link has to be reachable in exactly the states no check produces - up
   * to date, offline, the setting off - so it cannot come from `update`.
   */
  releasesUrl: string | null
  /** What this PC's host knows about updates. Null until it has been heard. */
  update: UpdateView | null
  /** Why the host could not be heard, or why the last press was refused. */
  updateError: string | null
  updateChecking: boolean
  onCheckForUpdate: () => void
  /** Asks the host to update floti and the desktop. */
  onInstallUpdate: () => void
  onOpenReleases: () => void

  terminal: TerminalSettings
  onTerminalChange: (patch: Partial<TerminalSettings>) => void
  /**
   * The font stack the terminals are actually running, so the preview is the
   * real thing rather than this pane's re-derivation of the prepend rule.
   */
  terminalFontStack: string
  /** The shell a project pane opens on the picked host; null to have that host detect one. */
  terminalShell: string | null
  onTerminalShellChange: (path: string | null) => void
  /** Shells found on the picked host. Empty until the probe lands. */
  shells: DetectedShell[]
  /** This PC's file picker, for a shell installed somewhere detection misses. Absent for a host on another computer. */
  onLocateShell?: (() => void) | undefined
  /**
   * A shell's full path, typed or pasted, for one the file picker refuses (an
   * app execution alias). Resolves with a sentence saying why the path was
   * refused, or null once it is the shell.
   */
  onShellPath: (path: string) => Promise<string | null>

  /**
   * What the transcript archive holds. Null until the first read lands.
   *
   * Passed in rather than derived from the settings, because the interesting
   * half of this group is not the ceiling - it is how much is actually stored
   * against it, which only the main process knows.
   */
  archiveStats: ArchiveStats | null
  transcriptArchiveMaxBytes: number
  onTranscriptArchiveMaxBytesChange: (bytes: number) => void

  /**
   * Workspace templates: how many there are, where they live, and the way in to
   * managing them.
   *
   * A group here as well as a link in the New Workspace dialog, because
   * templates are **app-level**: they belong to this desktop rather than to any
   * one workspace, and having to start creating a workspace in order to rename or
   * delete one would be the same mistake as putting "stop scanning this
   * folder" only in Settings - which is a bug this pane has already been on the
   * wrong end of once.
   */
  templateCount: number
  templatesDir: string
  onManageTemplates: () => void
  /** Opens the templates folder itself, for the editing this app does not do. */
  onRevealTemplates: () => void

  /** Whether long lines wrap in the Files view. */
  filesWrap: boolean
  onFilesWrapChange: (wrap: boolean) => void

  /**
   * How far the browser pane may reach.
   *
   * The one browser key that is a preference. The two beside it in the
   * database - the recent addresses and the per-project ones - are state, so
   * they sit with `workspaceTabs` and are deliberately not on this pane.
   */
  browserReach: BrowserReach
  onBrowserReachChange: (reach: BrowserReach) => void
  /** What the address bar does with a phrase: an engine, or `off`. A preference, so it is here. */
  browserSearch: BrowserSearch
  onBrowserSearchChange: (search: BrowserSearch) => void
  /**
   * The two browser-tool keys: whether the picked host's sessions get the
   * browser tools (that host's, in Tools), and whether those tools are held to
   * localhost when the pane is not (this desktop's, in Browser).
   */
  browserMcp: boolean
  onBrowserMcpChange: (next: boolean) => void
  browserMcpLocalOnly: boolean
  onBrowserMcpLocalOnlyChange: (next: boolean) => void

  /** Whether the picked host's sessions may ask Floti what the other sessions there are doing. That host's, in Tools. */
  sessionMcp: boolean
  onSessionMcpChange: (next: boolean) => void
  /**
   * Whether the sessions a host starts can start sessions of their own, and
   * the limits that bound them. A profile can turn it off for its own sessions;
   * this is what every other session follows. The host enforces all of it;
   * Tools and Agents show the picked host's.
   */
  sessionAgents: boolean
  onSessionAgentsChange: (next: boolean) => void
  /**
   * Whether the sessions a host starts can use this window: open Floti's
   * views and read and change its settings (the floti-desktop tools). Each
   * host keeps its own; Tools shows the picked host's.
   */
  desktopMcp: boolean
  onDesktopMcpChange: (next: boolean) => void
  /** The host's agents in a sentence, with Open Agents: the Agents group's first row. */
  agentsOverview?: AgentsOverview | undefined
  agentLimits: AgentLimitSettings
  onAgentLimitsChange: (patch: Partial<AgentLimitSettings>) => void
  /** Whether a parent's tab shows its running children beside its terminal. Kept on this device. */
  showChildrenInParentTab: boolean
  onShowChildrenInParentTabChange: (next: boolean) => void
  /** Reopen what a crash took without asking. Also ticked from the offer itself. */
  restoreWithoutAsking: boolean
  onRestoreWithoutAskingChange: (next: boolean) => void
  /** What quitting the desktop does to the sessions it started on this PC's host, kept on this device. */
  quitSessions: QuitSessions
  onQuitSessionsChange: (next: QuitSessions) => void
  /** Asks how many sessions stopping ends, and stops this PC's host on a yes. */
  onStopHost: () => void
  /** The log file this PC's host writes its standard error and panics to, as the host names it; null when there is none to name. */
  hostLogFile: string | null
  /** Settings > Hosts: each host on another computer, its quit choice and where its link stands. */
  hosts: readonly SettingsHost[]
  /** Opens the add-a-host dialog. */
  onAddHost: () => void
  /** Forgets a host on this device. Ends nothing on it. */
  onRemoveHost: (id: string) => void
  /** One host's own quit choice; every other host's is left as it is. */
  onHostQuitChange: (id: string, choice: QuitSessions) => void
  /** Checks a host's computer, and installs or updates floti there after a yes. */
  hostInstall: HostInstallActions
  /** Floti was installed or updated on host `id`, at `floti` there. */
  onHostInstalled: (id: string, floti: string, system: HostSystem) => void
  /** Starts updating a host's floti from outside Settings, such as its sidebar row. */
  hostUpdateRequest?: { id: string; nonce: number } | null | undefined
  /** A host's Reconnect now: its link tries at once rather than after the wait. */
  onReconnectHost?: ((id: string) => void) | undefined
  /** This PC's host, while it and this desktop are on different versions (`HostsSettings`' notice). */
  localHost?: HostsSettingsProps['localHost']
  /** Settings > Devices: the devices attached to each connected host, and Disconnect. */
  devices: DevicesSettingsProps
}

/**
 * What this PC's host knows about updates, as Settings > Updates draws it.
 *
 * Structural rather than the desktop package's protocol types, for the reason
 * `StatusBarProps.update` gives. The host makes every request; this is what
 * it last told the window.
 */
export interface UpdateView {
  /** The channel's newest release, as the host's last check found it. */
  newest: string | null
  /** Whether `newest` is a strictly newer release than this build. */
  newer: boolean
  /** When the host's last check read the release list, as RFC 3339 text. */
  checkedAt: string | null
  /** Why the host's last check failed. */
  failed: string | null
  /** Whether a check runs on the host now. */
  checking: boolean
  /** Why the host neither checks nor updates: a dev or check run, or a floti not installed on its own. */
  unavailable: 'readOnly' | 'notStandalone' | null
  /** The update the host runs, while it runs: its last line, and whether it waits for the desktop to quit. */
  installing: { line: string; waitingForApp: boolean } | null
  /** Why the last update the host ran failed. */
  installFailed: string | null
}

/**
 * The six the Terminal group owns, named once so nothing has to list them
 * twice. The shell for project panes is a host's, in Programs.
 *
 * `projectShellHeightPct` is the odd one: it is not a terminal preference and
 * never reaches `applyPrefs`, it is how tall a project page's shell is. It is
 * shown here because this is the group somebody looks in for the shell under a
 * project - the shell picker is already here - and a settings pane organised by
 * where a person would look for a thing beats one organised by which module
 * consumes it.
 */
export type TerminalSettings = Pick<
  AppSettings,
  | 'terminalFontFamily'
  | 'terminalFontSize'
  | 'terminalCursorStyle'
  | 'terminalCursorBlink'
  | 'terminalScrollback'
  | 'projectShellHeightPct'
>

/** The limits on a family of agents, named once for the group that edits them. */
export type AgentLimitSettings = Pick<
  AppSettings,
  | 'agentChildrenPerParent'
  | 'agentCommandsPerParent'
  | 'agentSessionsPerHost'
  | 'agentDepth'
  | 'agentStartsPerParent'
>

/** What a fact reads when there is nothing to put in it. */
const NOTHING = '-'

/**
 * What a row calls its setting, and what it writes under it, from the
 * settings table a session reads them from too (`settingsTable` in core), so
 * the page and the floti-desktop tools cannot say different things.
 */
const labelOf = (key: SettingKey): string => settingEntry(key).label
const hintOf = settingHint

/** A `data-` attribute a driver reaches a control by, named at run time. */
const dataHook = (name: string, value = ''): Record<`data-${string}`, string> => ({ [name]: value })

export function SettingsPane({
  section,
  host = THIS_PC,
  onShowSection,
  status,
  statusError = null,
  checking,
  onRecheck,
  onLocateClaude,
  onClearClaudeOverride,
  onShowWalkthrough,
  roots,
  projectCount,
  scanning,
  onAddRoot,
  onAddRootPath,
  onRemoveRoot,
  pinnedProjects,
  onUnpinProject,
  appearance,
  onAppearanceChange,
  themes,
  themeState,
  onOpenThemesFolder,
  onDuplicateTheme,
  updateCheck,
  onUpdateCheckChange,
  appVersion,
  releasesUrl,
  update,
  updateError,
  updateChecking,
  onCheckForUpdate,
  onInstallUpdate,
  onOpenReleases,
  terminal,
  onTerminalChange,
  terminalFontStack,
  terminalShell,
  onTerminalShellChange,
  shells,
  onLocateShell,
  onShellPath,
  archiveStats,
  transcriptArchiveMaxBytes,
  onTranscriptArchiveMaxBytesChange,
  templateCount,
  templatesDir,
  onManageTemplates,
  onRevealTemplates,
  filesWrap,
  onFilesWrapChange,
  browserReach,
  onBrowserReachChange,
  browserSearch,
  onBrowserSearchChange,
  browserMcp,
  onBrowserMcpChange,
  browserMcpLocalOnly,
  onBrowserMcpLocalOnlyChange,
  sessionMcp,
  onSessionMcpChange,
  sessionAgents,
  onSessionAgentsChange,
  desktopMcp,
  onDesktopMcpChange,
  agentsOverview,
  agentLimits,
  onAgentLimitsChange,
  showChildrenInParentTab,
  onShowChildrenInParentTabChange,
  restoreWithoutAsking,
  onRestoreWithoutAskingChange,
  quitSessions,
  onQuitSessionsChange,
  onStopHost,
  hostLogFile,
  hosts,
  onAddHost,
  onRemoveHost,
  onHostQuitChange,
  hostInstall,
  onHostInstalled,
  hostUpdateRequest = null,
  onReconnectHost,
  localHost = null,
  devices
}: SettingsPaneProps): JSX.Element {
  const meta = SETTINGS_SECTIONS.find((entry) => entry.id === section) ?? SETTINGS_SECTIONS[0]!
  const onHost = meta.group === 'host'
  // A host section's values are that host's, and Unknown while they are not known.
  const unknown = onHost && host.unknown !== null
  // These hold more than one group each and keep their headings; every other
  // section is one group, whose title is the page's and whose hint is the line
  // under it.
  const sole = !['workspace', 'agents', 'programs', 'tools', 'browser'].includes(section)

  return (
    <SettingsPage
      data-settings-section={section}
      data-settings-host={onHost ? host.name : undefined}
      title={meta.label}
      onHost={onHost ? <HostLine name={host.name} /> : undefined}
      hint={meta.hint}
      sole={sole}
      aside={
        // With no host, the empty page offers it in its own place.
        section === 'hosts' && hosts.length > 0 ? (
          <Action data-settings-add-host onClick={onAddHost}>
            <span className="flex items-center gap-1.5">
              <PlusIcon width={12} height={12} />
              Add a host…
            </span>
          </Action>
        ) : undefined
      }
    >
      {onHost && host.unknown?.kind === 'away' && (
        <ReachNotice
          name={host.name}
          failedAt={host.unknown.failedAt}
          onShowHosts={onShowSection === undefined ? undefined : () => onShowSection('hosts')}
        />
      )}

      {section === 'general' && (
        <Group name="walkthrough" title="Walkthrough" hint="The steps Floti opened with the first time.">
          <Actions>
            <Action data-settings-show-walkthrough onClick={onShowWalkthrough}>
              Show the walkthrough
            </Action>
          </Actions>
        </Group>
      )}

      {section === 'programs' && (
        <>
          <ClaudeGroup
            host={host}
            unknown={unknown}
            status={status}
            statusError={statusError}
            checking={checking}
            onRecheck={onRecheck}
            onLocate={onLocateClaude}
            onClearOverride={onClearClaudeOverride}
          />
          <ShellGroup
            host={host}
            unknown={unknown}
            shell={terminalShell}
            onChange={onTerminalShellChange}
            shells={shells}
            onLocate={onLocateShell}
            onShellPath={onShellPath}
          />
        </>
      )}

      {section === 'tools' && (
        <ToolsGroup
          hostName={host.name}
          unknown={unknown}
          sessionMcp={sessionMcp}
          onSessionMcpChange={onSessionMcpChange}
          desktopMcp={desktopMcp}
          onDesktopMcpChange={onDesktopMcpChange}
          browserMcp={browserMcp}
          onBrowserMcpChange={onBrowserMcpChange}
          sessionAgents={sessionAgents}
          onSessionAgentsChange={onSessionAgentsChange}
        />
      )}

      {section === 'workspace' && (
        <>
          <WorkspaceGroup
            host={host}
            unknown={unknown}
            roots={roots}
            projectCount={projectCount}
            scanning={scanning}
            onAddRoot={onAddRoot}
            onAddRootPath={onAddRootPath}
            onRemoveRoot={onRemoveRoot}
            pinnedProjects={pinnedProjects}
            onUnpinProject={onUnpinProject}
          />
          {/* This PC's templates: a host on another computer's are chosen and
              kept in its own New Workspace dialog. */}
          {!host.remote && (
            <TemplatesGroup
              total={templateCount}
              dir={templatesDir}
              onManage={onManageTemplates}
              onReveal={onRevealTemplates}
            />
          )}
        </>
      )}

      {section === 'appearance' && (
        <AppearanceGroup
          settings={appearance}
          onChange={onAppearanceChange}
          themes={themes}
          state={themeState}
          onOpenFolder={onOpenThemesFolder}
          onDuplicate={onDuplicateTheme}
        />
      )}

      {section === 'terminal' && (
        <TerminalGroup terminal={terminal} onChange={onTerminalChange} fontStack={terminalFontStack} />
      )}

      {section === 'sessions' && (
        <SessionsGroup
          restore={restoreWithoutAsking}
          onRestoreChange={onRestoreWithoutAskingChange}
          inParentTab={showChildrenInParentTab}
          onInParentTabChange={onShowChildrenInParentTabChange}
          quit={quitSessions}
          onQuitChange={onQuitSessionsChange}
          onStopHost={onStopHost}
          hostLogFile={hostLogFile}
        />
      )}

      {section === 'agents' && (
        <>
          <AgentsGroup overview={agentsOverview} />
          <AgentLimitsGroup
            hostName={host.name}
            unknown={unknown}
            limits={agentLimits}
            onLimitsChange={onAgentLimitsChange}
          />
        </>
      )}

      {section === 'hosts' && (
        <HostsSettings
          hosts={hosts}
          onAdd={onAddHost}
          onRemove={onRemoveHost}
          onQuitChange={onHostQuitChange}
          install={hostInstall}
          onInstalled={onHostInstalled}
          updateRequest={hostUpdateRequest}
          onReconnect={onReconnectHost}
          localHost={localHost}
        />
      )}

      {section === 'devices' && <DevicesSettings {...devices} />}

      {section === 'files' && <FilesGroup wrap={filesWrap} onWrapChange={onFilesWrapChange} />}

      {section === 'browser' && (
        <BrowserGroup
          reach={browserReach}
          onReachChange={onBrowserReachChange}
          search={browserSearch}
          onSearchChange={onBrowserSearchChange}
          mcpLocalOnly={browserMcpLocalOnly}
          onMcpLocalOnlyChange={onBrowserMcpLocalOnlyChange}
          onShowTools={onShowSection === undefined ? undefined : () => onShowSection('tools')}
        />
      )}

      {section === 'archive' && (
        <ArchiveGroup
          unknown={unknown}
          stats={archiveStats}
          maxBytes={transcriptArchiveMaxBytes}
          onMaxBytesChange={onTranscriptArchiveMaxBytesChange}
        />
      )}

      {section === 'updates' && (
        <UpdatesGroup
          host={host}
          unknown={unknown}
          appVersion={appVersion}
          releasesUrl={releasesUrl}
          update={update}
          error={updateError}
          checking={updateChecking}
          onCheckNow={onCheckForUpdate}
          onInstall={onInstallUpdate}
          onOpenReleases={onOpenReleases}
          updateCheck={updateCheck}
          onUpdateCheckChange={onUpdateCheckChange}
          onShowHosts={onShowSection === undefined ? undefined : () => onShowSection('hosts')}
        />
      )}
    </SettingsPage>
  )
}

// ---------------------------------------------------------------------------
// Programs
// ---------------------------------------------------------------------------

/**
 * The Claude CLI the picked host runs: where it found it, what version it is,
 * the config it reads. "Could not look" is Unknown, never "not found".
 */
function ClaudeGroup({
  host,
  unknown,
  status,
  statusError,
  checking,
  onRecheck,
  onLocate,
  onClearOverride
}: {
  host: SettingsHostView
  unknown: boolean
  status: SetupClaudeStatus | null
  statusError: string | null
  checking: boolean
  onRecheck: () => void
  onLocate: (() => void) | undefined
  onClearOverride: () => void
}): JSX.Element {
  // A host that cannot be read shows nothing it said before: Unknown.
  const shown = unknown ? null : status
  const found = shown !== null && shown.path !== null && shown.version !== null
  const overridden = shown?.source === 'setting'
  return (
    <Group name="claude" title={labelOf('claudePath')} hint={hintOf('claudePath')}>
      <div className="pb-1">
        <Verdict
          tone={shown === null ? 'todo' : found ? (shown.tested ? 'ok' : 'warn') : 'warn'}
          text={
            unknown
              ? 'Unknown'
              : shown === null
                ? statusError === null
                  ? 'Looking…'
                  : `Unknown: ${statusError}`
                : found
                  ? overridden
                    ? 'Set by you'
                    : `Found on ${host.remote ? host.name : 'this machine'}`
                  : shown.path !== null
                    ? (shown.error ?? 'Found, but it did not report its version.')
                    : (shown.error ?? 'Not found.')
          }
        />
        {shown !== null && statusError !== null && (
          <p data-settings-status-error className="mt-2 text-[11.5px] text-warn">
            Could not check again: {statusError}
          </p>
        )}
        <dl className="mt-2.5 space-y-1.5">
          <Fact label="Path">
            <span data-settings-claude-path title={shown?.path ?? ''}>
              {shown?.path ?? NOTHING}
            </span>
          </Fact>
          <Fact label="Version">
            <span data-settings-claude-version>{shown?.version ?? NOTHING}</span>
          </Fact>
          <Fact label="Config">
            <span data-settings-claude-config title={shown?.configDir ?? ''}>
              {shown?.configDir ?? NOTHING}
            </span>
          </Fact>
        </dl>

        {found && shown !== null && !shown.tested && (
          <p
            data-settings-version-warning
            className="mt-3 rounded-well border border-warn/30 bg-warn/10 px-3 py-2 text-[11.5px] leading-[1.55] text-warn"
          >
            Floti was tested against {shown.testedRange.min} up to (not including) {shown.testedRange.max}.{' '}
            {shown.semver ?? shown.version} is outside that, so a flag may have moved. Nothing is blocked.
          </p>
        )}
      </div>

      <Actions>
        <Action data-settings-recheck onClick={onRecheck} disabled={checking || unknown}>
          <RefreshIcon className={cn('mr-1.5 inline', checking && 'animate-spin')} />
          Check again
        </Action>
        <Action
          data-settings-locate
          onClick={() => onLocate?.()}
          disabled={onLocate === undefined || unknown}
          title={
            onLocate === undefined
              ? `The file picker browses this PC, so it cannot choose a claude on ${host.name}`
              : 'Choose the claude program yourself'
          }
        >
          Locate manually…
        </Action>
        <Action
          data-settings-clear-claude
          onClick={onClearOverride}
          disabled={!overridden || unknown}
          title={
            overridden
              ? 'Forget the executable you picked and use whatever Floti finds'
              : 'Nothing to clear - Floti found this one itself'
          }
        >
          Clear override
        </Action>
      </Actions>
    </Group>
  )
}

/** The shell a project pane opens on the picked host: one it found, one chosen by hand, or detect. */
function ShellGroup({
  host,
  unknown,
  shell,
  onChange,
  shells,
  onLocate,
  onShellPath
}: {
  host: SettingsHostView
  unknown: boolean
  shell: string | null
  onChange: (path: string | null) => void
  shells: DetectedShell[]
  onLocate: (() => void) | undefined
  onShellPath: (path: string) => Promise<string | null>
}): JSX.Element {
  // A shell picked by hand may not be one of the detected ones, and dropping it
  // out of the list would make the picker show "Detect automatically" for a
  // setting that is doing no such thing.
  const shellOptions =
    shell !== null && !shells.some((each) => sameFile(each.path, shell))
      ? [...shells, { path: shell, name: fileName(shell), label: 'Chosen by you', args: [] }]
      : shells
  return (
    <Group name="shell" title={labelOf('terminalShell')} hint={hintOf('terminalShell')}>
      {unknown ? (
        <Row label="Default shell">
          <UnknownValue />
        </Row>
      ) : (
        <div className="flex flex-col items-stretch gap-1.5">
          <div className="flex items-center gap-2">
            <Select
              value={shell ?? ''}
              label="Default shell"
              data-settings-terminal-shell={shell ?? ''}
              onChange={(value) => onChange(value === '' ? null : value)}
            >
              <option value="">Detect automatically</option>
              {shellOptions.map((each) => (
                <option key={each.path} value={each.path}>
                  {each.name} - {each.label}
                </option>
              ))}
            </Select>
            {onLocate !== undefined && (
              <Action data-settings-terminal-shell-locate onClick={onLocate}>
                Choose…
              </Action>
            )}
          </div>
          <TypedPathField
            label="Shell path"
            placeholder={host.remote ? `Or type the full path to a shell on ${host.name}` : 'Or type the full path to a shell'}
            action="Use"
            mark="data-settings-terminal-shell"
            onPath={onShellPath}
          />
        </div>
      )}
    </Group>
  )
}

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

/**
 * Which of Floti's tools the picked host's sessions get. Each is that host's,
 * enforced there for every session it runs, whichever window started it; a
 * session on it that turns one off with `desktop_set` changes this value.
 */
function ToolsGroup({
  hostName,
  unknown,
  sessionMcp,
  onSessionMcpChange,
  desktopMcp,
  onDesktopMcpChange,
  browserMcp,
  onBrowserMcpChange,
  sessionAgents,
  onSessionAgentsChange
}: {
  hostName: string
  unknown: boolean
  sessionMcp: boolean
  onSessionMcpChange: (next: boolean) => void
  desktopMcp: boolean
  onDesktopMcpChange: (next: boolean) => void
  browserMcp: boolean
  onBrowserMcpChange: (next: boolean) => void
  sessionAgents: boolean
  onSessionAgentsChange: (next: boolean) => void
}): JSX.Element {
  const rows: readonly {
    key: 'sessionMcp' | 'desktopMcp' | 'browserMcp' | 'sessionAgents'
    mark: string
    on: boolean
    set: (next: boolean) => void
    hint: string
  }[] = [
    {
      key: 'sessionMcp',
      mark: 'data-settings-session-mcp',
      on: sessionMcp,
      set: onSessionMcpChange,
      hint: sessionMcp
        ? `Sessions can list every Claude Code session running on ${hostName}: its name, its directory, whether it is busy or waiting on you. Not their conversations.`
        : `Off on ${hostName}. Sessions started there get no floti-sessions tools and give themselves no titles.`
    },
    {
      key: 'desktopMcp',
      mark: 'data-settings-desktop-mcp',
      on: desktopMcp,
      set: onDesktopMcpChange,
      hint: desktopMcp
        ? `Sessions can open Floti’s views beside them, read and change its settings and diagnose it, in a window attached to ${hostName}.`
        : `Off on ${hostName}. Sessions started there get no floti-desktop tools and no floti desktop.`
    },
    {
      key: 'browserMcp',
      mark: 'data-settings-browser-mcp',
      on: browserMcp,
      set: onBrowserMcpChange,
      hint: browserMcp
        ? `Sessions can open pages of their own in the browser of a window attached to ${hostName}, and drive a page of yours once you share it.`
        : `Off on ${hostName}. Sessions started there get no browser tools.`
    },
    {
      key: 'sessionAgents',
      mark: 'data-settings-session-agents',
      on: sessionAgents,
      set: onSessionAgentsChange,
      hint: sessionAgents
        ? `Sessions started on ${hostName} get the floti-agents tools and floti agent, within the limits in Agents. A profile can turn it off for its own sessions.`
        : `Off on ${hostName}. Sessions started there get no floti-agents tools and no floti agent, whichever window started them.`
    }
  ]
  return (
    <Group name="tools" title={`What sessions on ${hostName} get`}>
      {rows.map((row) =>
        unknown ? (
          <UnknownCheckRow key={row.key} label={labelOf(row.key)} />
        ) : (
          <CheckRow key={row.key} label={labelOf(row.key)} hint={row.hint}>
            <span {...dataHook(row.mark, String(row.on))}>
              <Checkbox checked={row.on} onChange={() => row.set(!row.on)} label={labelOf(row.key)} />
            </span>
          </CheckRow>
        )
      )}
    </Group>
  )
}

// ---------------------------------------------------------------------------
// Workspace
// ---------------------------------------------------------------------------

/** The folders the picked host scans, and the projects pinned out of their workspace there. */
function WorkspaceGroup({
  host,
  unknown,
  roots,
  projectCount,
  scanning,
  onAddRoot,
  onAddRootPath,
  onRemoveRoot,
  pinnedProjects,
  onUnpinProject
}: {
  host: SettingsHostView
  unknown: boolean
  roots: string[]
  projectCount: number
  scanning: boolean
  onAddRoot: () => void
  onAddRootPath: ((path: string) => Promise<string | null>) | undefined
  onRemoveRoot: (path: string) => void
  pinnedProjects: string[]
  onUnpinProject: (path: string) => void
}): JSX.Element {
  return (
    <Group
      name="workspace"
      title="Workspace"
      hint={
        unknown
          ? undefined
          : scanning
            ? `${count(roots.length, 'folder')} · scanning…`
            : `${count(roots.length, 'folder')} · ${count(projectCount, 'project')}`
      }
    >
      <p className="text-[12.5px] text-fg">{labelOf('scanRoots')}</p>
      {unknown ? (
        <p className="mt-1 pb-1">
          <UnknownValue />
        </p>
      ) : roots.length === 0 ? (
        <p className="mt-1 pb-1 text-[12px] text-fg-subtle">Floti scans nothing until you say what to scan.</p>
      ) : (
        <ul className="mt-2 overflow-hidden rounded-well border border-border bg-surface-sunken">
          {roots.map((root) => (
            <li
              key={root}
              data-settings-root={root}
              className="flex items-center gap-2 border-b border-border px-3 py-1.5 last:border-b-0"
            >
              <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-fg-muted" title={root}>
                {root}
              </span>
              <button
                type="button"
                data-settings-remove-root={root}
                onClick={() => onRemoveRoot(root)}
                aria-label={`Stop scanning ${root}`}
                title={`Stop scanning ${root}`}
                className={cn(
                  'grid size-5 shrink-0 place-items-center rounded text-fg-subtle',
                  'transition-colors hover:bg-hover hover:text-danger'
                )}
              >
                <CloseIcon width={11} height={11} />
              </button>
            </li>
          ))}
        </ul>
      )}

      <p className="mt-2.5 text-[11px] leading-[1.55] text-fg-subtle">
        A <em>workspace</em> is a folder with a <code className="font-mono">workspace.yaml</code>, and lets one session
        compose several repos&rsquo; skills. Removing a folder only stops Floti scanning it.
      </p>

      {!unknown &&
        (host.remote && onAddRootPath !== undefined ? (
          <div className="mt-3">
            <TypedPathField
              label={`Folder on ${host.name}`}
              placeholder={`The full path of a folder on ${host.name}`}
              action="Add"
              mark="data-settings-add-root"
              onPath={onAddRootPath}
            />
          </div>
        ) : (
          !host.remote && (
            <Actions>
              <Action data-settings-add-root onClick={onAddRoot} primary={roots.length === 0}>
                Add a folder
              </Action>
            </Actions>
          )
        ))}

      <Divider />

      {/* Pins are made on the sidebar, on the row of the project being
          pinned - the star is there because that is where the decision is.
          This is where the set is legible all at once, which is what the
          sidebar cannot be: its Pinned section shows a vanished folder as
          one row saying so, and a list of the paths is what says *which*
          path, in a form that can be compared with what is on disk. */}
      <p className="text-[12.5px] text-fg">{labelOf('pinnedProjects')}</p>
      <p className="mt-0.5 mb-2 text-[11px] leading-[1.55] text-fg-subtle">{hintOf('pinnedProjects')}</p>

      {unknown ? (
        <UnknownValue />
      ) : pinnedProjects.length === 0 ? (
        <p className="text-[12px] text-fg-subtle">Nothing is pinned.</p>
      ) : (
        <ul className="overflow-hidden rounded-well border border-border bg-surface-sunken">
          {pinnedProjects.map((path) => (
            <li
              key={path}
              data-settings-pinned={path}
              className="flex items-center gap-2 border-b border-border px-3 py-1.5 last:border-b-0"
            >
              <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-fg-muted" title={path}>
                {path}
              </span>
              {/* The roots list above wears the same shape with a danger
                  hover on its ×, and this one deliberately does not:
                  un-scanning a folder takes projects out of the tree, and
                  un-pinning one moves a row back into its workspace. */}
              <button
                type="button"
                data-settings-unpin={path}
                onClick={() => onUnpinProject(path)}
                aria-label={`Unpin ${path}`}
                title={`Unpin ${path}`}
                className={cn(
                  'grid size-5 shrink-0 place-items-center rounded text-fg-subtle',
                  'transition-colors hover:bg-hover hover:text-fg'
                )}
              >
                <CloseIcon width={11} height={11} />
              </button>
            </li>
          ))}
        </ul>
      )}
    </Group>
  )
}

// ---------------------------------------------------------------------------
// Appearance
// ---------------------------------------------------------------------------

export type AppearanceSettings = Pick<
  AppSettings,
  'theme' | 'themeDark' | 'themeLight' | 'paneGap' | 'cornerRadius' | 'density' | 'accentColor'
>

const DENSITY_LABEL: Record<Density, string> = { comfortable: 'Comfortable', compact: 'Compact' }

/**
 * Themes, shape and accent.
 *
 * Under the cards are **two slots and a switch**, not one choice: `themeDark`
 * and `themeLight` each name a theme, and `theme` says which slot is on
 * screen - one of them always, or whichever Windows is in. That is what lets
 * "Follow Windows" mean something once there is more than one theme of a kind,
 * and it is why the title bar's three-way toggle still works unchanged.
 *
 * So a card writes its **own kind's** slot. Following Windows, that is all it
 * does, and a card of the kind Windows is not in today is recorded for later -
 * which is why, while following, the two cards in the slots carry a tag saying
 * when each one shows: a click that changed nothing on screen still visibly
 * moved something. Not following, a card also pins the preference to its
 * kind, so whatever was clicked is what is on screen.
 *
 * The selected ring is on the theme the main process says is showing, never on
 * this pane's guess at it - a slot naming a deleted file resolves to a
 * built-in, and the ring says so.
 */
function AppearanceGroup({
  settings,
  onChange,
  themes,
  state,
  onOpenFolder,
  onDuplicate
}: {
  settings: AppearanceSettings
  onChange: (patch: Partial<AppearanceSettings>) => void
  themes: ThemeListing | null
  state: ThemeState | null
  onOpenFolder: () => void
  onDuplicate: (id: string) => void
}): JSX.Element {
  const all = themes?.themes ?? []
  const follow = settings.theme === 'system'
  const showing = state?.applied ?? null

  // What each slot actually shows: its theme, or the built-in standing in for
  // one that is not there - the same rule `resolveTheme` applies.
  const slotTheme = (kind: ThemeKind): ThemeDefinition | undefined => {
    const wanted = kind === 'dark' ? settings.themeDark : settings.themeLight
    return all.find((t) => t.id === wanted) ?? all.find((t) => t.id === DEFAULT_THEME_ID[kind])
  }
  const dark = slotTheme('dark')
  const light = slotTheme('light')

  const pick = (theme: ThemeDefinition): void => {
    const slot = theme.kind === 'dark' ? { themeDark: theme.id } : { themeLight: theme.id }
    onChange(follow ? slot : { ...slot, theme: theme.kind })
  }

  const problems = [
    ...(themes?.errors ?? []),
    ...all.flatMap((t) => t.problems.map((message) => ({ file: t.file ?? t.id, message })))
  ]

  return (
    <Group
      name="appearance"
      title="Appearance"
      hint="Themes change Floti's chrome. Terminals keep their own colours."
    >
      <div
        role="radiogroup"
        aria-label={labelOf('theme')}
        data-settings-theme={settings.theme}
        className="grid grid-cols-[repeat(auto-fill,minmax(164px,1fr))] gap-2.5 pt-1"
      >
        {all.map((theme) => (
          <ThemeCard
            key={theme.id}
            theme={theme}
            showing={showing?.id === theme.id}
            slot={
              !follow ? null : theme.id === dark?.id ? 'dark' : theme.id === light?.id ? 'light' : null
            }
            onPick={() => pick(theme)}
          />
        ))}
      </div>

      <label className="mt-3 flex items-center gap-2.5 text-[12.5px] text-fg-muted">
        <Checkbox
          checked={follow}
          // Turning it off pins whatever is showing now, so the click changes
          // nothing on screen - it only stops the next Windows change moving it.
          onChange={() => onChange({ theme: follow ? (state?.resolved ?? 'dark') : 'system' })}
          label="Follow Windows"
          mark="data-settings-theme-follow"
        />
        <span>
          Follow Windows: {light?.name ?? 'the light theme'} when Windows is light,{' '}
          {dark?.name ?? 'the dark theme'} when it is dark
        </span>
      </label>

      {problems.length > 0 && (
        <ul
          data-settings-theme-problems={problems.length}
          className="mt-3 space-y-1 rounded-raised border border-warn/30 bg-warn/10 px-3 py-2 text-[11.5px] leading-[1.5]"
        >
          {problems.map((problem, index) => (
            <li key={`${problem.file}:${String(index)}`} className="flex gap-2">
              <span className="shrink-0 font-mono text-[11px] text-warn">{problem.file}</span>
              <span className="min-w-0 text-fg-muted">{problem.message}</span>
            </li>
          ))}
        </ul>
      )}

      <div className="mt-3 flex flex-wrap items-center gap-x-4 gap-y-2 rounded-raised border border-border bg-surface px-3.5 py-3">
        <div className="min-w-[220px] flex-1">
          <p className="text-[12.5px] text-fg">Your own themes</p>
          <p className="mt-0.5 text-[11px] leading-[1.5] text-fg-subtle">
            One JSON file of colours per theme. Saving one repaints Floti.
          </p>
          {/* Its own line, truncated from the left of nothing: a path wrapped
              mid-sentence breaks at whichever backslash the width lands on. */}
          <p
            data-settings-themes-dir
            title={themes?.dir}
            className="mt-1 truncate font-mono text-[10.5px] text-fg-muted select-text"
          >
            {themes?.dir ?? NOTHING}
          </p>
        </div>
        <div className="flex shrink-0 gap-2">
          <Action
            data-settings-theme-duplicate={showing?.id ?? ''}
            disabled={showing === null}
            onClick={() => {
              if (showing !== null) onDuplicate(showing.id)
            }}
          >
            Duplicate {showing?.name ?? 'theme'}
          </Action>
          <Action data-settings-themes-open onClick={onOpenFolder}>
            Open folder
          </Action>
        </div>
      </div>

      <div className="mt-2">
        <Row label={labelOf('paneGap')} hint={hintOf('paneGap')}>
          <Stepper
            value={settings.paneGap}
            min={PANE_GAP.min}
            max={PANE_GAP.max}
            label={labelOf('paneGap')}
            data-settings-gap={String(settings.paneGap)}
            onChange={(paneGap) => onChange({ paneGap })}
          />
        </Row>

        <Divider />

        <Row label={labelOf('cornerRadius')} hint={hintOf('cornerRadius')}>
          <Stepper
            value={settings.cornerRadius}
            min={CORNER_RADIUS.min}
            max={CORNER_RADIUS.max}
            label={labelOf('cornerRadius')}
            data-settings-radius={String(settings.cornerRadius)}
            onChange={(cornerRadius) => onChange({ cornerRadius })}
          />
        </Row>

        <Divider />

        <Row label={labelOf('density')} hint={hintOf('density')}>
          <div
            role="radiogroup"
            aria-label={labelOf('density')}
            className="flex items-center gap-0.5 rounded-well border border-border bg-surface-sunken p-0.5"
          >
            {DENSITY_MODES.map((mode) => (
              <button
                key={mode}
                type="button"
                role="radio"
                data-settings-density={mode}
                aria-checked={settings.density === mode}
                onClick={() => onChange({ density: mode })}
                className={cn(
                  'rounded-raised px-2.5 py-1 text-[11.5px] transition-colors',
                  settings.density === mode ? SEGMENT_ON : 'text-fg-subtle hover:text-fg'
                )}
              >
                {DENSITY_LABEL[mode]}
              </button>
            ))}
          </div>
        </Row>

        <Divider />

        <Row label={labelOf('accentColor')} hint={hintOf('accentColor')}>
          <div role="radiogroup" aria-label={labelOf('accentColor')} className="flex items-center gap-2.5 px-1">
            <AccentSwatch
              name={`${showing?.name ?? 'The theme'}'s own`}
              mark="theme"
              color={state === null ? null : themeAccent(all, state)}
              selected={settings.accentColor === null}
              onPick={() => onChange({ accentColor: null })}
            />
            {ACCENT_SWATCHES.map((swatch) => (
              <AccentSwatch
                key={swatch.hex}
                name={swatch.name}
                mark={swatch.hex}
                color={
                  showing === null
                    ? swatch.hex
                    : deriveAccent(showing.tokens, showing.kind, swatch.hex).accent
                }
                selected={settings.accentColor === swatch.hex}
                onPick={() => onChange({ accentColor: swatch.hex })}
              />
            ))}
          </div>
        </Row>
      </div>
    </Group>
  )
}

/**
 * The accent the theme on screen has of its own. Read from the listing rather
 * than from `state`, because with a chosen accent `state` carries the chosen
 * one - and this swatch is the way back to the other.
 */
function themeAccent(all: ThemeDefinition[], state: ThemeState): string {
  return all.find((t) => t.id === state.applied.id)?.tokens.accent ?? state.applied.tokens.accent
}

function AccentSwatch({
  name,
  mark,
  color,
  selected,
  onPick
}: {
  name: string
  mark: string
  color: string | null
  selected: boolean
  onPick: () => void
}): JSX.Element {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={selected}
      aria-label={name}
      title={name}
      data-settings-accent={mark}
      onClick={onPick}
      // A theme colour, so an inline style - the value is data the main
      // process parsed and re-spelled, never a literal in this file.
      style={color === null ? undefined : { backgroundColor: color }}
      className={cn(
        'size-5 shrink-0 rounded-full ring-offset-2 ring-offset-surface-raised transition-shadow',
        selected ? 'ring-[1.5px] ring-fg' : 'ring-1 ring-transparent hover:ring-border-strong'
      )}
    />
  )
}

/**
 * One theme as a card: a miniature of the window in that theme's own colours,
 * its name and kind, and - while following Windows - which slot it fills.
 */
function ThemeCard({
  theme,
  showing,
  slot,
  onPick
}: {
  theme: ThemeDefinition
  showing: boolean
  slot: ThemeKind | null
  onPick: () => void
}): JSX.Element {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={showing}
      data-settings-theme-card={theme.id}
      title={theme.file === null ? `${theme.name}, built in` : `${theme.name}, from ${theme.file}`}
      onClick={onPick}
      className={cn(
        'min-w-0 rounded-raised border bg-surface p-1.5 text-left transition-colors',
        showing
          ? 'border-accent ring-1 ring-accent'
          : 'border-border hover:border-border-strong hover:bg-hover'
      )}
    >
      <ThemeSwatch tokens={theme.tokens} />
      <span className="flex min-w-0 items-center gap-1.5 px-1 pt-2 pb-0.5">
        <span className="truncate text-[12.5px] font-medium text-fg">{theme.name}</span>
        <span className="shrink-0 text-[11px] text-fg-subtle">
          {theme.kind === 'dark' ? 'Dark' : 'Light'}
        </span>
        <span className="flex-1" />
        {slot !== null && (
          <span
            data-settings-theme-slot={slot}
            className="shrink-0 rounded-raised border border-border px-1 text-[10px] leading-[15px] text-fg-muted"
          >
            when {slot}
          </span>
        )}
        {showing && <CheckIcon width={12} height={12} className="shrink-0 text-accent-text" />}
      </span>
    </button>
  )
}

/**
 * The window, small: canvas, rail, sidebar island and two session panes whose
 * bodies are the terminal's fixed ground - because that is what every theme
 * actually looks like with sessions open. Inline colours, from the theme's own
 * parsed tokens; the terminal is the one token the stylesheet owns.
 */
function ThemeSwatch({ tokens }: { tokens: ThemeTokens }): JSX.Element {
  const island = { backgroundColor: tokens.surface, borderColor: tokens['border-strong'] }
  const bar = (width: string, color: string): JSX.Element => (
    <span className="h-[3px] rounded-full" style={{ width, backgroundColor: color }} />
  )
  return (
    <span
      aria-hidden
      className="flex h-[78px] gap-[3px] overflow-hidden rounded-raised p-[5px]"
      style={{ backgroundColor: tokens.bg }}
    >
      <span className="flex w-[6px] shrink-0 flex-col gap-[3px] pt-px">
        <span className="h-[6px] rounded-xs" style={{ backgroundColor: tokens.accent }} />
        <span className="h-[6px] rounded-xs" style={{ backgroundColor: tokens['border-strong'] }} />
        <span className="h-[6px] rounded-xs" style={{ backgroundColor: tokens['border-strong'] }} />
      </span>
      <span
        className="flex w-[36px] shrink-0 flex-col gap-[4px] rounded-xs border px-[3px] py-[5px]"
        style={island}
      >
        {bar('70%', tokens['fg-subtle'])}
        {bar('90%', tokens.accent)}
        {bar('60%', tokens['fg-subtle'])}
        {bar('80%', tokens['fg-subtle'])}
      </span>
      {[1, 0.8].map((grow) => (
        <span
          key={grow}
          className="flex min-w-0 flex-col overflow-hidden rounded-xs border"
          style={{ ...island, flexGrow: grow, flexBasis: 0 }}
        >
          <span
            className="flex h-[10px] shrink-0 items-center px-[3px]"
            style={{ borderBottom: `1px solid ${tokens.border}` }}
          >
            <span className="h-[4px] w-[18px] rounded-xs" style={{ backgroundColor: tokens.active }} />
          </span>
          <span className="flex-1 bg-terminal" />
        </span>
      ))}
    </span>
  )
}

// ---------------------------------------------------------------------------
// Workspace templates
// ---------------------------------------------------------------------------

/**
 * The templates this desktop has, and the two ways to reach them.
 *
 * The path is a button rather than a line of text, and that is the group's
 * whole argument for existing beside the manager: **there is no file editor in
 * Floti for these**. A template is a folder, editing one is a job for the editor
 * somebody already has, and a settings group that named the folder without
 * opening it would be describing a destination it declined to take you to.
 *
 * The count is stated rather than the list drawn. Which template is which is
 * the manager's question and the picker's; what belongs here is the one fact
 * Settings is for - whether this desktop has any, and where they are.
 */
function TemplatesGroup({
  total,
  dir,
  onManage,
  onReveal
}: {
  total: number
  dir: string
  onManage: () => void
  onReveal: () => void
}): JSX.Element {
  return (
    <Group
      name="templates"
      title="Workspace templates"
      hint={total === 0 ? 'none yet' : count(total, 'template')}
    >
      <p className="pb-1 text-[12px] leading-[1.55] text-fg-muted">
        A template is a folder here; creating a workspace from one copies it in. The New Workspace
        dialog picks between them, and the built-in <em>Starter</em> template is always the first
        row whatever is in this folder.
      </p>

      <button
        type="button"
        data-settings-templates-dir
        onClick={onReveal}
        title={`Show ${dir} in Explorer`}
        className="mt-1.5 block w-full truncate rounded-well border border-border bg-surface-sunken px-3 py-1.5 text-left font-mono text-[11px] text-fg-muted transition-colors hover:bg-hover hover:text-fg"
      >
        {dir === '' ? NOTHING : dir}
      </button>
      <p className="mt-1.5 text-[11px] leading-[1.55] text-fg-subtle">
        Edited in your own editor, not in Floti - these are plain files, and most people keep them
        in git. Deleting the folder puts the shipped README and example back at the next start,
        which is the whole of &ldquo;reset&rdquo;.
      </p>

      <Actions>
        <Action data-settings-manage-templates onClick={onManage} primary={total === 0}>
          Manage templates
        </Action>
      </Actions>
    </Group>
  )
}

// ---------------------------------------------------------------------------
// Transcript archive
// ---------------------------------------------------------------------------

/**
 * The ceilings this pane offers.
 *
 * Not the validator's range, which runs from a kilobyte so that a check can
 * drive eviction. These are the sizes a person would choose, and the smallest
 * of them is still four hundred times the whole archive on the machine this was
 * written against (1.47 MB for 21,952 messages out of 311 MB of transcripts).
 */
const ARCHIVE_CEILINGS = [
  { bytes: 256 * 1024 * 1024, label: '256 MB' },
  { bytes: 512 * 1024 * 1024, label: '512 MB' },
  { bytes: 1024 ** 3, label: '1 GB' },
  { bytes: 2 * 1024 ** 3, label: '2 GB' },
  { bytes: 4 * 1024 ** 3, label: '4 GB' },
  { bytes: 8 * 1024 ** 3, label: '8 GB' }
] as const

/**
 * What Floti has kept, and the one knob over it.
 *
 * The figures are stated rather than drawn. A bar with no number on it would
 * be exactly the wrong answer here: the whole reason this group exists is that
 * `desktop.db` is the user's file and a feature that grows it silently is one they
 * find out about from their disk. So the sentence says how many conversations,
 * how many messages, how many bytes, and how many bytes out of how many - and,
 * when the ceiling has actually bitten, how many conversations it dropped.
 *
 * There is no on/off switch and that is deliberate; the field's comment in
 * `types.ts` has the argument. What can be turned down is the ceiling.
 */
function ArchiveGroup({
  unknown,
  stats: read,
  maxBytes,
  onMaxBytesChange
}: {
  /** The picked host cannot be read: its archive and its ceiling are Unknown. */
  unknown: boolean
  stats: ArchiveStats | null
  maxBytes: number
  onMaxBytesChange: (bytes: number) => void
}): JSX.Element {
  const stats = unknown ? null : read
  const used = stats?.storedBytes ?? 0
  const percent = maxBytes > 0 ? (used / maxBytes) * 100 : 0
  // Two significant figures below 1%, so a real archive on a default ceiling
  // reads "0.00014%" rather than "0%" - which would say "nothing is stored"
  // about something that is.
  const percentText = percent === 0 ? '0%' : percent < 1 ? `${percent.toPrecision(2)}%` : `${percent.toFixed(1)}%`

  // A ceiling set outside the offered list - by a check, or by a build that
  // offered different sizes - is added to the list rather than silently
  // replaced by the nearest one, which would make the select lie about what is
  // in force the moment it painted.
  const choices = ARCHIVE_CEILINGS.some((choice) => choice.bytes === maxBytes)
    ? ARCHIVE_CEILINGS
    : [{ bytes: maxBytes, label: formatBytes(maxBytes) }, ...ARCHIVE_CEILINGS]

  return (
    <Group
      name="archive"
      title="Transcript archive"
      hint="Claude Code deletes transcripts on its own schedule. Floti keeps a compressed copy of each one first, and never writes to Claude's files."
    >
      <div className="pb-1">
        <Verdict
          data-settings-archive-state={
            unknown ? 'unknown' : stats === null ? 'reading' : stats.sessions === 0 ? 'empty' : 'holding'
          }
          tone={stats === null ? 'todo' : 'ok'}
          text={
            unknown
              ? 'Unknown'
              : stats === null
                ? 'Reading…'
                : stats.sessions === 0
                  ? 'Nothing archived yet'
                  : `${count(stats.sessions, 'conversation')} kept`
          }
        />
        <dl className="mt-2.5 space-y-1.5">
          <Fact label="Kept">
            <span data-settings-archive-sessions={String(stats?.sessions ?? 0)}>
              {unknown
                ? NOTHING
                : `${count(stats?.sessions ?? 0, 'session')} · ${count(stats?.messages ?? 0, 'message')}`}
            </span>
          </Fact>
          <Fact label="Stored">
            <span data-settings-archive-stored={String(used)}>
              {unknown ? NOTHING : `${formatBytes(used)} of ${formatBytes(maxBytes)} (${percentText})`}
            </span>
          </Fact>
          <Fact label="Dropped">
            <span data-settings-archive-evicted={String(stats?.evictedSessions ?? 0)}>
              {unknown ? NOTHING : count(stats?.evictedSessions ?? 0, 'session')}
            </span>
          </Fact>
        </dl>
      </div>

      <Divider />

      <Row label={labelOf('transcriptArchiveMaxBytes')} hint={hintOf('transcriptArchiveMaxBytes')}>
        {unknown ? (
          <UnknownValue />
        ) : (
          <Select
            value={String(maxBytes)}
            label="How much of the database the archive may use"
            data-settings-archive-max={String(maxBytes)}
            onChange={(value) => onMaxBytesChange(Number(value))}
          >
            {choices.map((choice) => (
              <option key={choice.bytes} value={String(choice.bytes)}>
                {choice.label}
              </option>
            ))}
          </Select>
        )}
      </Row>
    </Group>
  )
}

// ---------------------------------------------------------------------------
// Updates
// ---------------------------------------------------------------------------

/** The things this group can be saying, named so a driver can read one. */
export type UpdateOutcomeState =
  | 'unheard'
  | 'unavailable'
  | 'installing'
  | 'checking'
  | 'installFailed'
  | 'unreachable'
  | 'unasked'
  | 'newer'
  | 'current'

export interface UpdateOutcome {
  state: UpdateOutcomeState
  tone: 'ok' | 'warn' | 'todo'
  text: string
}

/**
 * The whole of what this group says, as one pure function of what it was given.
 *
 * Separate from the component because the states are the interesting part
 * and a component cannot be asked what it would say. Exported so a driver could
 * reach it - though a check should write its own expected sentences rather than
 * import these, because one that asked this function what this function says
 * would be asserting that the code agrees with itself.
 *
 * A host that does not update comes first: it is why nothing else here can
 * happen, and a dev run says so rather than showing an error. A check that
 * could not reach the release is `todo` and not `warn`: offline is an
 * expected answer, and a warning triangle would blame the user's network for
 * a request the host made on its own. An update that failed is `warn`: it
 * was asked for and did not happen.
 */
export function updateOutcome(
  update: UpdateView | null,
  checking: boolean,
  appVersion: string | null,
  error: string | null = null
): UpdateOutcome {
  if (update === null) {
    return error === null
      ? { state: 'unheard', tone: 'todo', text: "Waiting to hear from this PC's host." }
      : { state: 'unheard', tone: 'todo', text: `Could not hear from this PC's host - ${error}.` }
  }
  if (update.unavailable === 'readOnly') {
    return {
      state: 'unavailable',
      tone: 'todo',
      text: 'This is a development run: its host checks for nothing and updates nothing.'
    }
  }
  if (update.unavailable === 'notStandalone') {
    return {
      state: 'unavailable',
      tone: 'todo',
      text: "This PC's host does not run floti installed on its own, so it checks for nothing and updates nothing."
    }
  }
  if (update.installing !== null) return { state: 'installing', tone: 'todo', text: update.installing.line }
  if (checking) return { state: 'checking', tone: 'todo', text: 'Looking for a newer release…' }
  if (update.installFailed !== null) {
    return { state: 'installFailed', tone: 'warn', text: `The update did not finish - ${update.installFailed}` }
  }
  if (error !== null) return { state: 'unreachable', tone: 'todo', text: `Could not ask this PC's host - ${error}.` }
  if (update.failed !== null) {
    return { state: 'unreachable', tone: 'todo', text: `Could not check for a newer release - ${update.failed}` }
  }
  if (update.newest === null) {
    return { state: 'unasked', tone: 'todo', text: "This PC's host has not checked for a newer release yet." }
  }
  if (update.newer) {
    return {
      state: 'newer',
      tone: 'todo',
      text: `${update.newest} is available. This build is ${appVersion ?? 'unknown'}.`
    }
  }
  return {
    state: 'current',
    tone: 'ok',
    text: `${appVersion ?? update.newest} is the newest release. Checked ${checkedWhen(update.checkedAt)}.`
  }
}

/**
 * "just now" or "5m ago", from the instant the check carries.
 *
 * `formatAge` answers `now` under a minute, which does not take the word "ago",
 * and an unparseable instant answers nothing at all rather than `NaNm` - a row
 * or a payload from another build is a fact about the past, and the sentence
 * around this one is still true without its last clause.
 */
function checkedWhen(checkedAt: string | null): string {
  if (checkedAt === null) return 'this session'
  const at = Date.parse(checkedAt)
  if (!Number.isFinite(at)) return 'this session'
  const age = formatAge(at)
  return age === 'now' ? 'just now' : `${age} ago`
}

/**
 * The Files view. One row: whether a long line wraps, which the toggle in a
 * file's own status line writes too, so the two never disagree.
 */
function FilesGroup({ wrap, onWrapChange }: { wrap: boolean; onWrapChange: (wrap: boolean) => void }): JSX.Element {
  return (
    <Group name="files" title="Files" hint="How a file read from the Files view is shown.">
      <Row label={labelOf('filesWrap')} hint={hintOf('filesWrap')}>
        <span data-settings-files-wrap={String(wrap)}>
          <Checkbox checked={wrap} onChange={() => onWrapChange(!wrap)} label="Wrap long lines in files" />
        </span>
      </Row>
    </Group>
  )
}

/**
 * The browser pane's posture on this desktop, and how far Claude may take it.
 *
 * Three rows, because there are three decisions this desktop makes: where the
 * pane may go at all, what the address bar does with a phrase, and whether
 * Claude's browser tools are held to localhost when the pane is not. Whether a
 * host's sessions get the browser tools at all is that host's (Floti host ›
 * Tools), and the group says so. Everything else about the pane - downloads
 * denied, every permission but clipboard writing and fullscreen denied,
 * self-signed certificates accepted for loopback and nowhere else - is not a
 * setting and never will be. Those are the app's postures, and a posture with
 * a switch on it is a posture somebody turns off on the afternoon it gets in
 * their way.
 *
 * The two reach rows are worded as a pair, because the rule between them is
 * an intersection and the failure to avoid is somebody setting the second and
 * believing it widened the first.
 */
function BrowserGroup({
  reach,
  onReachChange,
  search,
  onSearchChange,
  mcpLocalOnly,
  onMcpLocalOnlyChange,
  onShowTools
}: {
  reach: BrowserReach
  onReachChange: (reach: BrowserReach) => void
  search: BrowserSearch
  onSearchChange: (search: BrowserSearch) => void
  mcpLocalOnly: boolean
  onMcpLocalOnlyChange: (next: boolean) => void
  /** Opens Floti host › Tools, where a host decides whether its sessions get the browser tools. */
  onShowTools: (() => void) | undefined
}): JSX.Element {
  return (
    <>
      <Group name="browser" title="Browser" untitled>
        <Row label={labelOf('browserReach')} hint={hintOf('browserReach', reach)}>
          <Select
            value={reach}
            label="Where the browser pane may go"
            data-settings-browser-reach={reach}
            onChange={(value) => onReachChange(value as BrowserReach)}
          >
            <option value="web">Anywhere I navigate to</option>
            <option value="local">This machine only</option>
          </Select>
        </Row>

        <Row label={labelOf('browserSearch')} hint={hintOf('browserSearch', search)}>
          <Select
            value={search}
            label={labelOf('browserSearch')}
            data-settings-browser-search={search}
            onChange={(value) => onSearchChange(value as BrowserSearch)}
          >
            {BROWSER_SEARCH_ENGINES.map((choice) => (
              <option key={choice} value={choice}>
                {browserSearchName(choice) ?? 'Off'}
              </option>
            ))}
          </Select>
        </Row>
      </Group>

      <Group name="browser-claude" title="Claude in this browser">
        {/* Not disabled when a host's browser tools are off, deliberately. The
            value is a standing preference: somebody who has confined the tools
            and then turns them off for an afternoon should find them still
            confined when they turn them back on, and a control that greys out
            is a control whose state people stop trusting. */}
        <CheckRow
          label={labelOf('browserMcpLocalOnly')}
          hint={hintOf('browserMcpLocalOnly', mcpLocalOnly) ?? settingEntry('browserMcpLocalOnly').description}
        >
          <span data-settings-browser-mcp-local={String(mcpLocalOnly)}>
            <Checkbox
              checked={mcpLocalOnly}
              onChange={() => onMcpLocalOnlyChange(!mcpLocalOnly)}
              label={labelOf('browserMcpLocalOnly')}
            />
          </span>
        </CheckRow>
        <p
          data-settings-browser-tools-pointer
          className="mt-2 border-t border-border pt-2 text-[11.5px] leading-[1.5] text-fg-muted"
        >
          Whether a host’s sessions get the browser tools at all is set on that host:{' '}
          {onShowTools === undefined ? (
            'Floti host › Tools'
          ) : (
            <button type="button" onClick={onShowTools} className="text-accent-text hover:underline">
              Floti host › Tools
            </button>
          )}
          .
        </p>
      </Group>
    </>
  )
}

/**
 * What this desktop does with the sessions it starts: after a crash, in a
 * parent's tab, and when it quits; and this PC's host, which they run in.
 */
function SessionsGroup({
  restore,
  onRestoreChange,
  inParentTab,
  onInParentTabChange,
  quit,
  onQuitChange,
  onStopHost,
  hostLogFile
}: {
  restore: boolean
  onRestoreChange: (next: boolean) => void
  inParentTab: boolean
  onInParentTabChange: (next: boolean) => void
  quit: QuitSessions
  onQuitChange: (next: QuitSessions) => void
  onStopHost: () => void
  hostLogFile: string | null
}): JSX.Element {
  return (
    <Group name="sessions" title="Sessions" hint="What this desktop does with the sessions it starts.">
      <Row label={labelOf('restoreWithoutAsking')} hint={hintOf('restoreWithoutAsking', restore)}>
        <span data-settings-restore-without-asking={String(restore)}>
          <Checkbox
            checked={restore}
            onChange={() => onRestoreChange(!restore)}
            label={labelOf('restoreWithoutAsking')}
          />
        </span>
      </Row>
      <Row label={labelOf('showChildrenInParentTab')} hint={settingEntry('showChildrenInParentTab').description}>
        <span data-settings-children-in-parent-tab={String(inParentTab)}>
          <Checkbox
            checked={inParentTab}
            onChange={() => onInParentTabChange(!inParentTab)}
            label={labelOf('showChildrenInParentTab')}
          />
        </span>
      </Row>
      {/* Only this PC's host: the one a desktop runs its sessions in. */}
      <Row label={labelOf('quitSessions')} hint={hintOf('quitSessions', quit)}>
        <Select
          value={quit}
          label={labelOf('quitSessions')}
          data-settings-quit-sessions={quit}
          onChange={(value) => onQuitChange(value === 'keep' ? 'keep' : 'end')}
        >
          <option value="end">End the sessions it started</option>
          <option value="keep">Keep them running</option>
        </Select>
      </Row>
      <Row
        label="This PC’s host"
        hint="Stopping it ends every session running in it, whichever device started it, after asking. Floti starts it again when it next needs it."
      >
        <Action data-settings-stop-host onClick={onStopHost}>
          Stop host
        </Action>
      </Row>
      {hostLogFile !== null && (
        <Row
          label="Log file"
          hint="Where this PC’s host writes what it says about itself, a crash included. It holds no key, code, prompt or session output."
        >
          <span
            data-settings-host-log
            className="block max-w-[22rem] truncate font-mono text-[11px] text-fg-muted select-text"
            title={hostLogFile}
          >
            {hostLogFile}
          </span>
        </Row>
      )}
    </Group>
  )
}

/** The picked host's agents in a sentence, with Open Agents: the section's first row. */
function AgentsGroup({ overview }: { overview: AgentsOverview | undefined }): JSX.Element | null {
  if (overview === undefined) return null
  return (
    <Group name="agents" title="Agents" untitled>
      <Row
        label={
          overview.count === null
            ? `Agents on ${overview.hostName}`
            : `${overview.count} ${overview.count === 1 ? 'agent' : 'agents'} on ${overview.hostName}`
        }
        hint={`Lead, worker, reviewer and the rest: what a session started as one of them runs with.${
          overview.folder === null ? '' : ` Kept as files in ${overview.folder}.`
        }`}
      >
        <span data-settings-agents-open>
          <Action onClick={overview.onOpen}>Open Agents</Action>
        </span>
      </Row>
    </Group>
  )
}

/** The limits the Limits group draws, in its order, with the hook a driver reaches each by. */
const LIMIT_ROWS: readonly {
  key: keyof AgentLimitSettings
  bounds: { min: number; max: number }
  mark: string
}[] = [
  { key: 'agentChildrenPerParent', bounds: AGENT_CHILDREN_PER_PARENT, mark: 'data-settings-agent-children-per-parent' },
  { key: 'agentCommandsPerParent', bounds: AGENT_COMMANDS_PER_PARENT, mark: 'data-settings-agent-commands-per-parent' },
  { key: 'agentSessionsPerHost', bounds: AGENT_SESSIONS_PER_HOST, mark: 'data-settings-agent-sessions-per-host' },
  { key: 'agentDepth', bounds: AGENT_DEPTH, mark: 'data-settings-agent-depth' },
  { key: 'agentStartsPerParent', bounds: AGENT_STARTS_PER_PARENT, mark: 'data-settings-agent-starts-per-parent' }
]

/**
 * The limits on a family of agents, as the picked host keeps them.
 *
 * The host enforces every number, whether or not a window is open, and
 * sessions the user starts count toward none of them. A limit is a whole
 * number inside the range the host accepts, so a value outside it is brought
 * to the nearest end rather than refused by the host after the fact.
 */
function AgentLimitsGroup({
  hostName,
  unknown,
  limits,
  onLimitsChange
}: {
  hostName: string
  unknown: boolean
  limits: AgentLimitSettings
  onLimitsChange: (patch: Partial<AgentLimitSettings>) => void
}): JSX.Element {
  return (
    <Group
      name="agent-limits"
      title="Limits"
      hint={`${hostName} enforces each of these whether or not a window is open. Sessions you start yourself count toward none of them.`}
    >
      {LIMIT_ROWS.map(({ key, bounds, mark }, at) => (
        <div key={key}>
          {at > 0 && <Divider />}
          <Row label={labelOf(key)} hint={hintOf(key)}>
            {unknown ? (
              <UnknownValue />
            ) : (
              <NumberField
                value={limits[key]}
                min={bounds.min}
                max={bounds.max}
                label={labelOf(key)}
                {...dataHook(mark, String(limits[key]))}
                onCommit={(value) => onLimitsChange({ [key]: value } as Partial<AgentLimitSettings>)}
              />
            )}
          </Row>
        </div>
      ))}
    </Group>
  )
}

/**
 * Releases: what this build is, what the newest one is, and the two things a
 * person may ask this PC's host: check now, and update.
 *
 * Check now is live whatever the tick says - the setting governs whether the
 * host asks by itself, not whether the user may - and Release notes is live
 * whatever the last check found, because "up to date", "could not check" and
 * "never checked" are the states in which somebody wants to look for
 * themselves. Update is offered only while a newer release exists and the
 * host updates; the desktop closes when the update is ready for it and the update
 * opens it again, its sessions running throughout. On a host that does not
 * update, the sentence says why and both buttons that would ask it rest.
 *
 * A host on another computer shows only its own tick: its floti is checked
 * and updated from Settings › Hosts, which says where its version stands.
 */
function UpdatesGroup({
  host,
  unknown,
  appVersion,
  releasesUrl,
  update,
  error,
  checking,
  onCheckNow,
  onInstall,
  onOpenReleases,
  updateCheck,
  onUpdateCheckChange,
  onShowHosts
}: {
  host: SettingsHostView
  unknown: boolean
  appVersion: string | null
  releasesUrl: string | null
  update: UpdateView | null
  error: string | null
  checking: boolean
  onCheckNow: () => void
  onInstall: () => void
  onOpenReleases: () => void
  updateCheck: boolean
  onUpdateCheckChange: (next: boolean) => void
  onShowHosts: (() => void) | undefined
}): JSX.Element {
  const outcome = updateOutcome(update, checking, appVersion, error)
  const updates = update !== null && update.unavailable === null
  const installing = update !== null && update.installing !== null
  const tick = (
    <Row label={labelOf('updateCheck')} hint={hintOf('updateCheck')}>
      {unknown ? (
        <UnknownValue />
      ) : (
        <span data-settings-update-check={String(updateCheck)}>
          <Checkbox
            checked={updateCheck}
            onChange={() => onUpdateCheckChange(!updateCheck)}
            label="Look for a newer floti once a day and install it"
          />
        </span>
      )}
    </Row>
  )

  if (host.remote) {
    return (
      <Group
        name="updates"
        title="Updates"
        hint={`${host.name} makes the only request Floti makes there on its own: once a day it reads floti.dev's signed list of releases.`}
      >
        {tick}
        <p data-settings-update-elsewhere className="mt-2 text-[11.5px] leading-[1.5] text-fg-muted">
          Where {host.name}’s floti stands, and Update floti, are in{' '}
          {onShowHosts === undefined ? (
            'Settings › Hosts'
          ) : (
            <button type="button" onClick={onShowHosts} className="text-accent-text hover:underline">
              Settings › Hosts
            </button>
          )}
          .
        </p>
      </Group>
    )
  }

  return (
    // The posture belongs to the group rather than to the tick: it is true of
    // everything in here, the buttons included.
    <Group
      name="updates"
      title="Updates"
      hint="This PC's host makes the only request Floti makes on its own: once a day it reads floti.dev's signed list of releases. Floti itself contacts nothing."
    >
      <div className="pb-1">
        <Verdict data-settings-update-outcome={outcome.state} tone={outcome.tone} text={outcome.text} />
        <dl className="mt-2.5 space-y-1.5">
          <Fact label="Version">
            <span data-settings-app-version>{appVersion ?? NOTHING}</span>
          </Fact>
          {/* What the host's last check that read the release list found,
              kept across a check that could not: the release did not stop
              existing because the network went away. */}
          <Fact label="Latest">
            <span data-settings-latest-version>{update?.newest ?? NOTHING}</span>
          </Fact>
        </dl>
      </div>

      <Divider />

      {tick}

      <Actions>
        {update?.newer === true && updates && (
          <Action
            data-settings-update-install
            primary
            onClick={onInstall}
            disabled={installing}
            title="Replaces floti and Floti. Floti closes when the update is ready and opens again; every session keeps running."
          >
            Update to {update.newest}
          </Action>
        )}
        {/* Disabled while a check is in flight, while an update runs and on
            a host that does not update - never because the tick above is
            off. */}
        <Action data-settings-update-now onClick={onCheckNow} disabled={checking || installing || !updates}>
          <RefreshIcon className={cn('mr-1.5 inline', checking && 'animate-spin')} />
          Check now
        </Action>
        <Action
          data-settings-releases
          onClick={onOpenReleases}
          disabled={releasesUrl === null}
          title={releasesUrl ?? 'Not known until app:info lands'}
        >
          Release notes
        </Action>
      </Actions>
    </Group>
  )
}

// ---------------------------------------------------------------------------
// Terminal
// ---------------------------------------------------------------------------

const CURSOR_LABEL: Record<TerminalCursorStyle, string> = {
  block: 'Block',
  underline: 'Underline',
  bar: 'Bar'
}

/**
 * What the preview well renders.
 *
 * Box-drawing, block elements and the letters that are told apart only by their
 * shapes, because those are what a font chosen for its letterforms drops or
 * gets wrong - and Claude Code's whole interface is box-drawing.
 *
 * The rules are built from the body's own length rather than typed out: a
 * hand-counted box is exactly the kind of thing that goes one character out and
 * then looks like a rendering bug in the font someone is evaluating.
 */
const PREVIEW_BODY = '  0O1lI  {}[]  <=>  ~-  ░▒▓█  The quick brown fox  '
const PREVIEW_LINES = [
  `╭${'─'.repeat(PREVIEW_BODY.length)}╮`,
  `│${PREVIEW_BODY}│`,
  `╰${'─'.repeat(PREVIEW_BODY.length)}╯`
]

/**
 * Terminal appearance, and how tall a project page's shell is.
 *
 * Colour is deliberately absent. The xterm palette is fixed in both themes
 * (DESIGN.md par. 6, "foreign-ground islands") and asserted pixel-for-pixel by
 * the fidelity checks; making it settable is a design amendment, not a row.
 */
function TerminalGroup({
  terminal,
  onChange,
  fontStack
}: {
  terminal: TerminalSettings
  onChange: (patch: Partial<TerminalSettings>) => void
  fontStack: string
}): JSX.Element {
  return (
    <Group
      name="terminal"
      title="Terminal"
      hint="Applies to every open terminal as you change it."
    >
      <FontRow terminal={terminal} onChange={onChange} />

      <Divider />

      <Row label={labelOf('terminalFontSize')} hint={hintOf('terminalFontSize')}>
        <Stepper
          value={terminal.terminalFontSize}
          min={TERMINAL_FONT_SIZE.min}
          max={TERMINAL_FONT_SIZE.max}
          label="Terminal font size"
          data-settings-terminal-size={String(terminal.terminalFontSize)}
          onChange={(terminalFontSize) => onChange({ terminalFontSize })}
        />
      </Row>

      <Divider />

      <Row label={labelOf('terminalCursorStyle')} hint={hintOf('terminalCursorStyle')}>
        <div
          role="radiogroup"
          aria-label="Cursor style"
          className="flex items-center gap-0.5 rounded-well border border-border bg-surface-sunken p-0.5"
        >
          {TERMINAL_CURSOR_STYLES.map((style) => (
            <button
              key={style}
              type="button"
              role="radio"
              data-settings-terminal-cursor={style}
              aria-checked={terminal.terminalCursorStyle === style}
              onClick={() => onChange({ terminalCursorStyle: style })}
              className={cn(
                'rounded-raised px-2.5 py-1 text-[11.5px] transition-colors',
                terminal.terminalCursorStyle === style
                  ? SEGMENT_ON
                  : 'text-fg-subtle hover:text-fg'
              )}
            >
              {CURSOR_LABEL[style]}
            </button>
          ))}
        </div>
      </Row>

      <Divider />

      <Row label={labelOf('terminalCursorBlink')} hint={hintOf('terminalCursorBlink')}>
        <span data-settings-terminal-blink={String(terminal.terminalCursorBlink)}>
          <Checkbox
            checked={terminal.terminalCursorBlink}
            onChange={() => onChange({ terminalCursorBlink: !terminal.terminalCursorBlink })}
            label="Blink the terminal cursor"
          />
        </span>
      </Row>

      <Divider />

      <Row label={labelOf('terminalScrollback')} hint={hintOf('terminalScrollback')}>
        <NumberField
          value={terminal.terminalScrollback}
          min={TERMINAL_SCROLLBACK.min}
          max={TERMINAL_SCROLLBACK.max}
          label="Scrollback lines"
          data-settings-terminal-scrollback={String(terminal.terminalScrollback)}
          onCommit={(terminalScrollback) => onChange({ terminalScrollback })}
        />
      </Row>

      <Divider />

      {/* The drag handle above the shell is the control for this; the row is
          here so the value is findable and so a drag that landed somewhere
          silly can be typed back. Which is also why it is a field rather than a
          stepper: nobody nudges this while watching it, because the thing it
          moves is on a different tab. */}
      <Row label={labelOf('projectShellHeightPct')} hint={hintOf('projectShellHeightPct')}>
        <NumberField
          value={terminal.projectShellHeightPct}
          min={PROJECT_SHELL_HEIGHT_PCT.min}
          max={PROJECT_SHELL_HEIGHT_PCT.max}
          label="Project shell height"
          data-settings-shell-height={String(terminal.projectShellHeightPct)}
          onCommit={(projectShellHeightPct) => onChange({ projectShellHeightPct })}
        />
      </Row>

      {/* Plain DOM at the chosen font, not an xterm instance: the point is to
          see the choice before any terminal repaints, and a second terminal in
          the settings pane would be a second pty to own. The ground and the
          foreground are the terminal's own fixed pair (DESIGN.md par. 6), which
          is why they are hex here - the same exception the session tab takes. */}
      <div
        data-settings-terminal-preview
        // `lineHeight: normal` rather than a ratio of the point size, because
        // that is what a terminal row actually is: xterm measures a span in the
        // configured font and takes its box, so 14px type sits on 19px rows.
        // Pinning the rows to the point size instead would squash the preview
        // and pull the box-drawing apart at exactly the size it is meant to
        // show off.
        style={{
          fontFamily: fontStack,
          fontSize: `${String(terminal.terminalFontSize)}px`,
          lineHeight: 'normal'
        }}
        className="mt-3 overflow-x-auto rounded-well border border-border bg-terminal px-3 py-2.5 text-[#c9d1d9] select-text"
      >
        {PREVIEW_LINES.map((line) => (
          <div key={line} className="whitespace-pre">
            {line}
          </div>
        ))}
      </div>
      <p className="mt-1.5 text-[11px] leading-[1.5] text-fg-subtle">
        The terminal keeps its own ground in both themes, so this is what a pane will look like.
      </p>
    </Group>
  )
}

/**
 * The font family row, with the "you do not have that font" hint.
 *
 * The hint is a courtesy, not a guard. Whatever is typed here is put *in front
 * of* the built-in stack rather than replacing it, so a font this machine does
 * not have simply never wins a glyph - and a font that has letters but no
 * box-drawing loses only the box-drawing. That is what makes the field safe to
 * leave open rather than restricting it to a list.
 */
function FontRow({
  terminal,
  onChange
}: {
  terminal: TerminalSettings
  onChange: (patch: Partial<TerminalSettings>) => void
}): JSX.Element {
  const saved = terminal.terminalFontFamily ?? ''
  const [draft, setDraft, reset] = useDraft(saved)

  const commit = (): void => {
    const next = draft.trim()
    if (next === saved) return
    onChange({ terminalFontFamily: next === '' ? null : next })
  }

  return (
    <Row label={labelOf('terminalFontFamily')} hint={hintOf('terminalFontFamily')}>
      <div className="flex flex-col items-end gap-1.5">
        <div className="flex items-center gap-2">
          <input
            type="text"
            data-settings-terminal-font={saved}
            aria-label="Terminal font family"
            placeholder="Built-in stack"
            spellCheck={false}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={commit}
            onKeyDown={(e) => {
              if (e.key === 'Enter') commit()
              if (e.key === 'Escape') reset()
            }}
            className={cn(
              'h-[30px] w-[190px] rounded-well border border-border bg-surface-sunken px-2.5',
              'font-mono text-[11.5px] text-fg placeholder:text-fg-subtle select-text',
              'focus:border-accent focus:outline-none'
            )}
          />
          <Action
            data-settings-terminal-font-clear
            onClick={() => onChange({ terminalFontFamily: null })}
            disabled={terminal.terminalFontFamily === null}
            title={
              terminal.terminalFontFamily === null
                ? 'Nothing to clear - this is the built-in stack'
                : 'Back to the built-in stack'
            }
          >
            Clear
          </Action>
        </div>
        {terminal.terminalFontFamily !== null && !fontInstalled(terminal.terminalFontFamily) && (
          <p
            data-settings-terminal-font-missing={terminal.terminalFontFamily}
            className="max-w-[280px] text-right text-[11px] leading-[1.5] text-warn"
          >
            {terminal.terminalFontFamily} is not installed on this machine, so terminals fall back
            to the built-in stack.
          </p>
        )}
      </div>
    </Row>
  )
}

/**
 * Whether this machine can actually draw that family.
 *
 * Measured, not asked. `document.fonts.check('14px "Whatever"')` is the obvious
 * call and it does not answer this question: the font set it reports on is the
 * document's `@font-face` rules, so a family it has never heard of comes back
 * **true** - verified on Chromium 2026-08-11, where a deliberately nonsense
 * name passed. What does answer it is the oldest trick there is: render a probe
 * string with the family in front of a fallback and again with the fallback
 * alone. A family that resolves changes the width; one that does not cannot.
 *
 * Two fallbacks with very different metrics, because one comparison would call
 * a font missing if it happened to match that fallback's advance exactly. At
 * 72px a real difference is tens of pixels, so this is not a close call.
 *
 * Cached: the answer cannot change while the window is open, and this runs on
 * every render of the row.
 */
const installedFonts = new Map<string, boolean>()

function fontInstalled(family: string): boolean {
  const cached = installedFonts.get(family)
  if (cached !== undefined) return cached

  let answer = true
  const context = document.createElement('canvas').getContext('2d')
  if (context !== null) {
    const width = (font: string): number => {
      context.font = font
      return context.measureText('MWMWiill 0123').width
    }
    answer = ['monospace', 'serif', 'sans-serif'].some(
      (fallback) => width(`72px "${family}", ${fallback}`) !== width(`72px ${fallback}`)
    )
  }
  installedFonts.set(family, answer)
  return answer
}

/**
 * A path, typed or pasted, where a file picker cannot help: a shell the
 * system dialog will not hand over (an app execution alias - PowerShell 7
 * from the Microsoft Store is one), or anything on a host on another
 * computer, which this PC's picker cannot browse. The host checks it; a
 * refused one keeps the text and says why under it, and a kept one clears the
 * field, the list above now showing it.
 */
function TypedPathField({
  label,
  placeholder,
  action,
  mark,
  onPath
}: {
  /** The field's accessible name. */
  label: string
  placeholder: string
  /** The button's word: Use, Add. */
  action: string
  /** The hooks a driver reaches it by: `<mark>-path`, `<mark>-use` and `<mark>-problem`. */
  mark: string
  /** Resolves with why the path was refused, or null once it is kept. */
  onPath: (path: string) => Promise<string | null>
}): JSX.Element {
  const [draft, setDraft] = useState('')
  const [problem, setProblem] = useState<string | null>(null)
  const [checking, setChecking] = useState(false)
  const path = draft.trim()

  const submit = (): void => {
    if (path === '' || checking) return
    setChecking(true)
    void onPath(path)
      .then((refused) => {
        setProblem(refused)
        if (refused === null) setDraft('')
      })
      .finally(() => setChecking(false))
  }

  return (
    <>
      <div className="flex items-center gap-2">
        <input
          type="text"
          {...dataHook(`${mark}-path`)}
          aria-label={label}
          aria-invalid={problem !== null}
          placeholder={placeholder}
          spellCheck={false}
          value={draft}
          onChange={(e) => {
            setDraft(e.target.value)
            setProblem(null)
          }}
          onKeyDown={(e) => {
            if (e.key === 'Enter') submit()
            if (e.key === 'Escape') {
              setDraft('')
              setProblem(null)
            }
          }}
          className={cn(
            'h-[30px] min-w-0 flex-1 rounded-well border bg-surface-sunken px-2.5',
            'font-mono text-[11.5px] text-fg placeholder:font-sans placeholder:text-fg-subtle select-text',
            'focus:outline-none',
            problem === null ? 'border-border focus:border-accent' : 'border-danger/50'
          )}
        />
        <Action {...dataHook(`${mark}-use`)} onClick={submit} disabled={path === '' || checking}>
          {action}
        </Action>
      </div>
      {problem !== null && (
        <p
          role="alert"
          {...dataHook(`${mark}-problem`)}
          className="text-right text-[11px] leading-[1.5] break-words text-danger select-text"
        >
          {problem}
        </p>
      )}
    </>
  )
}

const sameFile = (a: string, b: string): boolean => a.toLowerCase() === b.toLowerCase()
const fileName = (path: string): string => path.split(/[\\/]/).pop() ?? path

// ---------------------------------------------------------------------------

const count = (n: number, noun: string): string =>
  `${n.toLocaleString()} ${noun}${n === 1 ? '' : 's'}`

