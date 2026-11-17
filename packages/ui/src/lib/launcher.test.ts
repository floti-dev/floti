import { describe, expect, it } from 'vitest'
import type { Profile, Project } from '@floti/core/types'
import { folderName, homePath, launchSentence, profileFor, rankProjects, type SentencePart } from './launcher'

const WORKSPACE = 'C:\\Users\\someone\\.harness\\dev'
const repo = (name: string): string => `${WORKSPACE}\\repos\\${name}`

function project(path: string, name = folderName(path)): Project {
  return {
    path,
    name,
    kind: 'repo',
    workspacePath: WORKSPACE,
    hasClaudeDir: false,
    inventory: { skills: [], agents: [], commands: [], hooks: [], mcpServers: [], claudeMd: false } as never,
    git: null
  }
}

let nextId = 1
function profile(patch: Partial<Profile> & Pick<Profile, 'name'>): Profile {
  return {
    id: nextId++,
    root: WORKSPACE,
    overlays: [],
    access: [],
    model: null,
    effort: null,
    permissionMode: null,
    agent: null,
    mcp: [],
    openingPrompt: null,
    pinnedOrder: null,
    createdAt: '2026-10-01T00:00:00.000Z',
    updatedAt: '2026-10-01T00:00:00.000Z',
    ...patch
  }
}

/** The sentence as text, with mono parts in backticks. */
const read = (parts: readonly SentencePart[]): string =>
  parts.map((part) => (part.mono ? `\`${part.text}\`` : part.text)).join('')

describe('rankProjects', () => {
  const timepiece = project(repo('timepiece'))
  const builder = project(repo('TimePiece-Builder'))
  const reporting = project(repo('timepiece-reporting'))
  const floti = project(repo('floti'))
  const overtime = project(repo('overtime'))
  const all = [floti, reporting, builder, timepiece, overtime]

  it('lists everything by when it was last worked in when nothing is typed, then by name', () => {
    const recency = new Map([
      [reporting.path.toLowerCase(), 300],
      [floti.path.toLowerCase(), 500]
    ])
    expect(rankProjects(all, '  ', recency).map((m) => m.project.name)).toEqual([
      'floti',
      'timepiece-reporting',
      'overtime',
      'timepiece',
      'TimePiece-Builder'
    ])
  })

  it('puts a name that starts with the query before one that contains it, and marks where', () => {
    const recency = new Map([[overtime.path.toLowerCase(), 900]])
    const ranked = rankProjects(all, 'Time', recency)
    expect(ranked.map((m) => [m.project.name, m.hit])).toEqual([
      ['timepiece', [0, 4]],
      ['TimePiece-Builder', [0, 4]],
      ['timepiece-reporting', [0, 4]],
      // Recency orders within a tier, never across one.
      ['overtime', [4, 8]]
    ])
  })

  it('falls back to the path, with either separator, and drops what matches nothing', () => {
    const ranked = rankProjects(all, 'repos/flo', new Map())
    expect(ranked.map((m) => [m.project.name, m.hit])).toEqual([['floti', null]])
    expect(rankProjects(all, 'nothing like it', new Map())).toEqual([])
  })
})

describe('profileFor', () => {
  const dev = profile({ name: 'dev', overlays: [WORKSPACE] })
  const timepieceProfile = profile({ name: 'Timepiece', root: repo('timepiece'), overlays: [WORKSPACE, repo('timepiece')] })
  const builderProfile = profile({ name: 'Timepiece Builder', overlays: [WORKSPACE, repo('timepiece'), repo('TimePiece-Builder')] })
  const cloud = profile({
    name: 'tp-cloud',
    overlays: [WORKSPACE, repo('timepiece'), repo('timepiece-ui'), repo('timepiece-reporting')]
  })
  const cash = profile({ name: 'Ledger', overlays: [WORKSPACE, `${WORKSPACE}\\tools\\ledger`] })
  const profiles = [cloud, builderProfile, cash, dev, timepieceProfile]

  it('picks the profile composing the fewest folders among those about the folder', () => {
    expect(profileFor(WORKSPACE, profiles)?.name).toBe('dev')
    expect(profileFor(repo('timepiece'), profiles)?.name).toBe('Timepiece')
    expect(profileFor(repo('TimePiece-Builder'), profiles)?.name).toBe('Timepiece Builder')
    expect(profileFor(repo('timepiece-ui'), profiles)?.name).toBe('tp-cloud')
    // An overlay is enough: this profile runs at the workspace root.
    expect(profileFor(`${WORKSPACE}\\TOOLS\\ledger`, profiles)?.name).toBe('Ledger')
  })

  it('starts a folder no profile is about with none', () => {
    expect(profileFor(repo('floti'), profiles)).toBeNull()
    expect(profileFor(repo('floti'), [])).toBeNull()
  })

  it('breaks a tie with the profile rooted there, then the pinned order, then the name', () => {
    const at = repo('sketchbook')
    const rooted = profile({ name: 'zeta', root: at, overlays: [WORKSPACE] })
    const overlaid = profile({ name: 'alpha', overlays: [at] })
    expect(profileFor(at, [overlaid, rooted])?.name).toBe('zeta')

    const second = profile({ name: 'a second', overlays: [WORKSPACE, at], pinnedOrder: 2 })
    const first = profile({ name: 'b first', overlays: [WORKSPACE, at], pinnedOrder: 1 })
    const unpinned = profile({ name: 'a unpinned', overlays: [WORKSPACE, at] })
    expect(profileFor(at, [unpinned, second, first])?.name).toBe('b first')
    expect(profileFor(at, [unpinned, profile({ name: 'B', overlays: [WORKSPACE, at] })])?.name).toBe('a unpinned')
  })
})

describe('homePath', () => {
  it('writes a path under the home directory from ~, and leaves anything else alone', () => {
    expect(homePath(repo('timepiece'), 'C:\\Users\\someone')).toBe('~\\.harness\\dev\\repos\\timepiece')
    expect(homePath('c:\\users\\SOMEONE\\x', 'C:\\Users\\someone\\')).toBe('~\\x')
    expect(homePath('C:\\Users\\someone', 'C:\\Users\\someone')).toBe('~')
    expect(homePath('C:\\Users\\someone-else\\x', 'C:\\Users\\someone')).toBe('C:\\Users\\someone-else\\x')
    expect(homePath('D:\\work', null)).toBe('D:\\work')
  })
})

describe('launchSentence', () => {
  const home = 'C:\\Users\\someone'

  it('names the program and the folder, and nothing else for a plain launch', () => {
    expect(read(launchSentence({ cwd: repo('floti'), home, profile: null, permissionMode: null, resume: null }))).toBe(
      'Runs `claude` in `~\\.harness\\dev\\repos\\floti`.'
    )
    expect(read(launchSentence({ cwd: repo('floti'), home, profile: null, permissionMode: 'plan', resume: null }))).toBe(
      'Runs `claude` in `~\\.harness\\dev\\repos\\floti` with `--permission-mode plan`.'
    )
  })

  it('names the profile, what it composes and grants, every flag, and the opening prompt', () => {
    const p = profile({
      name: 'Timepiece',
      overlays: [WORKSPACE, repo('timepiece'), repo('timepiece-ui')],
      access: [WORKSPACE, repo('timepiece'), repo('beacon')],
      model: 'opus',
      effort: 'xhigh',
      agent: 'reviewer',
      openingPrompt: ' /recap '
    })
    expect(read(launchSentence({ cwd: repo('timepiece'), home, profile: p, permissionMode: 'auto', resume: null }))).toBe(
      'Runs `claude` in `~\\.harness\\dev\\repos\\timepiece` with the Timepiece profile, composing `dev`, `timepiece` and `timepiece-ui`, with access to `beacon`, and `--model opus --effort xhigh --permission-mode auto --agent reviewer`. It opens by saying `/recap`.'
    )
  })

  it('says a conversation is being reopened, and drops the opening prompt a resume does not send', () => {
    const p = profile({ name: 'dev', overlays: [WORKSPACE], openingPrompt: '/recap' })
    expect(
      read(launchSentence({ cwd: repo('timepiece'), home, profile: p, permissionMode: null, resume: 'payroll export fix' }))
    ).toBe('Reopens “payroll export fix” with `claude --resume` in `~\\.harness\\dev\\repos\\timepiece` with the dev profile, composing `dev`.')
  })
})

describe('launchSentence for a session started as an agent', () => {
  const home = 'C:\\Users\\someone'
  const lead = {
    name: 'research-lead',
    allowedTools: ['Read', 'Grep', 'Bash(git log:*)'],
    disallowedTools: ['Edit', 'Write'],
    canStart: true
  }

  it('says what the agent adds: every flag, where its instructions go, and that it can start agents', () => {
    const p = profile({ name: 'studio-docs', overlays: [repo('lumen-web')], access: [repo('lumen-web')], model: 'haiku' })
    expect(
      read(
        launchSentence({
          cwd: repo('orbit-api'),
          home,
          profile: p,
          permissionMode: 'plan',
          resume: null,
          model: 'opus',
          effort: 'xhigh',
          agent: lead
        })
      )
    ).toBe(
      "Runs `claude` in `~\\.harness\\dev\\repos\\orbit-api` as the research-lead agent with the studio-docs profile, composing `lumen-web`, and `--model opus --effort xhigh --permission-mode plan --allowedTools Read,Grep,Bash(git log:*) --disallowedTools Edit,Write`. Its instructions go in after the project's; nothing is typed into the terminal. It can start agents of its own."
    )
  })

  it('names a new worktree with its branch, and an agent that cannot start agents gets none', () => {
    expect(
      read(
        launchSentence({
          cwd: repo('orbit-api'),
          home,
          profile: null,
          permissionMode: 'acceptEdits',
          resume: null,
          model: null,
          effort: null,
          agent: { name: 'worker', allowedTools: [], disallowedTools: ['mcp__floti-plugin-trackr__create'], canStart: false },
          worktree: {
            folder: `${repo('orbit-api')}\\.floti\\worktrees\\orbit-api`,
            branch: 'agent/orbit-api',
            from: 'main'
          }
        })
      )
    ).toBe(
      "Runs `claude` in a new worktree, `~\\.harness\\dev\\repos\\orbit-api\\.floti\\worktrees\\orbit-api` on branch `agent/orbit-api` cut from `main`, as the worker agent, and `--permission-mode acceptEdits --disallowedTools mcp__floti-plugin-trackr__create`. Its instructions go in after the project's; nothing is typed into the terminal. It gets no floti-agents tools."
    )
  })

  it('in the popover names the agent’s tool rules without spelling them, and a model chosen replaces the profile’s', () => {
    const p = profile({ name: 'studio-docs', model: 'haiku' })
    expect(
      read(
        launchSentence({
          cwd: repo('orbit-api'),
          home,
          profile: p,
          permissionMode: 'plan',
          resume: null,
          model: 'opus',
          effort: 'xhigh',
          agent: lead,
          rules: 'named'
        })
      )
    ).toBe(
      'Runs `claude` in `~\\.harness\\dev\\repos\\orbit-api` as the research-lead agent with the studio-docs profile, and `--model opus --effort xhigh --permission-mode plan` and its tool rules. It can start agents of its own.'
    )
  })

  it('reopens a conversation as its agent in its own folder, with no worktree', () => {
    expect(
      read(
        launchSentence({
          cwd: repo('orbit-api'),
          home,
          profile: null,
          permissionMode: null,
          resume: 'plan the export',
          agent: { ...lead, allowedTools: [], disallowedTools: [] },
          worktree: { folder: 'x', branch: 'agent/x', from: 'main' }
        })
      )
    ).toBe(
      "Reopens “plan the export” with `claude --resume` in `~\\.harness\\dev\\repos\\orbit-api` as the research-lead agent. Its instructions go in after the project's; nothing is typed into the terminal. It can start agents of its own."
    )
  })
})
