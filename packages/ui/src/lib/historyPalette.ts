import type { HistorySession, LaunchAgents } from '@helm/core'

/**
 * What History's palette says about a session, apart from how it draws it:
 * the name a row shows, the one word a row needs, the sentence that says what
 * Enter runs and the times, each a function a test can call with a session.
 */

/** A session running now, as the window knows it from its host's list. */
export interface HistoryRunning {
  /** Mid-turn, as the sessions pane says "working"; otherwise "running". */
  working: boolean
  /** The window has a tab for it, so Enter shows that tab; otherwise Enter opens one. */
  here: boolean
}

/** The cut H3 puts on a session named by what was typed. */
export const PROMPT_NAME_MAX = 60

/**
 * What a row calls a session: the name given by hand, else the title its
 * agent wrote, else what was typed first in quotes, cut at 60 characters, so
 * it reads as a quote and not as a title. A stand-in for a record with no
 * usable prompt ("Image only") is Floti's own words and has no quotes.
 */
export function paletteName(session: Pick<HistorySession, 'label' | 'agentTitle' | 'title' | 'titleFallback'>): {
  text: string
  quoted: boolean
} {
  const named = session.label ?? session.agentTitle
  if (named !== undefined && named !== null && named.trim() !== '') return { text: named, quoted: false }
  if (session.titleFallback) return { text: session.title, quoted: false }
  const line = session.title.split(/\r?\n/, 1)[0]?.trim() ?? ''
  const cut = line.length > PROMPT_NAME_MAX ? `${line.slice(0, PROMPT_NAME_MAX - 1).trimEnd()}…` : line
  return { text: `“${cut}”`, quoted: true }
}

/** Why a session cannot be resumed, as the one word its row says (H13), or null when it can. */
export type Unresumable = 'folder gone' | 'archived' | 'dropped' | 'prompts only'

export function unresumable(session: HistorySession): Unresumable | null {
  if (!session.projectExists) return 'folder gone'
  if (session.transcriptFile !== null) return null
  if (session.archive === 'archived') return 'archived'
  if (session.archive === 'evicted') return 'dropped'
  return 'prompts only'
}

/** The sentence a preview gives for why a session cannot be resumed. */
export function unresumableSentence(word: Unresumable, folder: string): string {
  switch (word) {
    case 'folder gone':
      return `Its folder, ${folder}, is gone. Put the folder back to resume it.`
    case 'archived':
      return 'Claude Code removed its transcript; Floti kept the conversation, so it can be read but not resumed.'
    case 'dropped':
      return 'Floti archived it, then dropped it to stay under the limit in Settings › Archive. What you typed is kept.'
    case 'prompts only':
      return 'Claude Code removed it before Floti archived it. What you typed is kept.'
  }
}

/**
 * Whether the agent a session was started as is gone from its host, as the
 * host's listing says: named among neither its agents nor the files that are
 * not agents. A listing that could not be read is not known, never gone.
 */
export function agentGone(
  agent: string | undefined,
  agents: Pick<LaunchAgents, 'agents' | 'bad'> | null | undefined
): boolean {
  if (agent === undefined || agent === '' || agents === null || agents === undefined) return false
  return !agents.agents.some((each) => each.name === agent)
}

/** The word at a row's right (H4), and the tone it is drawn in. */
export interface RowWord {
  text: string
  tone: 'ok' | 'accent' | 'warn' | 'subtle'
}

export function rowWord(
  session: HistorySession,
  running: HistoryRunning | null,
  gone: boolean
): RowWord | null {
  if (running !== null) return running.working ? { text: 'working', tone: 'accent' } : { text: 'running', tone: 'ok' }
  const word = unresumable(session)
  if (word !== null) return { text: word, tone: word === 'folder gone' ? 'warn' : 'subtle' }
  if (gone) return { text: 'agent gone', tone: 'warn' }
  return null
}

/** How a child ended (H9): "Done 09:21", "Ended with it 09:41", "Failed 08:55". */
export function endedWord(session: HistorySession): { text: string; danger: boolean } | null {
  const at = clock(session.lastAt)
  if (session.endedBy === 'floti') return { text: `Ended with it ${at}`, danger: false }
  if (session.endedBy === 'parent') return { text: `Stopped by it ${at}`, danger: false }
  if (session.endedBy === 'person') return { text: `Stopped ${at}`, danger: false }
  if (session.endedBy !== 'exited') return null
  if (session.exitCode !== undefined && session.exitCode !== 0) return { text: `Failed ${at}`, danger: true }
  return { text: `${session.lastReport?.state === 'done' ? 'Done' : 'Ended'} ${at}`, danger: false }
}

/**
 * The launch sentence (H5): what Enter runs, before it runs. `profile` is the
 * profile's name, or null for none.
 */
export function launchSentence(
  session: HistorySession,
  options: { profile: string | null; withoutAgent: boolean; hostName?: string | undefined }
): string {
  const agent = session.agent === undefined || session.agent === '' || options.withoutAgent ? null : session.agent
  const how = [
    options.profile === null ? null : `with the ${options.profile} profile`,
    agent === null ? null : `as the ${agent} agent`
  ]
    .filter((part) => part !== null)
    .join(' ')
  const where = options.hostName === undefined ? '' : ` on ${options.hostName}`
  return `claude --resume in ${session.project}${where}${how === '' ? '' : ` ${how}`}`
}

/** A row's "when" (H4): now, 12m, 3h today, then yesterday, a weekday this week, else the date. */
export function historyWhen(at: number, now = Date.now()): string {
  const ms = now - at
  if (ms < 60_000) return 'now'
  if (ms < 3_600_000) return `${String(Math.floor(ms / 60_000))}m`
  const then = new Date(at)
  const today = new Date(now)
  const days = Math.round((startOfDay(today) - startOfDay(then)) / 86_400_000)
  if (days <= 0) return `${String(Math.floor(ms / 3_600_000))}h`
  if (days === 1) return 'yesterday'
  if (days < 7) return then.toLocaleDateString('en-GB', { weekday: 'short' })
  return then.toLocaleDateString('en-GB', { day: 'numeric', month: 'short' })
}

/** The facts line's span: "today 08:20 to 09:12", "Thursday 15:48 to 17:22". */
export function historySpan(first: number, last: number, now = Date.now()): string {
  const days = Math.round((startOfDay(new Date(now)) - startOfDay(new Date(first))) / 86_400_000)
  const day =
    days <= 0
      ? 'today'
      : days === 1
        ? 'yesterday'
        : days < 7
          ? new Date(first).toLocaleDateString('en-GB', { weekday: 'long' })
          : new Date(first).toLocaleDateString('en-GB', { day: 'numeric', month: 'short' })
  return `${day} ${clock(first)} to ${clock(last)}`
}

/** 24-hour "09:41". */
export function clock(at: number): string {
  const date = new Date(at)
  return `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`
}

function startOfDay(date: Date): number {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime()
}

/** The words of a query, lower-cased, as every list matches them. */
export function queryWords(query: string): string[] {
  return query
    .toLowerCase()
    .split(/\s+/)
    .filter((word) => word !== '')
}

/**
 * A matched message cut to the part a 28px row shows: from a little before
 * the first word that matched, with an ellipsis where it was cut, on one line.
 */
export function snippet(text: string, query: string, lead = 36): string {
  const flat = text.replace(/\s+/g, ' ').trim()
  const lower = flat.toLowerCase()
  const at = queryWords(query)
    .map((word) => lower.indexOf(word))
    .filter((index) => index >= 0)
    .reduce((first, index) => Math.min(first, index), Number.POSITIVE_INFINITY)
  if (!Number.isFinite(at) || at <= lead) return flat
  const from = flat.lastIndexOf(' ', at - lead)
  return `…${flat.slice(from < 0 ? at - lead : from + 1)}`
}

/** `text` split where any word of `query` occurs, each part saying whether it matched. */
export function marks(text: string, query: string): { text: string; hit: boolean }[] {
  const words = queryWords(query)
  if (words.length === 0) return [{ text, hit: false }]
  const lower = text.toLowerCase()
  const hit = new Array<boolean>(text.length).fill(false)
  for (const word of words) {
    for (let at = lower.indexOf(word); at >= 0; at = lower.indexOf(word, at + word.length)) {
      for (let i = at; i < at + word.length; i += 1) hit[i] = true
    }
  }
  const parts: { text: string; hit: boolean }[] = []
  for (let i = 0; i < text.length; i += 1) {
    const last = parts[parts.length - 1]
    if (last !== undefined && last.hit === hit[i]) last.text += text[i]
    else parts.push({ text: text[i] ?? '', hit: hit[i] ?? false })
  }
  return parts
}
