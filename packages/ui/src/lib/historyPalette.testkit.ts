import type { HistorySession } from '@helm/core'

/** A moment in November 2026, local time, as History's tests read the clock. */
export const at = (day: number, hour: number, minute = 0): number => new Date(2026, 10, day, hour, minute).getTime()

/** A session History lists, resumable, with what a test changes. */
export function historySession(patch: Partial<HistorySession> = {}): HistorySession {
  return {
    sessionId: 'a1',
    project: 'C:\\code\\atlas-api',
    projectName: 'atlas-api',
    promptCount: 51,
    firstAt: at(6, 15, 48),
    lastAt: at(6, 17, 22),
    firstPrompt: 'add rate limits to the public endpoints',
    title: 'add rate limits to the public endpoints',
    titleFallback: false,
    label: null,
    transcriptFile: 'C:\\Users\\you\\.claude\\projects\\a1.jsonl',
    transcriptBytes: 1000,
    projectExists: true,
    archive: 'archived',
    archivedMessages: 214,
    ...patch
  }
}
