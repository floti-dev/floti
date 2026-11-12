import { useCallback, useEffect, useState } from 'react'
import type { UpdateView } from '@helm/ui'
import type * as P from '../../../shared/host/protocol'
import { updateView, type Updates } from './host/updates'

/**
 * What this PC's host knows about newer releases, and the two things a
 * person may ask of it: check now, and update.
 *
 * The host is the one that asks the release; the window only listens to it
 * (`update:state`, followed across reconnects for as long as the window is
 * open) and passes on a press. Nothing here asks on its own: `check` and
 * `install` are handed to buttons and to nothing else. Check now is
 * unthrottled on purpose - a person pressing a button is its own bound.
 *
 * `view` is null until the host has been heard, and `error` says why it
 * could not be, or why the last press was refused; the next state heard
 * clears it.
 */
export interface UpdateHook {
  view: UpdateView | null
  error: string | null
  /** A Check now the user pressed is in flight, or the host is checking. */
  checking: boolean
  check: () => void
  install: () => void
}

export interface UpdateDeps {
  updates: Updates
}

export function useUpdate(deps: UpdateDeps, appVersion: string | null): UpdateHook {
  const { updates } = deps
  const [state, setState] = useState<P.UpdateState | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [asking, setAsking] = useState(false)

  useEffect(
    () =>
      updates.watch((heard) => {
        if (heard.ok) {
          setState(heard.state)
          setError(null)
        } else {
          setError(heard.message)
        }
      }),
    [updates]
  )

  const check = useCallback(() => {
    setAsking(true)
    void updates
      .check()
      .then((result) => {
        if (result.ok) setState(result.state)
        setError(result.ok ? null : result.message)
      })
      .finally(() => setAsking(false))
  }, [updates])

  const install = useCallback(() => {
    void updates.install().then((result) => setError(result.ok ? null : result.message))
  }, [updates])

  const view = state === null ? null : updateView(state, appVersion)
  return { view, error, checking: asking || view?.checking === true, check, install }
}
