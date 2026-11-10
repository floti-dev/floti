import { describe, expect, it } from 'vitest'
import { EMPTY_INVENTORY, type LaunchAgent, type LaunchAgents, type Profile, type Project } from '@helm/core/types'
import { agentOptions, agentValuesNote, resolveLaunch, runnableAgent, worktreePlan } from './launchAgent'

function agent(name: string, patch: Partial<LaunchAgent> = {}): LaunchAgent {
  return {
    name,
    description: `${name} does its work.`,
    file: `${name}.md`,
    backends: ['claude-code'],
    listedBackends: [],
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

function profile(id: number, name: string, patch: Partial<Profile> = {}): Profile {
  return {
    id,
    name,
    root: 'C:\\work',
    overlays: [],
    access: [],
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

const researchLead = agent('research-lead', {
  models: { 'claude-code': 'opus', 'other-agent': 'small' },
  efforts: { 'claude-code': 'xhigh' },
  permission: 'plan',
  profile: 'studio-docs'
})
const docs = profile(1, 'studio-docs', { model: 'haiku' })
const folders = profile(2, 'orbit', { model: 'sonnet', effort: 'low', permissionMode: 'auto' })
const CLAUDE = { id: 'claude-code', name: 'Claude Code' }
const OTHER = { id: 'other-agent', name: 'Other Agent' }

describe('resolveLaunch', () => {
  it('with no agent shows the folder profile’s values and runs on Claude Code where the host has it', () => {
    const launch = resolveLaunch({
      agent: null,
      installed: [OTHER, CLAUDE],
      profiles: [docs, folders],
      folderProfile: folders,
      touched: {}
    })
    expect(launch).toMatchObject({
      backend: 'claude-code',
      profile: folders,
      model: 'sonnet',
      effort: 'low',
      mode: 'auto',
      fromAgent: []
    })
    expect(launch.backends).toEqual([OTHER, CLAUDE])
  })

  it('takes an agent’s model and effort for its backend, its permission and its profile, and says which are its', () => {
    const launch = resolveLaunch({
      agent: researchLead,
      installed: [CLAUDE, OTHER],
      profiles: [docs, folders],
      folderProfile: folders,
      touched: {}
    })
    expect(launch).toMatchObject({
      backend: 'claude-code',
      profile: docs,
      model: 'opus',
      effort: 'xhigh',
      mode: 'plan',
      fromAgent: ['model', 'effort', 'permissions', 'profile']
    })
    // The agent narrows the backends to its own.
    expect(launch.backends).toEqual([CLAUDE])
  })

  it('keeps what the person changed, and a profile picked wins over the agent’s', () => {
    const launch = resolveLaunch({
      agent: researchLead,
      installed: [CLAUDE],
      profiles: [docs, folders],
      folderProfile: folders,
      touched: { profile: { id: folders.id }, effort: { effort: null }, mode: { mode: 'acceptEdits' } }
    })
    expect(launch).toMatchObject({ profile: folders, model: 'opus', effort: null, mode: 'acceptEdits' })
    expect(launch.fromAgent).toEqual(['model'])
  })

  it('reads an agent’s model for the backend picked, and falls back to the profile’s where it gives none', () => {
    const lead = agent('lead', { backends: ['claude-code', 'other-agent'], models: { 'Other-Agent': 'small' } })
    const onOther = resolveLaunch({
      agent: lead,
      installed: [CLAUDE, OTHER],
      profiles: [folders],
      folderProfile: folders,
      touched: { backend: { id: 'other-agent' } }
    })
    expect(onOther).toMatchObject({ backend: 'other-agent', model: 'small', effort: 'low', mode: 'auto' })
    const onClaude = resolveLaunch({
      agent: lead,
      installed: [CLAUDE, OTHER],
      profiles: [folders],
      folderProfile: folders,
      touched: {}
    })
    expect(onClaude).toMatchObject({ backend: 'claude-code', model: 'sonnet' })
  })
})

describe('agentValuesNote', () => {
  it('names whose the values are, and says nothing when the agent set none', () => {
    expect(agentValuesNote('research-lead', ['model', 'effort', 'permissions', 'profile'])).toBe(
      "Model, effort, permissions and profile are research-lead's. Change any of them before starting."
    )
    expect(agentValuesNote('worker', ['permissions'])).toBe("Permissions are worker's. Change them before starting.")
    expect(agentValuesNote('scout', ['model'])).toBe("Model is scout's. Change it before starting.")
    expect(agentValuesNote('plain', [])).toBeNull()
  })
})

describe('worktreePlan', () => {
  const repo: Project = {
    path: 'C:\\work\\orbit-api',
    name: 'orbit-api',
    kind: 'repo',
    harnessPath: null,
    hasClaudeDir: false,
    inventory: EMPTY_INVENTORY,
    git: { branch: 'main', detached: false, dirty: 0, ahead: 0, behind: 0 }
  }
  const worker = agent('worker', { worktree: true })

  it('names the worktree after the session, on its agent branch, cut from the folder’s branch', () => {
    expect(worktreePlan(worker, repo, 'orbit api!', '~\\orbit-api')).toEqual({
      kind: 'made',
      folder: 'C:\\work\\orbit-api\\.floti\\worktrees\\orbit-api',
      branch: 'agent/orbit-api',
      from: 'main'
    })
  })

  it('refuses outside a git repository, and is none for an agent that works in the folder itself', () => {
    expect(worktreePlan(worker, { ...repo, git: null }, 'notes', '~\\notes')).toEqual({
      kind: 'refused',
      reason:
        'worker works in a worktree of its own, and ~\\notes is not in a git repository. Pick a folder in one, or No agent.'
    })
    expect(worktreePlan(researchLead, repo, 'orbit-api', '~\\orbit-api')).toBeNull()
    expect(worktreePlan(null, repo, 'orbit-api', '~\\orbit-api')).toBeNull()
  })
})

describe('agentOptions', () => {
  const listing: LaunchAgents = {
    agents: [
      agent('docs-writer', { backends: [], listedBackends: ['codex'] }),
      researchLead,
      agent('picky', { backends: [], cannotRun: 'Claude Code cannot enforce its rule.' })
    ],
    bad: [{ name: 'deploy-check', file: 'deploy-check.md', problem: 'permission must be plan or auto' }],
    backends: [CLAUDE]
  }

  it('lists each agent with its description, one no backend here runs disabled with why, then the bad files', () => {
    expect(agentOptions(listing, 'This PC')).toEqual([
      {
        name: 'docs-writer',
        detail: 'Runs only on codex, which This PC does not have.',
        disabled: true,
        bad: false
      },
      { name: 'research-lead', detail: 'research-lead does its work.', disabled: false, bad: false },
      { name: 'picky', detail: 'Claude Code cannot enforce its rule.', disabled: true, bad: false },
      {
        name: 'deploy-check',
        detail: 'Cannot be loaded: permission must be plan or auto',
        disabled: true,
        bad: true
      }
    ])
    expect(agentOptions(null, 'This PC')).toEqual([])
  })

  it('finds only an agent that can run here', () => {
    expect(runnableAgent(listing, 'research-lead')).toBe(researchLead)
    expect(runnableAgent(listing, 'docs-writer')).toBeNull()
    expect(runnableAgent(listing, null)).toBeNull()
  })
})
