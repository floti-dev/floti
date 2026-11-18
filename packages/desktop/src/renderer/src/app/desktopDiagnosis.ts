import type { RemoteHostConnection } from '@floti/core/types'
import type { LocalHostStep, OwnLogTail } from '../../../shared/ipc'
import type { DesktopCall, DesktopToolResult } from './host/desktopTools'

/**
 * `desktop_status` and `desktop_log`, as the window answers its part of them.
 *
 * The host answers its own part with no window (its versions, agent CLIs,
 * folders, plugins, tool families, limits and its own log) and asks the
 * window only for what the window alone can tell: this desktop's version,
 * the hosts it knows with how each is connected and its last error, and
 * main's log, `main.log`, which main reads back on `ownLog:tail`. The host
 * merges the two, so each answer here is a sentence for the agent and the
 * same facts as JSON beside it.
 */

/** A host the window knows, as the tools describe it. */
export interface DiagnosedHost {
  id: string
  name: string
  /** How the window's link to it stands, as one word. */
  state: string
  /** Its floti version, while connected and known. */
  version?: string
  /** What went wrong last, when something did. */
  lastError?: string
}

/** What the window reads for the diagnosis tools, given by the app. */
export interface DesktopDiagnosisWindow {
  /** This desktop's version, or null before main has said it. */
  version(): string | null
  /** This PC's host, then each host on another computer this device added. */
  hosts(): readonly DiagnosedHost[]
  /** Main's log: its path and latest lines (`ownLog:tail`). */
  mainLog(lines: number, word?: string): Promise<OwnLogTail>
}

/** How many lines of main's log are read when the host does not say. */
const MAIN_LINES = 100

const answered = (sentence: string, structured: unknown): DesktopToolResult => ({
  content: [
    { type: 'text', text: sentence },
    { type: 'text', text: JSON.stringify(structured) }
  ]
})

const readable = (error: unknown): string => (error instanceof Error ? error.message : String(error))

/** This PC's host, as the window's link to it stands: `step` is where main's link is, `lost` set while the host is gone. */
export function localHost(
  id: string,
  name: string,
  step: LocalHostStep | null,
  lost: { restarts: boolean } | null,
  version: string | null
): DiagnosedHost {
  if (lost !== null) {
    return {
      id,
      name,
      state: 'lost',
      lastError: lost.restarts
        ? 'The host went away; Floti is starting a new one.'
        : 'The host went away; Floti is waiting for it to come back.'
    }
  }
  if (step === null) return { id, name, state: 'unknown' }
  switch (step.step) {
    case 'ready':
      return { id, name, state: 'connected', ...(version === null ? {} : { version }) }
    case 'refused':
      return { id, name, state: 'refused', lastError: step.sentence }
    case 'unreachable':
      return { id, name, state: 'unreachable', lastError: step.reason }
    default:
      return { id, name, state: step.step }
  }
}

/** A host on another computer, as main's link to it stands; null until main has said. */
export function remoteHost(id: string, name: string, connection: RemoteHostConnection | null): DiagnosedHost {
  if (connection === null) return { id, name, state: 'unknown' }
  switch (connection.state) {
    case 'connected':
      return { id, name, state: 'connected', ...(connection.version === undefined ? {} : { version: connection.version }) }
    case 'failed':
    case 'missing':
      return { id, name, state: connection.state, lastError: connection.lastLine }
    case 'keyChanged':
      return {
        id,
        name,
        state: 'keyChanged',
        lastError: 'The host proved another key than the one this device pinned, so nothing more was sent to it.'
      }
    case 'tooOld':
      return {
        id,
        name,
        state: 'tooOld',
        lastError: `Its floti speaks protocol ${connection.host}, older than this desktop's ${connection.client}.`
      }
    case 'newer':
      return {
        id,
        name,
        state: 'newer',
        lastError: `Its floti speaks protocol ${connection.host}, newer than this desktop's ${connection.client}.`
      }
    default:
      return { id, name, state: connection.state }
  }
}

const hostLine = (host: DiagnosedHost): string =>
  `${host.name} (${host.id}): ${host.state}${host.version === undefined ? '' : `, floti ${host.version}`}${host.lastError === undefined ? '' : `: ${host.lastError}`}`

/** Answers the window's part of `desktop_status`. */
export async function statusForAgent(_call: DesktopCall, window: DesktopDiagnosisWindow): Promise<DesktopToolResult> {
  const version = window.version()
  const hosts = window.hosts()
  let mainLog: string | null = null
  let mainLogProblem: string | null = null
  try {
    mainLog = (await window.mainLog(0)).path
  } catch (error) {
    mainLogProblem = readable(error)
  }
  const sentence = [
    `Floti desktop ${version ?? '(version not known yet)'}; main's log is ${mainLog ?? `not known: ${mainLogProblem ?? ''}`}.`,
    `Hosts this window knows: ${String(hosts.length)}.`,
    ...hosts.map(hostLine)
  ].join('\n')
  return answered(sentence, {
    desktop: { version },
    mainLog,
    ...(mainLogProblem === null ? {} : { mainLogProblem }),
    hosts
  })
}

/** Answers the window's part of `desktop_log`: main's latest lines, as the host asked for them. */
export async function logForAgent(call: DesktopCall, window: DesktopDiagnosisWindow): Promise<DesktopToolResult> {
  const asked = (typeof call.arguments === 'object' && call.arguments !== null ? call.arguments : {}) as {
    lines?: unknown
    word?: unknown
  }
  const lines = typeof asked.lines === 'number' && Number.isInteger(asked.lines) ? asked.lines : MAIN_LINES
  const word = typeof asked.word === 'string' && asked.word.trim() !== '' ? asked.word.trim() : undefined
  const tail = await window.mainLog(lines, word)
  const holding = word === undefined ? '' : ` holding "${word}"`
  const sentence =
    tail.lines === null
      ? `main.log (${tail.path}) is not there yet.`
      : tail.lines.length === 0
        ? `main.log (${tail.path}) has no lines${holding}.`
        : `main.log (${tail.path}), its last ${String(tail.lines.length)} lines${holding}:\n${tail.lines.join('\n')}`
  return answered(sentence, { path: tail.path, lines: tail.lines ?? [] })
}
