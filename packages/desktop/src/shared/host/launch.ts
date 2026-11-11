import type { BadAgent, InstalledBackend, LaunchAgent, LaunchAgents } from '@helm/core/types'
import type { LaunchSessionRequest } from '../ipc'
import { CLAUDE_CODE_BACKEND } from './backend'
import type * as P from './protocol'

/**
 * A launcher's request as the host takes it. A session started as no agent
 * runs on Claude Code unless the launcher chose a backend, because the host's
 * own default is the user's shell; one started as an agent runs on the
 * agent's own first backend unless the launcher chose one, which the host
 * checks the agent allows.
 */
export function launchRequestToHost(req: LaunchSessionRequest): P.LaunchSessionRequest {
  const agent = req.agent ?? null
  return {
    cwd: req.cwd,
    projectPath: req.projectPath ?? null,
    name: req.name ?? null,
    profileId: req.profileId,
    permissionMode: req.permissionMode,
    resume: req.resume,
    backend: req.backend ?? (agent === null ? CLAUDE_CODE_BACKEND : null),
    ...(agent === null ? {} : { agent }),
    ...(req.chosen === undefined ? {} : { chosen: req.chosen }),
    cols: req.cols,
    rows: req.rows
  }
}

/**
 * What a host's backends are to a launcher: its agent backends, the ones
 * that do more than run a command line, in its order.
 */
export function installedBackends(backends: readonly P.BackendDescriptor[]): InstalledBackend[] {
  return backends
    .filter((backend) => backend.features.some((feature) => feature !== 'run'))
    .map((backend) => ({ id: backend.id, name: backend.name }))
}

/** The name of the agent a start without one gets. */
const DEFAULT_AGENT = 'default'

/**
 * A host's agents as a launcher offers them. The default the host makes from
 * its backends (no file) is what a start without an agent gets, so it is not
 * offered: No agent is that. A `default.md` the user wrote is an agent like
 * any other, and so is a shipped agent a host serves with no folder.
 */
export function launchAgentsFromHost(
  listing: P.AgentsListing,
  backends: readonly P.BackendDescriptor[]
): LaunchAgents {
  return {
    agents: listing.agents
      .filter((agent) => agent.file !== null || agent.name !== DEFAULT_AGENT)
      .map(launchAgentFromHost),
    bad: listing.bad.map(badAgentFromHost),
    backends: installedBackends(backends),
    folder: listing.folder,
    readOnly: listing.readOnly,
    shipped: listing.shipped.map(({ name, description }) => ({ name, description }))
  }
}

function launchAgentFromHost(agent: P.AgentSummary): LaunchAgent {
  const { fields } = agent
  return {
    name: agent.name,
    description: agent.description,
    file: agent.file,
    backends: agent.backends,
    listedBackends: fields.backends,
    models: present(fields.models),
    efforts: present(fields.efforts),
    permission: fields.permission,
    profile: fields.profile,
    hash: agent.hash,
    allowedTools: fields.allowedTools,
    disallowedTools: fields.disallowedTools,
    worktree: agent.worktree,
    canStart: agent.canStart,
    cannotRun: agent.cannotRun
  }
}

/** `map` without the keys the protocol's optional values leave unset. */
function present<T>(map: { [key: string]: T | undefined }): Record<string, T> {
  const kept: Record<string, T> = {}
  for (const [key, value] of Object.entries(map)) if (value !== undefined) kept[key] = value
  return kept
}

function badAgentFromHost(bad: P.BadAgentFile): BadAgent {
  return {
    name: bad.name,
    file: bad.file,
    problem: bad.problems.map((problem) => problem.message).join('; '),
    problems: bad.problems.map(({ field, line, message }) => ({ field, line, message })),
    hash: bad.hash
  }
}
