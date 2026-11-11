import { useCallback, useEffect, useRef, useState } from 'react'
import type { LaunchAgents } from '@helm/core/types'
import { bridgeFor } from '../bridge'
import { readable } from '../errors'

export interface AgentListState {
  /** The host's agents as it lists them now; null until the first answer. */
  listing: LaunchAgents | null
  error: string | null
  reload: () => void
  /** Writes a shipped agent the folder lacks. */
  addShipped: (name: string) => void
  /** Why the last Add on this host did not add it; null when it did, or nothing was added here. */
  addError: string | null
}

/**
 * The agents of `host`, read while `enabled` (the Agents view is on screen,
 * or an agent page is open) and again whenever its folder changes. The host
 * watches the folder only while the window listens, so nothing is watched
 * while no surface shows an agent. The window keeps no copy beyond that.
 */
export function useAgentList(host: string, enabled: boolean): AgentListState {
  const helm = bridgeFor(host)
  /** The last answer, with the host it is of: another host's list is never shown as this one's. */
  const [read, setRead] = useState<{ host: string; listing: LaunchAgents | null; error: string | null }>({
    host,
    listing: null,
    error: null
  })
  /** Why an Add did not, with the host it was on: another host's refusal is never shown as this one's. */
  const [added, setAdded] = useState<{ host: string; error: string | null }>({ host, error: null })
  /** Which read is the newest, so an answer that arrives after a later one is dropped. */
  const asked = useRef(0)

  const reload = useCallback(() => {
    const ask = ++asked.current
    helm
      .invoke('agents:list')
      .then((listing) => {
        if (ask === asked.current) setRead({ host, listing, error: null })
      })
      .catch((err: unknown) => {
        if (ask === asked.current) setRead((last) => ({ host, listing: last.host === host ? last.listing : null, error: readable(err) }))
      })
  }, [helm, host])

  useEffect(() => {
    if (!enabled) return
    const off = helm.on('agents:changed', reload)
    reload()
    return off
  }, [helm, enabled, reload])

  const addShipped = useCallback(
    (name: string) => {
      setAdded({ host, error: null })
      helm
        .invoke('agents:addShipped', { name })
        .then((result) => setAdded({ host, error: result.refusal }))
        .catch((err: unknown) => setAdded({ host, error: readable(err) }))
    },
    [helm, host]
  )

  const mine = read.host === host
  return {
    listing: mine ? read.listing : null,
    error: mine ? read.error : null,
    reload,
    addShipped,
    addError: added.host === host ? added.error : null
  }
}
