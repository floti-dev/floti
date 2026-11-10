import { describe, expect, it } from 'vitest'
import type { LaunchSessionRequest } from '../ipc'
import { installedBackends, launchAgentsFromHost, launchRequestToHost } from './launch'
import type * as P from './protocol'

const request: LaunchSessionRequest = {
  cwd: 'C:\\work\\api',
  profileId: null,
  permissionMode: null,
  resume: null,
  cols: 80,
  rows: 24
}

function descriptor(id: string, name: string, features: P.BackendFeature[]): P.BackendDescriptor {
  return {
    id,
    name,
    features,
    harness: { instructionFiles: [], parts: [] } as unknown as P.HarnessSupport
  }
}

function summary(name: string, patch: Partial<P.AgentSummary> = {}): P.AgentSummary {
  return {
    name,
    description: `${name} does its work.`,
    file: `${name}.md`,
    hash: 'ab12',
    backends: ['claude-code'],
    worktree: false,
    canStart: false,
    unusedLines: [],
    fields: {
      description: `${name} does its work.`,
      profile: null,
      backends: [],
      models: {},
      efforts: {},
      permission: null,
      worktree: false,
      canStart: false,
      allowedTools: [],
      disallowedTools: [],
      command: null,
      submit: null,
      stop: null
    },
    cannotRun: null,
    ...patch
  }
}

describe('launchRequestToHost', () => {
  it('runs a session started as no agent on Claude Code, and leaves out what a launcher before agents never said', () => {
    const sent = launchRequestToHost(request)
    expect(sent.backend).toBe('claude-code')
    expect(sent).not.toHaveProperty('agent')
    expect(sent).not.toHaveProperty('chosen')
    expect(sent.projectPath).toBeNull()
    expect(sent.name).toBeNull()
  })

  it('leaves the backend to the agent unless one was chosen, and carries the agent and what was chosen', () => {
    const chosen = { model: 'opus', effort: null }
    expect(launchRequestToHost({ ...request, agent: 'research-lead', chosen })).toMatchObject({
      backend: null,
      agent: 'research-lead',
      chosen
    })
    expect(launchRequestToHost({ ...request, agent: 'research-lead', backend: 'other-agent' }).backend).toBe(
      'other-agent'
    )
  })
})

describe('launchAgentsFromHost', () => {
  const backends = [
    descriptor('shell', 'Shell', ['run']),
    descriptor('claude-code', 'Claude Code', ['run', 'status', 'resume'])
  ]

  it('offers no built-in default, keeps a default.md and a shipped agent with no file, and takes the fields', () => {
    const listing: P.AgentsListing = {
      folder: 'C:\\Users\\someone\\.config\\floti\\agents',
      readOnly: null,
      agents: [
        summary('default', { file: null, hash: null }),
        summary('lead', {
          file: null,
          hash: null,
          fields: {
            ...summary('lead').fields,
            models: { 'claude-code': 'opus' },
            efforts: { 'claude-code': 'high' },
            permission: 'plan',
            disallowedTools: ['Edit']
          }
        }),
        summary('docs-writer', { backends: [], cannotRun: 'Runs only on codex, which this host does not have.' })
      ],
      bad: [
        {
          name: 'typo',
          file: 'typo.md',
          hash: 'cd34',
          problems: [
            { field: 'worktree', line: 3, message: 'worktree must be yes or no' },
            { field: null, line: null, message: 'no body' }
          ]
        }
      ],
      shipped: []
    }
    const agents = launchAgentsFromHost(listing, backends)
    expect(agents.agents.map((agent) => agent.name)).toEqual(['lead', 'docs-writer'])
    expect(agents.agents[0]).toMatchObject({
      models: { 'claude-code': 'opus' },
      efforts: { 'claude-code': 'high' },
      permission: 'plan',
      disallowedTools: ['Edit'],
      cannotRun: null
    })
    expect(agents.agents[1]?.cannotRun).toBe('Runs only on codex, which this host does not have.')
    expect(agents.bad).toEqual([{ name: 'typo', file: 'typo.md', problem: 'worktree must be yes or no; no body' }])
    expect(agents.backends).toEqual([{ id: 'claude-code', name: 'Claude Code' }])

    const written = launchAgentsFromHost(
      { ...listing, agents: [summary('default'), ...listing.agents.slice(1)] },
      backends
    )
    expect(written.agents.map((agent) => agent.name)).toEqual(['default', 'lead', 'docs-writer'])
  })

  it('counts as agent backends only those that do more than run a command line', () => {
    expect(installedBackends([descriptor('shell', 'Shell', ['run'])])).toEqual([])
  })
})
