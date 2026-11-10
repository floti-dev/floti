import type {
  BadAgent,
  EffortLevel,
  InstalledBackend,
  LaunchAgent,
  LaunchAgents,
  PermissionMode,
  Profile,
  Project
} from '@helm/core/types'

/**
 * Starting a session as an agent: what the launchers' pickers show once an
 * agent is picked, and what the agent asks of the folder.
 *
 * **An agent fills the pickers nobody touched.** Its model and effort for the
 * backend, its permission and its profile become the values shown, and each
 * stays the person's to change before starting; a picker the person already
 * changed keeps their choice, and picking another agent changes only the
 * untouched ones. A profile picked beside an agent wins over the agent's.
 * What is shown is exactly what is sent.
 */

/** The models the Model picker offers for Claude Code, besides Default. */
export const MODEL_CHOICES = ['opus', 'sonnet', 'haiku', 'fable'] as const

/** The backend a session started as no agent runs on when the host has it. */
const CLAUDE_CODE = 'claude-code'

/** A picker the person changed, with its value; absent while it follows the folder or the agent. */
export interface Touched {
  profile?: { id: number | null }
  mode?: { mode: PermissionMode | null }
  model?: { model: string | null }
  effort?: { effort: EffortLevel | null }
  backend?: { id: string }
}

/** Which pickers' values are the agent's, in the order the note names them. */
export type AgentField = 'model' | 'effort' | 'permissions' | 'profile'

/** What a launch would start with, as the pickers show it. */
export interface ResolvedLaunch {
  backend: string | null
  /** The backends the Backend picker offers: the agent's, or every installed one. */
  backends: readonly InstalledBackend[]
  profile: Profile | null
  model: string | null
  effort: EffortLevel | null
  mode: PermissionMode | null
  /** The pickers whose value came from the agent and nobody changed. */
  fromAgent: readonly AgentField[]
}

/**
 * The values a launch in a folder shows: the person's choices, then the
 * agent's, then the folder's profile's.
 */
export function resolveLaunch(options: {
  agent: LaunchAgent | null
  installed: readonly InstalledBackend[]
  profiles: readonly Profile[]
  /** The profile the folder starts with (`profileFor`). */
  folderProfile: Profile | null
  touched: Touched
}): ResolvedLaunch {
  const { agent, installed, profiles, folderProfile, touched } = options
  const backends =
    agent === null
      ? installed
      : agent.backends.map((id) => installed.find((backend) => backend.id === id) ?? { id, name: id })
  const backend =
    (touched.backend !== undefined && backends.some((each) => each.id === touched.backend?.id)
      ? touched.backend.id
      : undefined) ??
    (agent === null ? backends.find((each) => each.id === CLAUDE_CODE)?.id : undefined) ??
    backends[0]?.id ??
    null
  const fromAgent: AgentField[] = []

  const agentsProfile =
    agent?.profile == null ? null : (profiles.find((each) => sameName(each.name, agent.profile ?? '')) ?? null)
  let profile: Profile | null
  if (touched.profile !== undefined) {
    profile = profiles.find((each) => each.id === touched.profile?.id) ?? null
  } else if (agentsProfile !== null) {
    profile = agentsProfile
  } else {
    profile = folderProfile
  }

  const agentsModel = agent !== null && backend !== null ? keyed(agent.models, backend) : undefined
  let model: string | null
  if (touched.model !== undefined) model = touched.model.model
  else if (agentsModel !== undefined) model = agentsModel
  else model = profile?.model ?? null

  const agentsEffort = agent !== null && backend !== null ? keyed(agent.efforts, backend) : undefined
  let effort: EffortLevel | null
  if (touched.effort !== undefined) effort = touched.effort.effort
  else if (agentsEffort !== undefined) effort = agentsEffort
  else effort = profile?.effort ?? null

  let mode: PermissionMode | null
  if (touched.mode !== undefined) mode = touched.mode.mode
  else if (agent?.permission != null) mode = agent.permission
  else mode = profile?.permissionMode ?? null

  if (touched.model === undefined && agentsModel !== undefined) fromAgent.push('model')
  if (touched.effort === undefined && agentsEffort !== undefined) fromAgent.push('effort')
  if (touched.mode === undefined && agent?.permission != null) fromAgent.push('permissions')
  if (touched.profile === undefined && agentsProfile !== null) fromAgent.push('profile')
  return { backend, backends, profile, model, effort, mode, fromAgent }
}

/**
 * The line under the pickers that says whose their values are: "Model,
 * effort, permissions and profile are research-lead's. Change any of them
 * before starting." None when the agent set none of them.
 */
export function agentValuesNote(agent: string, fields: readonly AgentField[]): string | null {
  if (fields.length === 0) return null
  const first = fields[0]!
  const named = listed(fields)
  const plural = fields.length > 1 || first === 'permissions'
  const change = fields.length > 1 ? 'any of them' : plural ? 'them' : 'it'
  return `${named.charAt(0).toUpperCase()}${named.slice(1)} ${plural ? 'are' : 'is'} ${agent}'s. Change ${change} before starting.`
}

/** `a`, `a and b`, `a, b and c`. */
function listed(words: readonly string[]): string {
  if (words.length <= 1) return words.join('')
  return `${words.slice(0, -1).join(', ')} and ${words.at(-1)}`
}

/** Where a session started as an agent that works in a worktree would run. */
export type WorktreePlan =
  | { kind: 'made'; folder: string; branch: string; from: string }
  | { kind: 'refused'; reason: string }

/**
 * The worktree a session called `name`, started as `agent` in `project`, is
 * made in: `<project>/.floti/worktrees/<name>` on `agent/<name>`, cut from the
 * branch the folder has checked out, as the host makes a child's. The host
 * makes the name unique; the plan says the name it starts from. Outside a git
 * repository there is none, and the start is refused. Null when the agent
 * works in the folder itself.
 */
export function worktreePlan(
  agent: LaunchAgent | null,
  project: Project,
  name: string,
  folderShown: string
): WorktreePlan | null {
  if (agent === null || !agent.worktree) return null
  if (project.git === null) {
    return {
      kind: 'refused',
      reason: `${agent.name} works in a worktree of its own, and ${folderShown} is not in a git repository. Pick a folder in one, or No agent.`
    }
  }
  const separator = project.path.includes('\\') && !project.path.includes('/') ? '\\' : '/'
  const word = worktreeName(name)
  return {
    kind: 'made',
    folder: [project.path.replace(/[\\/]+$/, ''), '.floti', 'worktrees', word].join(separator),
    branch: `agent/${word}`,
    from: project.git.branch ?? 'the commit it has checked out'
  }
}

/**
 * A worktree's one-word name, as the host makes it from the session's
 * (`word_of` in `workspace/worktrees.rs`): every character but a letter, a
 * digit or `_` a hyphen, runs of them one, none at the ends, 40 at most.
 */
function worktreeName(name: string): string {
  const word = name
    .trim()
    .replace(/[^A-Za-z0-9_]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 40)
    .replace(/-+$/, '')
  return word === '' ? 'agent' : word
}

/** One row of the Agent picker. */
export interface AgentOption {
  name: string
  /** The second line: its description, why it cannot run here, or why its file cannot be loaded. */
  detail: string
  disabled: boolean
  /** A file the host could not load, listed apart under a rule. */
  bad: boolean
}

/**
 * The Agent picker's rows after No agent: the host's agents by name, one
 * that no backend here can run disabled with the reason, then the files that
 * cannot be loaded, disabled with their problem. `hostName` names the host in
 * the reason, as "This PC" or a remote host's name.
 */
export function agentOptions(listing: LaunchAgents | null, hostName: string): AgentOption[] {
  if (listing === null) return []
  const installed = new Set(listing.backends.map((backend) => backend.id))
  const agents = listing.agents.map(
    (agent): AgentOption => ({
      name: agent.name,
      detail: agent.backends.length > 0 ? agent.description : cannotRunHere(agent, installed, hostName),
      disabled: agent.backends.length === 0,
      bad: false
    })
  )
  const bad = listing.bad.map(
    (file: BadAgent): AgentOption => ({
      name: file.name,
      detail: `Cannot be loaded: ${file.problem}`,
      disabled: true,
      bad: true
    })
  )
  return [...agents, ...bad]
}

function cannotRunHere(agent: LaunchAgent, installed: ReadonlySet<string>, hostName: string): string {
  if (agent.listedBackends.length > 0 && !agent.listedBackends.some((id) => installed.has(id))) {
    const only = listed(agent.listedBackends).replace(/ and /, ' or ')
    return `Runs only on ${only}, which ${hostName} does not have.`
  }
  return agent.cannotRun ?? `None of ${hostName}'s agent backends can run it.`
}

/** The agent of `listing` called `name`, when it can run here. */
export function runnableAgent(listing: LaunchAgents | null, name: string | null): LaunchAgent | null {
  if (listing === null || name === null) return null
  return listing.agents.find((agent) => agent.name === name && agent.backends.length > 0) ?? null
}

function sameName(a: string, b: string): boolean {
  return a.trim().toLowerCase() === b.trim().toLowerCase()
}

/** The value `map` keys by `backend`, whatever the key's case. */
function keyed<V>(map: Readonly<Record<string, V>>, backend: string): V | undefined {
  const found = Object.keys(map).find((key) => key.toLowerCase() === backend.toLowerCase())
  return found === undefined ? undefined : map[found]
}
