import { useCallback, useEffect, useState } from 'react'
import type { UpdateView } from '@helm/ui'
import type { HostConnection } from '../../../shared/host/client'
import type * as P from '../../../shared/host/protocol'
import { updateView, type Updates } from './host/updates'

/**
 * What this PC's host knows about newer releases, and the two things a
 * person may ask of it: check now, and update.
 *
 * The host is the one that asks the release; the window only listens to it
 * (`update:state`, held for as long as the window is open) and passes on a
 * press. Nothing here asks on its own: `check` and `install` are handed to
 * buttons and to nothing else. Check now is unthrottled on purpose - a
 * person pressing a button is its own bound.
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
  /** The window's own connection to this PC's host. */
  connection: () => Promise<HostConnection>
}

export function useUpdate(deps: UpdateDeps, appVersion: string | null): UpdateHook {
  const { updates, connection } = deps
  const [state, setState] = useState<P.UpdateState | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [asking, setAsking] = useState(false)

  useEffect(
    () =>
      updates.watch(connection, (heard) => {
        if (heard.ok) {
          setState(heard.state)
          setError(null)
        } else {
          setError(heard.message)
        }
      }),
    [updates, connection]
  )

  const check = useCallback(() => {
    setAsking(true)
    void updates
      .check(connection)
      .then((result) => {
        if (result.ok) setState(result.state)
        setError(result.ok ? null : result.message)
      })
      .finally(() => setAsking(false))
  }, [updates, connection])

  const install = useCallback(() => {
    void updates.install(connection).then((result) => setError(result.ok ? null : result.message))
  }, [updates, connection])

  const view = state === null ? null : updateView(state, appVersion)
  return { view, error, checking: asking || view?.checking === true, check, install }
}
