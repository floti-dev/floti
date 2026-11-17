import { useCallback, useEffect, useState } from 'react'
import type { LiveSession, SessionResources, SessionsOverview } from '@floti/core'
import { desktop } from './bridge'
import { readable } from './errors'

/**
 * Every live Claude Code session on the machine, and what the desktop's own are
 * holding.
 *
 * Two facts with two different costs, so two channels and two lifetimes.
 *
 * The **overview** is machine-wide and effectively free - main is already
 * reading Claude Code's registry every 750ms for the tab dots, and this is the
 * same `readdir`. It is subscribed for the whole life of the window, because
 * the launch-time warning needs it on the project pane and not only on the
 * sessions pane.
 *
 * The **resources** - process tree and ports - cost a 400ms child process per
 * pass, so they are asked for only while something is looking. `watch()` is the
 * whole of that: a pane calls it while it is on screen and main runs no timer
 * and spawns nothing while nobody has. Reference-counted in main, so two
 * watchers cannot switch each other off.
 */

export interface LiveSessionsState {
  /** Every live session on the machine, hosted first. */
  sessions: LiveSession[]
  /**
   * When main last read the registry, epoch ms. Null before the first answer:
   * `sessions` is then empty because nothing has been read, not because
   * nothing runs.
   */
  readAtMs: number | null
  /** Why the registry could not be read, while no answer has come since; null otherwise. */
  error: string | null
  /** Reads the registry again: an Unknown's Try again. */
  retry: () => void
  /** What each hosted session is holding, by the desktop's session row id. */
  resources: Map<number, SessionResources>
  /**
   * Ask main to run the resource pass while this is true.
   *
   * Balanced: every `watch(true)` needs its `watch(false)`, which is what a
   * `useEffect` cleanup is for.
   */
  watch: (on: boolean) => void
}

export function useLiveSessions(): LiveSessionsState {
  const [overview, setOverview] = useState<SessionsOverview | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [resources, setResources] = useState<Map<number, SessionResources>>(new Map())

  /**
   * One read of the registry. A failure is kept as why, never as an empty
   * list: the last answer, if there was one, stays, and a push clears it.
   */
  const readOverview = useCallback(() => {
    void desktop.invoke('sessions:overview').then(
      (next) => {
        setOverview(next)
        setError(null)
      },
      (err: unknown) => setError(readable(err))
    )
  }, [])

  useEffect(() => {
    const offs = [
      desktop.on('sessions:overview', (next) => {
        setOverview(next)
        setError(null)
      }),
      // The whole set every time, for the reason `session:activity` sends the
      // whole set: a window that missed one push - mounted late, or reloaded -
      // is corrected by the next one rather than carrying a stale tree.
      desktop.on('sessions:resources', (next) => {
        setResources(new Map(next.map((snapshot) => [snapshot.id, snapshot])))
      })
    ]

    // Adopted on mount rather than waited for. An idle machine publishes
    // nothing until something moves, so a pane that waited for the next push
    // would be blank for as long as nobody did anything.
    readOverview()
    // A read that failed (the local host could not be reached) keeps the map
    // empty, so every hosted row paints Unknown rather than "nothing there".
    void desktop
      .invoke('sessions:resources')
      .then((next) => setResources(new Map(next.map((snapshot) => [snapshot.id, snapshot]))))
      .catch(() => undefined)

    return () => {
      for (const off of offs) off()
    }
  }, [readOverview])

  const retry = useCallback(() => readOverview(), [readOverview])

  const watch = useCallback((on: boolean) => {
    desktop.send('sessions:watch', { watching: on })
  }, [])

  return {
    sessions: overview?.sessions ?? [],
    readAtMs: overview?.readAtMs ?? null,
    error,
    retry,
    resources,
    watch
  }
}
