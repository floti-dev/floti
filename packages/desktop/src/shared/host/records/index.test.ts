import { describe, expect, it } from 'vitest'
import type * as P from '../protocol'
import { historyPageFromHost, historySessionFromHost } from './history'
import { profileFromHost, sessionFromHost, splitSettingsPatch } from './index'

const hostSession = (patch: Partial<P.SessionRecord> = {}): P.SessionRecord => ({
  id: 9,
  name: 'reviewer',
  label: null,
  cwd: '/work/docs',
  branch: null,
  projectPath: null,
  profileId: null,
  argv: [],
  conversationId: null,
  backend: 'claude-code',
  status: 'running',
  startedAt: '2026-11-05T09:00:00.000Z',
  endedAt: null,
  durationMs: null,
  exitCode: null,
  ...patch
})

describe('what a session’s agent called it, as the window reads it', () => {
  it('carries the title its agent gave a session, and none for one untitled or from a host before 2.2', () => {
    expect(sessionFromHost(hostSession({ title: 'Fix the login form' }))).toMatchObject({ title: 'Fix the login form' })
    expect(sessionFromHost(hostSession())).not.toHaveProperty('title')
  })

  const conversation = (patch: Partial<P.HistorySession> = {}): P.HistorySession => ({
    sessionId: '3d1f0c2a-0000-4000-8000-000000000001',
    backend: 'claude-code',
    project: '/work/docs',
    projectName: 'docs',
    promptCount: 2,
    firstAt: 1,
    lastAt: 2,
    firstPrompt: 'fix the login form',
    title: 'fix the login form',
    titleFallback: false,
    label: null,
    transcriptFile: null,
    transcriptBytes: null,
    projectExists: true,
    archive: null,
    archivedMessages: null,
    match: null,
    ...patch
  })

  it('carries the title a conversation’s agent gave it in History beside the derived one', () => {
    expect(historySessionFromHost(conversation({ agentTitle: 'Login form' }))).toMatchObject({
      title: 'fix the login form',
      agentTitle: 'Login form'
    })
    expect(historySessionFromHost(conversation())).not.toHaveProperty('agentTitle')
  })

  it('carries a conversation’s family, how it ended and its matching lines, and leaves out what the host did not say', () => {
    const report = { state: 'stuck', text: 'Needs the lock', at: '2026-11-07T09:21:00Z' }
    const line = { uuid: 'm1', at: 5, text: 'the rate limit' }
    expect(
      historySessionFromHost(
        conversation({
          parent: 'lead',
          children: 3,
          branch: 'feat/queue',
          profileId: 4,
          endedBy: 'floti',
          exitCode: 1,
          lastReport: report,
          matches: [line],
          matchCount: 6,
          lastReply: 'Closing it fixes it.'
        })
      )
    ).toMatchObject({
      parent: 'lead',
      children: 3,
      branch: 'feat/queue',
      profileId: 4,
      endedBy: 'floti',
      exitCode: 1,
      lastReport: report,
      matches: [line],
      matchCount: 6,
      lastReply: 'Closing it fixes it.'
    })
    const plain = historySessionFromHost(conversation())
    for (const field of ['parent', 'children', 'branch', 'profileId', 'endedBy', 'exitCode', 'lastReport', 'matches', 'matchCount', 'lastReply']) {
      expect(plain).not.toHaveProperty(field)
    }
  })
})

describe('a history page, as the window reads it', () => {
  it('carries the commands a parent’s sessions ran only when the host says them', () => {
    const page = { sessions: [], total: 0, tookMs: 1 }
    expect(historyPageFromHost(page)).not.toHaveProperty('commands')
    expect(
      historyPageFromHost({
        ...page,
        commands: [
          { command: 'pnpm test', startedAt: 5, running: false, endedBy: 'exited', exitCode: 1 },
          { command: 'pnpm dev', startedAt: 6, running: true }
        ]
      }).commands
    ).toEqual([
      { command: 'pnpm test', startedAt: 5, running: false, endedBy: 'exited', exitCode: 1 },
      { command: 'pnpm dev', startedAt: 6, running: true }
    ])
  })
})

describe('a host session, as the window reads it', () => {
  it('carries a child’s parent, the parent’s name and its agent', () => {
    const record = sessionFromHost(
      hostSession({ parentId: 4, parentName: 'lead', agent: 'reviewer' })
    )
    expect(record).toMatchObject({ id: 9, parentId: 4, parentName: 'lead', agent: 'reviewer' })
  })

  it('carries a session nobody started with neither a parent nor an agent', () => {
    for (const bare of [
      hostSession(),
      hostSession({ parentId: null, parentName: null, agent: null })
    ]) {
      const record = sessionFromHost(bare)
      expect(record).not.toHaveProperty('parentId')
      expect(record).not.toHaveProperty('parentName')
      expect(record).not.toHaveProperty('agent')
    }
  })

  it('keeps an agent the parent’s row outlived, with no parent', () => {
    const record = sessionFromHost(hostSession({ parentId: null, parentName: null, agent: 'reviewer' }))
    expect(record).toMatchObject({ agent: 'reviewer' })
    expect(record).not.toHaveProperty('parentId')
  })

  it('carries a command child’s command and its exit code once it has exited', () => {
    const command = 'pnpm test --filter core'
    const running = sessionFromHost(hostSession({ parentId: 4, parentName: 'lead', command }))
    expect(running).toMatchObject({ command, exitCode: null })
    expect(running).not.toHaveProperty('agent')

    const endedAt = '2026-11-05T09:30:00.000Z'
    expect(
      sessionFromHost(hostSession({ parentId: 4, parentName: 'lead', command, status: 'exited', endedAt, exitCode: 3 }))
    ).toMatchObject({ command, status: 'exited', exitCode: 3 })
  })

  it('carries no command for an agent child, a session nobody started, or a host before it', () => {
    for (const bare of [
      hostSession({ parentId: 4, parentName: 'lead', agent: 'reviewer', command: null }),
      hostSession({ parentId: 4, parentName: 'lead', agent: 'reviewer' }),
      hostSession()
    ]) {
      expect(sessionFromHost(bare)).not.toHaveProperty('command')
    }
  })

  it('carries that the host ended a session, and nothing for one that ended by itself', () => {
    const endedAt = '2026-11-05T09:30:00.000Z'
    expect(sessionFromHost(hostSession({ status: 'exited', endedAt, stopped: true }))).toMatchObject({
      status: 'exited',
      stopped: true
    })
    for (const done of [hostSession({ status: 'exited', endedAt }), hostSession({ status: 'exited', endedAt, stopped: false })]) {
      expect(sessionFromHost(done)).not.toHaveProperty('stopped')
    }
  })

  it('carries how a session ended, and nothing from a host that does not say', () => {
    const endedAt = '2026-11-05T09:30:00.000Z'
    for (const endedBy of ['exited', 'parent', 'person', 'floti'] as const) {
      expect(sessionFromHost(hostSession({ status: 'exited', endedAt, endedBy }))).toMatchObject({ endedBy })
    }
    expect(sessionFromHost(hostSession({ status: 'exited', endedAt, stopped: true }))).not.toHaveProperty('endedBy')
  })

  it('carries that a stopped child is kept under its parent, and nothing from a host that does not say', () => {
    const endedAt = '2026-11-05T09:30:00.000Z'
    const kept = hostSession({ parentId: 4, parentName: 'lead', status: 'exited', endedAt, kept: true })
    expect(sessionFromHost(kept)).toMatchObject({ kept: true })
    const left = hostSession({ parentId: 4, parentName: 'lead', status: 'exited', endedAt })
    expect(sessionFromHost(left)).not.toHaveProperty('kept')
    expect(sessionFromHost({ ...left, kept: false })).not.toHaveProperty('kept')
  })

  it('carries a child’s last report while it has one, and nothing otherwise', () => {
    const lastReport = { state: 'done', text: 'Fixed both bugs.', at: '2026-11-06T10:00:00.000Z' }
    expect(sessionFromHost(hostSession({ parentId: 4, parentName: 'lead', lastReport }))).toMatchObject({
      lastReport
    })
    expect(sessionFromHost(hostSession({ parentId: 4, parentName: 'lead' }))).not.toHaveProperty('lastReport')
  })

  it('carries that a child outlives its parent and the mode it was launched in, and nothing otherwise', () => {
    expect(
      sessionFromHost(hostSession({ parentId: 4, parentName: 'lead', outlive: true, permissionMode: 'plan' }))
    ).toMatchObject({ outlive: true, permissionMode: 'plan' })
    const launchedAs = {
      agentFile: 'C:\\Users\\someone\\.config\\floti\\agents\\lead.md',
      model: 'opus',
      effort: 'high' as const,
      allowedTools: ['Read'],
      disallowedTools: ['Edit'],
      tools: ['floti-agents'],
      worktree: true
    }
    expect(sessionFromHost(hostSession({ launchedAs })).launchedAs).toEqual(launchedAs)
    expect(sessionFromHost(hostSession({}))).not.toHaveProperty('launchedAs')
    for (const plain of [hostSession(), hostSession({ outlive: false })]) {
      expect(sessionFromHost(plain)).not.toHaveProperty('outlive')
      expect(sessionFromHost(plain)).not.toHaveProperty('permissionMode')
    }
  })
})

const hostProfile = (patch: Partial<P.Profile> = {}): P.Profile => ({
  id: 3,
  name: 'hub dev',
  root: '/work/hub',
  overlays: [],
  access: [],
  model: null,
  effort: null,
  permissionMode: null,
  agent: null,
  mcp: [],
  openingPrompt: null,
  pinnedOrder: null,
  createdAt: '2026-10-29T09:00:00.000Z',
  updatedAt: '2026-10-29T09:00:00.000Z',
  ...patch
})

describe('a host profile, as the window reads it', () => {
  it('keeps the choice to turn the session agents off', () => {
    expect(profileFromHost(hostProfile({ sessionAgents: false })).sessionAgents).toBe(false)
  })

  it('has no choice where the host has none or follows the setting', () => {
    expect(profileFromHost(hostProfile())).not.toHaveProperty('sessionAgents')
    expect(profileFromHost(hostProfile({ sessionAgents: null }))).not.toHaveProperty('sessionAgents')
  })

  it('keeps the choice to run with Remote Control, and has none where the host has it off', () => {
    expect(profileFromHost(hostProfile({ remoteControl: true })).remoteControl).toBe(true)
    expect(profileFromHost(hostProfile())).not.toHaveProperty('remoteControl')
    expect(profileFromHost(hostProfile({ remoteControl: null }))).not.toHaveProperty('remoteControl')
  })
})

describe('a window settings patch, split between the host and Helm', () => {
  it('keeps what quitting does on this device, never in the host', () => {
    const split = splitSettingsPatch({ quitSessions: { local: 'keep' }, scanRoots: ['/work'] })

    expect(split.own).toEqual({ quitSessions: { local: 'keep' } })
    expect(split.host).toEqual({ scanRoots: ['/work'] })
    expect(split.host).not.toHaveProperty('quitSessions')
  })

  it('keeps the tabs, panes and splits on this device, never in the host', () => {
    const paneLayout = {
      root: {
        axis: 'row' as const,
        children: [
          { panes: [{ kind: 'session' as const, id: 7 }], activeId: 'session:7' },
          { panes: [{ kind: 'settings' as const }], activeId: 'settings' }
        ],
        sizes: [0.3, 0.7]
      },
      focused: 1
    }
    const split = splitSettingsPatch({ paneLayout })

    expect(split.own).toEqual({ paneLayout })
    expect(split.host).toEqual({})
  })

  it('keeps which children are shown, shrunk or opened as tabs, and the setting for it, on this device', () => {
    const paneLayout = {
      root: { panes: [{ kind: 'session' as const, id: 7 }], activeId: 'session:7' },
      focused: 0,
      families: { 'session:7': { order: [9, 8], shrunk: [8], closed: [], screens: [] } }
    }
    const split = splitSettingsPatch({ paneLayout, showChildrenInParentTab: false })

    expect(split.own).toEqual({ paneLayout, showChildrenInParentTab: false })
    expect(split.host).toEqual({})
  })
})
