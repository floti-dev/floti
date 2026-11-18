import type {
  AppSettings,
  CachedProject,
  DiscoveryResult,
  FilesStatus,
  GitState,
  LiveSession,
  Profile,
  RestorableSession,
  RestoreOffer,
  SavedPaneLayout,
  SessionActivityState,
  SessionRecord,
  SessionResources,
  SessionsOverview,
  WriteConfigResult
} from '@floti/core'
import type {
  ClaudeAuth,
  ClaudeStatus,
  EffectiveViewRequest,
  LaunchedProfile,
  LaunchedSession,
  ScanStatus
} from '../../ipc'
import type * as P from '../protocol'
import { HOST_SETTING_KEYS, HOST_WORKSPACE_KEYS, type HostSettingKey, type HostWorkspaceKey } from '@floti/core/types'
import { CLAUDE_CODE_BACKEND } from '../backend'

export {
  historyPageFromHost,
  historyQueryForHost,
  historySessionFromHost,
  historySummaryFromHost
} from './history'
export { deliveryFromHost, logFromHost, pluginFromHost, uiFromHost } from './plugins'

/**
 * The host's rows and states in the window's vocabulary. Nothing here reaches
 * Node, so main and the window's host adapters share it.
 *
 * The protocol is backend-neutral: Claude Code's session id is a
 * `conversationId` and a session names its backend. The window still speaks
 * the words it was written in (`claudeSessionId`, `hostSessionId`), so this
 * is where one becomes the other, field by field, and nowhere else.
 */

export function sessionFromHost(record: P.SessionRecord): SessionRecord {
  return {
    id: record.id,
    name: record.name,
    label: record.label,
    // Set only once its agent titled it; a protocol before 2.2 never says.
    ...(record.title == null ? {} : { title: record.title }),
    cwd: record.cwd,
    branch: record.branch,
    projectPath: record.projectPath,
    profileId: record.profileId,
    argv: record.argv,
    claudeSessionId: record.conversationId,
    status: record.status,
    startedAt: record.startedAt,
    endedAt: record.endedAt,
    durationMs: record.durationMs,
    exitCode: record.exitCode,
    // A session a person started names no parent and no agent, and the
    // window's record then has neither.
    ...(record.parentId == null ? {} : { parentId: record.parentId, parentName: record.parentName ?? null }),
    ...(record.agent == null ? {} : { agent: record.agent }),
    // Set only for a command child; a protocol before 2.1 never says.
    ...(record.command == null ? {} : { command: record.command }),
    // Set only when the host ended it; a protocol before 2.1 never says.
    ...(record.stopped === true ? { stopped: true } : {}),
    // Set once it has exited; a protocol before 2.2 never says how.
    ...(record.endedBy == null ? {} : { endedBy: record.endedBy }),
    // Set only on a stopped child kept under its parent; a protocol before 2.2 never says.
    ...(record.kept === true ? { kept: true } : {}),
    // Set only while the child has a report its next turn has not cleared.
    ...(record.lastReport == null ? {} : { lastReport: record.lastReport }),
    // Set only for a child started to keep running when its parent ends.
    ...(record.outlive === true ? { outlive: true } : {}),
    // Absent for a session launched with no mode.
    ...(record.permissionMode == null ? {} : { permissionMode: record.permissionMode }),
    // A host before protocol 2.3 never says.
    ...(record.launchedAs == null ? {} : { launchedAs: { ...record.launchedAs } })
  }
}

/**
 * `record`, a session the window held running, as it ended by the host's
 * word (`session:endings`): its status, end, duration, exit code and who
 * stopped it are the host's; the rest of the row is the window's. A session
 * that ended is no longer kept and has no report.
 */
export function endedFromHost(record: SessionRecord, ending: P.SessionEnding): SessionRecord {
  const { stopped: _stopped, endedBy: _endedBy, kept: _kept, lastReport: _lastReport, ...rest } = record
  return {
    ...rest,
    status: ending.status,
    endedAt: ending.endedAt,
    durationMs: ending.durationMs,
    exitCode: ending.exitCode,
    ...(ending.stopped === true ? { stopped: true } : {}),
    ...(ending.endedBy == null ? {} : { endedBy: ending.endedBy })
  }
}

export function launchedFromHost(launched: P.LaunchedSession): LaunchedSession {
  return {
    session: sessionFromHost(launched.session),
    overlays: launched.overlays,
    composedInstructions: launched.composedInstructions,
    warnings: launched.warnings
  }
}

/** A saved profile: the protocol's and the window's are the same fields. */
export function profileFromHost(profile: P.Profile): Profile {
  return {
    id: profile.id,
    name: profile.name,
    root: profile.root,
    overlays: profile.overlays,
    access: profile.access,
    model: profile.model,
    effort: profile.effort,
    permissionMode: profile.permissionMode,
    agent: profile.agent,
    mcp: profile.mcp,
    openingPrompt: profile.openingPrompt,
    pinnedOrder: profile.pinnedOrder,
    // Only the off choice is a value; null and absent both follow the setting.
    ...(profile.sessionAgents === false ? { sessionAgents: false } : {}),
    // Only the on choice is a value; null and absent are off.
    ...(profile.remoteControl === true ? { remoteControl: true } : {}),
    createdAt: profile.createdAt,
    updatedAt: profile.updatedAt
  }
}

export function launchedProfileFromHost(launched: P.LaunchedProfile): LaunchedProfile {
  return {
    session: sessionFromHost(launched.session),
    profile: profileFromHost(launched.profile),
    overlays: launched.overlays,
    composedInstructions: launched.composedInstructions,
    warnings: launched.warnings
  }
}

export function activityFromHost(states: readonly P.SessionActivityState[]): SessionActivityState[] {
  return states.map((state) => ({
    id: state.id,
    activity: state.activity,
    waitingFor: state.waitingFor,
    message: state.message,
    claudeSessionId: state.conversationId
  }))
}

function liveFromHost(session: P.LiveSession): LiveSession {
  return {
    hostSessionId: session.hostSessionId,
    pid: session.pid,
    registered: session.registered,
    cwd: session.cwd,
    name: session.name,
    activity: session.activity,
    waitingFor: session.waitingFor,
    message: session.message,
    statusSinceMs: session.statusSinceMs,
    version: session.version,
    entrypoint: session.entrypoint,
    startedAtMs: session.startedAtMs,
    claudeSessionId: session.conversationId
  }
}

export function overviewFromHost(overview: P.SessionsOverview): SessionsOverview {
  return { sessions: overview.sessions.map(liveFromHost), readAtMs: overview.readAtMs }
}

/**
 * What each hosted session is holding, field by field. Every nullable field
 * is carried as it came: the host's "could not look" (null) and "nothing
 * there" ([]) stay apart all the way to the pane.
 */
export function resourcesFromHost(sets: readonly P.SessionResources[]): SessionResources[] {
  return sets.map((set) => ({
    id: set.id,
    rootPid: set.rootPid,
    processes:
      set.processes === null
        ? null
        : set.processes.map((process) => ({
            pid: process.pid,
            parentPid: process.parentPid,
            name: process.name,
            commandLine: process.commandLine,
            depth: process.depth,
            ports: process.ports === null ? null : [...process.ports]
          })),
    rootSeen: set.rootSeen,
    ports:
      set.ports === null
        ? null
        : set.ports.map((port) => ({
            port: port.port,
            pid: port.pid,
            process: port.process,
            addresses: [...port.addresses]
          })),
    opaque: set.opaque,
    atMs: set.atMs
  }))
}

/**
 * A config write's answer: a write with no conflict or no other refusal has
 * neither at all, as the desktop's own writer answers.
 */
export function writeConfigResultFromHost(result: P.WriteConfigResult): WriteConfigResult {
  return {
    ok: result.ok,
    file: result.file,
    snapshotId: result.snapshotId,
    unchanged: result.unchanged,
    ...(result.conflict === null ? {} : { conflict: result.conflict }),
    ...(result.error === null ? {} : { error: result.error })
  }
}

/** The window's request for an effective view, with every field it left out said. */
export function effectiveRequestForHost(request: EffectiveViewRequest): P.EffectiveViewRequest {
  return {
    profileId: request.profileId ?? null,
    cwd: request.cwd ?? null,
    overlays: request.overlays ?? []
  }
}

/**
 * The host's offer, with the pane layout this window last saved: the layout is
 * per device (it stays in the desktop's database), and it names each session by the
 * host's row id, which is the id the window's tabs carry.
 */
export function offerFromHost(offer: P.RestoreOffer, layout: SavedPaneLayout | null): RestoreOffer {
  const sessions: RestorableSession[] = offer.sessions.map((session) => ({
    id: session.id,
    name: session.name,
    cwd: session.cwd,
    branch: session.branch,
    profile: session.profile,
    profileGone: session.profileGone,
    lastAt: session.lastAt,
    blocked: session.blocked,
    // A host before protocol 2.3 names neither.
    ...(session.agent == null ? {} : { agent: session.agent }),
    ...(session.agentProblem == null ? {} : { agentProblem: session.agentProblem }),
    // A host before protocol 3.0 names neither.
    ...(session.conversationId == null ? {} : { conversationId: session.conversationId }),
    ...(session.title == null ? {} : { title: session.title }),
    ...(session.titleUnreadable === true ? { titleUnreadable: true } : {}),
    ...(session.profileProblem == null ? {} : { profileProblem: session.profileProblem })
  }))
  return {
    sessions,
    elsewhere: offer.elsewhere,
    // A host before protocol 3.0 does not say when they were lost.
    ...(offer.lostAt == null ? {} : { lostAt: offer.lostAt }),
    layout
  }
}

export { HOST_SETTING_KEYS, HOST_WORKSPACE_KEYS, type HostSettingKey, type HostWorkspaceKey }

/**
 * The device's settings with the host's laid over (`HOST_WORKSPACE_KEYS`), or
 * the device's own while there are none. The `claude` the host's sessions run
 * is the window's `claudePath`: the host's chosen executable for Claude Code,
 * or null where the host finds its own.
 */
export function settingsWithHost(own: AppSettings, host: P.HostSettings | null): AppSettings {
  if (host === null) return own
  return {
    ...own,
    scanRoots: host.scanRoots,
    pinnedProjects: host.pinnedProjects,
    firstRunCompletedAt: host.firstRunCompletedAt,
    updateCheck: host.updateCheck,
    claudePath: host.backendPaths[CLAUDE_CODE_BACKEND] ?? null,
    terminalShell: host.terminalShell,
    transcriptArchiveMaxBytes: host.transcriptArchiveMaxBytes,
    browserMcp: host.browserMcp,
    sessionMcp: host.sessionMcp,
    desktopMcp: host.desktopMcp,
    sessionAgents: host.sessionAgents,
    agentChildrenPerParent: host.agentChildrenPerParent,
    agentSessionsPerHost: host.agentSessionsPerHost,
    agentDepth: host.agentDepth,
    agentStartsPerParent: host.agentStartsPerParent,
    agentCommandsPerParent: host.agentCommandsPerParent
  }
}

/**
 * The host's executables with Claude Code's set to `claude`, or taken out so
 * the host finds its own: `backendPaths` replaces the whole map, so every
 * other backend's is kept as `host` holds it.
 */
export function backendPathsWith(
  host: P.HostSettings | null,
  claude: string | null
): P.HostSettings['backendPaths'] {
  const others = Object.fromEntries(
    Object.entries(host?.backendPaths ?? {}).filter(([backend]) => backend !== CLAUDE_CODE_BACKEND)
  )
  return claude === null ? others : { ...others, [CLAUDE_CODE_BACKEND]: claude }
}

/**
 * A window's settings patch, split into what the host keeps and what the
 * device keeps. `host` is the host's settings as last heard, which a change
 * of the `claude` keeps every other backend's executable from.
 */
export function splitSettingsPatch(
  patch: Partial<AppSettings>,
  host: P.HostSettings | null = null
): {
  host: P.HostSettingsPatch
  own: Partial<AppSettings>
} {
  const kept: P.HostSettingsPatch = {}
  const own: Partial<AppSettings> = { ...patch }
  if (patch.scanRoots !== undefined) kept.scanRoots = patch.scanRoots
  if (patch.pinnedProjects !== undefined) kept.pinnedProjects = patch.pinnedProjects
  if (patch.firstRunCompletedAt !== undefined) kept.firstRunCompletedAt = patch.firstRunCompletedAt
  if (patch.updateCheck !== undefined) kept.updateCheck = patch.updateCheck
  if (patch.claudePath !== undefined) kept.backendPaths = backendPathsWith(host, patch.claudePath)
  if (patch.terminalShell !== undefined) kept.terminalShell = patch.terminalShell
  if (patch.transcriptArchiveMaxBytes !== undefined) kept.transcriptArchiveMaxBytes = patch.transcriptArchiveMaxBytes
  if (patch.browserMcp !== undefined) kept.browserMcp = patch.browserMcp
  if (patch.sessionMcp !== undefined) kept.sessionMcp = patch.sessionMcp
  if (patch.desktopMcp !== undefined) kept.desktopMcp = patch.desktopMcp
  if (patch.sessionAgents !== undefined) kept.sessionAgents = patch.sessionAgents
  if (patch.agentChildrenPerParent !== undefined) kept.agentChildrenPerParent = patch.agentChildrenPerParent
  if (patch.agentSessionsPerHost !== undefined) kept.agentSessionsPerHost = patch.agentSessionsPerHost
  if (patch.agentDepth !== undefined) kept.agentDepth = patch.agentDepth
  if (patch.agentStartsPerParent !== undefined) kept.agentStartsPerParent = patch.agentStartsPerParent
  if (patch.agentCommandsPerParent !== undefined) kept.agentCommandsPerParent = patch.agentCommandsPerParent
  for (const key of HOST_WORKSPACE_KEYS) delete own[key]
  return { host: kept, own }
}

/** A repository's state: one git answered has no `error` at all, as the desktop's own discovery says. */
export function gitStateFromHost(state: P.GitState | null | undefined): GitState | null {
  if (state === null || state === undefined) return null
  const { error, ...rest } = state
  return error === null ? rest : { ...rest, error }
}

/** Git state by project path. */
export function gitStatesFromHost(states: Partial<Record<string, P.GitState | null>>): Record<string, GitState | null> {
  const out: Record<string, GitState | null> = {}
  for (const [path, state] of Object.entries(states)) out[path] = gitStateFromHost(state)
  return out
}

export function discoveryFromHost(result: P.DiscoveryResult): DiscoveryResult {
  return { ...result, projects: result.projects.map((project) => ({ ...project, git: gitStateFromHost(project.git) })) }
}

export function cachedProjectsFromHost(rows: readonly P.CachedProject[]): CachedProject[] {
  return rows.map((row) => ({ ...row, git: gitStateFromHost(row.git) }))
}

/** A folder's git letters, by root-relative path; every path the host names has one. */
export function filesStatusFromHost(status: P.FilesStatus): FilesStatus {
  if (status.files === null) return { ...status, files: null }
  const files: Record<string, P.GitFileState> = {}
  for (const [path, state] of Object.entries(status.files)) if (state !== undefined) files[path] = state
  return { ...status, files }
}

/** A scan's state: a scan that did not fail has no `error` at all, as the desktop's own scan says. */
export function scanStatusFromHost(status: P.ScanStatus): ScanStatus {
  return status.error === null ? { running: status.running } : { running: status.running, error: status.error }
}

const AUTH: Record<'true' | 'false', ClaudeAuth> = { true: 'authenticated', false: 'unauthenticated' }

/** Why a host's answer says nothing about Claude Code: it has no such backend to ask. */
export const NO_CLAUDE_BACKEND = 'The local host has no Claude Code backend, so whether Claude Code is installed is unknown.'

/**
 * What the host found of Claude Code, in the setup pane's words. A host with
 * no Claude Code backend did not look, so there is no status: null, which is
 * Unknown - never "not installed" - and has no tested range to give either:
 * the range is the host's backend's (`TESTED_VERSIONS`), and the desktop keeps none
 * of its own. Callers turn null into a failed read carrying `NO_CLAUDE_BACKEND`.
 */
export function claudeStatusFromHost(status: P.BackendStatus): ClaudeStatus
export function claudeStatusFromHost(status: P.BackendStatus | undefined): ClaudeStatus | null
export function claudeStatusFromHost(status: P.BackendStatus | undefined): ClaudeStatus | null {
  if (status === undefined) return null
  return {
    path: status.path,
    source: status.source,
    version: status.version,
    semver: status.semver,
    tested: status.tested,
    testedRange: status.testedRange,
    configDir: status.configDir ?? '',
    configDirExists: status.configDirExists,
    auth: status.signedIn.state === 'known' ? AUTH[status.signedIn.value ? 'true' : 'false'] : 'unknown',
    authSignal: status.signInSignal,
    error: status.error
  }
}
