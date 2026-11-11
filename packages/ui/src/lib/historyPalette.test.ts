import { describe, expect, it } from 'vitest'
import type { HistoryCommand, HistorySession, LaunchAgents } from '@helm/core'
import { at, historySession } from './historyPalette.testkit'
import {
  agentGone,
  commandWord,
  endedWord,
  historyWhen,
  launchSentence,
  marks,
  paletteName,
  rowWord,
  snippet,
  unresumable
} from './historyPalette'

const NOW = at(7, 10, 5)


describe('what a History row calls a session', () => {
  it('is the name given by hand, else its agent’s title, else what was typed first in quotes, cut at 60', () => {
    expect(paletteName(historySession({ label: 'Rate limits', agentTitle: 'Limits' }))).toEqual({ text: 'Rate limits', quoted: false })
    expect(paletteName(historySession({ agentTitle: 'Rate limits on the public endpoints' }))).toEqual({
      text: 'Rate limits on the public endpoints',
      quoted: false
    })
    expect(paletteName(historySession())).toEqual({ text: '“add rate limits to the public endpoints”', quoted: true })
    const long = 'a'.repeat(80)
    expect(paletteName(historySession({ title: long })).text).toBe(`“${'a'.repeat(59)}…”`)
    // Floti's own words for a record with no usable prompt are not quoted.
    expect(paletteName(historySession({ title: 'Image only', titleFallback: true }))).toEqual({ text: 'Image only', quoted: false })
  })
})

describe('the one word a row needs', () => {
  it('says running or working for a session running now, before anything else', () => {
    expect(rowWord(historySession({ projectExists: false }), { working: true, here: true }, false)).toEqual({ text: 'working', tone: 'accent' })
    expect(rowWord(historySession(), { working: false, here: false }, false)).toEqual({ text: 'running', tone: 'ok' })
  })

  it('names why one cannot be resumed, then a gone agent, and says nothing for one that can', () => {
    expect(unresumable(historySession({ projectExists: false }))).toBe('folder gone')
    expect(unresumable(historySession({ transcriptFile: null }))).toBe('archived')
    expect(unresumable(historySession({ transcriptFile: null, archive: 'evicted' }))).toBe('dropped')
    expect(unresumable(historySession({ transcriptFile: null, archive: null }))).toBe('prompts only')
    expect(rowWord(historySession(), null, true)).toEqual({ text: 'agent gone', tone: 'warn' })
    expect(rowWord(historySession(), null, false)).toBeNull()
  })

  it('takes an agent for gone only when the host’s listing was read and does not have it', () => {
    const listing = { agents: [{ name: 'lead' }], bad: [] } as unknown as LaunchAgents
    expect(agentGone('writer', listing)).toBe(true)
    expect(agentGone('lead', listing)).toBe(false)
    expect(agentGone('writer', null)).toBe(false)
    expect(agentGone(undefined, listing)).toBe(false)
  })
})

describe('how a child ended', () => {
  it('reads done, ended with its parent, stopped and failed, with the time', () => {
    const ended = (patch: Partial<HistorySession>) => endedWord(historySession({ lastAt: at(7, 9, 21), ...patch }))
    expect(ended({ endedBy: 'exited', exitCode: 0, lastReport: { state: 'done', text: '', at: '' } })).toEqual({ text: 'Done 09:21', danger: false })
    expect(ended({ endedBy: 'floti' })).toEqual({ text: 'Ended with it 09:21', danger: false })
    expect(ended({ endedBy: 'exited', exitCode: 1 })).toEqual({ text: 'Failed 09:21', danger: true })
    expect(ended({ endedBy: 'person' })).toEqual({ text: 'Stopped 09:21', danger: false })
    expect(ended({})).toBeNull()
  })
})

describe('how a command a session ran stands', () => {
  it('reads running, its exit code, in danger when not 0, and who stopped it', () => {
    const word = (patch: Partial<HistoryCommand>) => commandWord({ command: 'pnpm test', startedAt: 0, running: false, ...patch })
    expect(word({ running: true })).toEqual({ text: 'Running', danger: false })
    expect(word({ endedBy: 'exited', exitCode: 0 })).toEqual({ text: 'Exited 0', danger: false })
    expect(word({ endedBy: 'exited', exitCode: 2 })).toEqual({ text: 'Exited 2', danger: true })
    expect(word({ endedBy: 'parent' })).toEqual({ text: 'Stopped by it', danger: false })
    expect(word({ endedBy: 'floti' })).toEqual({ text: 'Ended with it', danger: false })
    expect(word({ endedBy: 'person' })).toEqual({ text: 'Stopped', danger: false })
    expect(word({})).toBeNull()
  })
})

describe('the launch sentence', () => {
  it('names claude --resume, the folder, the profile and the agent, and leaves out what it does not run', () => {
    expect(launchSentence(historySession({ agent: 'researcher' }), { profile: 'Atlas', withoutAgent: false })).toBe(
      'claude --resume in C:\\code\\atlas-api with the Atlas profile as the researcher agent'
    )
    expect(launchSentence(historySession({ agent: 'writer' }), { profile: null, withoutAgent: true, hostName: 'build-box' })).toBe(
      'claude --resume in C:\\code\\atlas-api on build-box'
    )
  })
})

describe('when, in a row’s width', () => {
  it('is minutes, then hours today, then yesterday, a weekday this week, else the date', () => {
    expect(historyWhen(at(7, 10, 5), NOW)).toBe('now')
    expect(historyWhen(at(7, 9, 53), NOW)).toBe('12m')
    expect(historyWhen(at(7, 8, 50), NOW)).toBe('1h')
    expect(historyWhen(at(6, 17, 22), NOW)).toBe('yesterday')
    expect(historyWhen(at(5, 12), NOW)).toBe('Thu')
    expect(historyWhen(at(3, 12), NOW)).toBe('Tue')
    expect(historyWhen(new Date(2026, 9, 28, 12).getTime(), NOW)).toBe('28 Oct')
  })
})

describe('a matched line', () => {
  it('starts a little before the first word that matched, and marks every word', () => {
    const text = 'In server/app.ts the rate limit middleware runs before auth, so a bad token still costs the bucket.'
    expect(snippet(text, 'rate limit', 10)).toBe('…server/app.ts the rate limit middleware runs before auth, so a bad token still costs the bucket.')
    expect(snippet('the rate limit', 'rate')).toBe('the rate limit')
    expect(marks('The Rate limit', 'rate limit')).toEqual([
      { text: 'The ', hit: false },
      { text: 'Rate', hit: true },
      { text: ' ', hit: false },
      { text: 'limit', hit: true }
    ])
  })
})
