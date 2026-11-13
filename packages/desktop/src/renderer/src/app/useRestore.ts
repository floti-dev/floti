import { useCallback, useEffect, useRef, useState } from 'react'
import { sessionLabel, type RestoreOffer } from '@helm/core/types'
import type { RestoreSessionsRequest, RestoreSessionsResult } from '../../../shared/ipc'
import { helm } from './bridge'
import { readable } from './errors'

/**
 * The window's half of restoring after a crash: the offer main holds, read
 * when the window mounts, again each time the window gets its host back
 * after losing it (`reconnects`), since the new host offers what the old one
 * lost, and again each time a session is told lost while the host stays
 * (its session keeper went), which the host then offers back; and the one
 * call that answers it.
 *
 * Where the reopened sessions go is the caller's - it owns the panes. This
 * owns what is said about it afterwards.
 */
export interface RestoreState {
  /** What a crash took, until it is answered. Null when there was nothing, or the read failed. */
  offer: RestoreOffer | null
  /**
   * The sentence the last read of the offer failed with: what was running is
   * not known, which is never said as nothing to restore. Null when the last
   * read answered, with an offer or with none, and before the first answers.
   */
  unread: string | null
  /** True while a read asked for with `retry` is under way. */
  retrying: boolean
  /** Reads the offer again after a read failed. */
  retry: () => void
  /** Forgets that the read failed, until the next read fails again. */
  dismissUnread: () => void
  busy: boolean
  /**
   * Reopens these, each at its pane's grid, and answers the offer. Resolves to
   * what main did, or null when it refused outright.
   */
  restore: (
    picks: RestoreSessionsRequest['sessions'],
    options?: RestoreOptions
  ) => Promise<RestoreSessionsResult | null>
  /** "Not now": answers the offer with nothing. */
  dismiss: () => void
  /** One line about how it went, for the toast. */
  report: { text: string; failed: boolean } | null
  dismissReport: () => void
}

export interface RestoreOptions {
  /** Nobody was asked (`restoreWithoutAsking`), so the report says it happened. */
  unasked?: boolean
  /** The host the sessions went with, as the window names it; absent when they went with Helm. */
  lostWith?: string
}

const plural = (count: number, one: string, many: string): string =>
  `${String(count)} ${count === 1 ? one : many}`

/**
 * What a restore did, in one line, worst news first: what did not come back
 * and why, else what came back short of something; then those already
 * running, which are shown rather than started again; then - only when
 * nobody was asked and nothing else was said - that anything happened at all.
 */
export function restoreReport(
  result: RestoreSessionsResult,
  unasked: boolean,
  lostWith?: string
): { text: string; failed: boolean } | null {
  const parts: string[] = []
  if (result.failed.length > 0) {
    const reasons = result.failed.map(({ name, reason }) => `“${name}” could not be reopened: ${reason}`)
    const back =
      result.restored.length === 0
        ? ''
        : `Reopened ${plural(result.restored.length, 'session', 'sessions')}. `
    parts.push(back + reasons.join(' '))
  } else {
    const warnings = result.restored.flatMap((restored) => restored.launched.warnings)
    if (warnings.length > 0) parts.push(warnings.join(' '))
  }
  for (const { session } of result.running ?? []) {
    parts.push(`“${sessionLabel(session)}” was already running, so it is shown instead of being reopened.`)
  }
  if (parts.length === 0 && unasked && result.restored.length > 0) {
    const reopened = `Reopened ${plural(result.restored.length, 'session', 'sessions')}`
    parts.push(
      lostWith === undefined
        ? `${reopened} Floti was running when it closed.`
        : `${reopened} ${lostWith} was running when it stopped.`
    )
  }
  return parts.length === 0 ? null : { text: parts.join(' '), failed: result.failed.length > 0 }
}

/**
 * Each lost row a restore answered for, to the row now showing it: the
 * session that reopened it, or the one already running its conversation,
 * which takes its place as a reopened one would.
 */
export function restoredPairs(result: RestoreSessionsResult): Map<number, number> {
  return new Map([
    ...result.restored.map(({ from, launched }): [number, number] => [from, launched.session.id]),
    ...(result.running ?? []).map(({ from, session }): [number, number] => [from, session.id])
  ])
}

export function useRestore(reconnects = 0): RestoreState {
  const [offer, setOffer] = useState<RestoreOffer | null>(null)
  const [unread, setUnread] = useState<string | null>(null)
  /** How many times `retry` asked: each asks the host again. */
  const [retries, setRetries] = useState(0)
  const [retrying, setRetrying] = useState(false)
  const [busy, setBusy] = useState(false)
  const [report, setReport] = useState<{ text: string; failed: boolean } | null>(null)
  /** Sessions lost while the window had its host (their keeper went): each makes the host offer them back. */
  const [losses, setLosses] = useState(0)

  useEffect(
    () =>
      helm.on('session:exit', (record) => {
        if (record.status === 'lost') setLosses((count) => count + 1)
      }),
    []
  )

  useEffect(() => {
    let live = true
    void helm
      .invoke('session:restorable')
      .then((answer) => {
        if (!live) return
        setOffer(answer)
        setUnread(null)
      })
      // Could not look is not nothing there: after a crash an offer that
      // silently vanished would be the worst case. An offer already shown
      // stays; what the failed read would have said is not known.
      .catch((err: unknown) => {
        if (live) setUnread(readable(err))
      })
      .finally(() => {
        if (live) setRetrying(false)
      })
    return () => {
      live = false
    }
  }, [reconnects, losses, retries])

  const retry = useCallback(() => {
    setRetrying(true)
    setRetries((count) => count + 1)
  }, [])

  const restore = useCallback(
    async (
      picks: RestoreSessionsRequest['sessions'],
      options: RestoreOptions = {}
    ): Promise<RestoreSessionsResult | null> => {
      setBusy(true)
      setReport(null)
      try {
        const result = await helm.invoke('session:restore', { sessions: picks })
        setReport(restoreReport(result, options.unasked === true, options.lostWith))
        return result
      } catch (err: unknown) {
        setReport({ text: readable(err), failed: true })
        return null
      } finally {
        // Answered either way: main forgets an offer the moment it is asked.
        setOffer(null)
        setBusy(false)
      }
    },
    []
  )

  const dismiss = useCallback(() => {
    setOffer(null)
    void helm.invoke('session:restore', { sessions: [] }).catch(() => undefined)
  }, [])

  return {
    offer,
    unread,
    retrying,
    retry,
    dismissUnread: useCallback(() => setUnread(null), []),
    busy,
    restore,
    dismiss,
    report,
    dismissReport: useCallback(() => setReport(null), [])
  }
}

/**
 * Opens the restore page (`open`) once for each failed read of the offer,
 * and not before the saved layout is in (`ready`): a tab opened over none
 * would be saved over the tabs the person had. A failure that comes first
 * opens it once the layout is in. A Try again that fails again leaves the
 * page where it is, or closed if it was set aside.
 */
export function useOpenOnFailedRead(unread: string | null, ready: boolean, open: () => void): void {
  const hadUnread = useRef(false)
  useEffect(() => {
    if (!ready) return
    const failed = unread !== null
    if (failed && !hadUnread.current) open()
    hadUnread.current = failed
  }, [unread, ready, open])
}
