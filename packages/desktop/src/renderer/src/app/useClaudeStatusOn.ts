import { useCallback, useEffect, useRef, useState } from 'react'
import type { ClaudeStatus, DesktopBridge } from '../../../shared/ipc'
import { bridgeFor } from './bridge'
import { readable } from './errors'

/**
 * What a host on another computer says of its Claude CLI, for Settings ›
 * Programs while that host is picked: asked through its own bridge, never
 * this PC's. This PC's is `useSetup`'s, which the version banner shares.
 *
 * A read that failed leaves no status: "could not look" is Unknown, never
 * "not found". Another host's answer is never shown as this one's.
 */
export interface ClaudeStatusOn {
  status: ClaudeStatus | null
  /** Why the last read failed; null once one succeeded. */
  statusError: string | null
  /** A read the user asked for is in flight. */
  checking: boolean
  recheck: () => void
}

/**
 * @param host the host to ask, or null to ask nothing (This PC, a host out of
 *   reach, or Settings not on screen).
 * @param bridges each host's bridge; `bridgeFor` outside a test.
 */
export function useClaudeStatusOn(
  host: string | null,
  bridges: (host: string) => Pick<DesktopBridge, 'invoke'> = bridgeFor
): ClaudeStatusOn {
  const [read, setRead] = useState<{
    host: string | null
    status: ClaudeStatus | null
    statusError: string | null
    checking: boolean
  }>({ host, status: null, statusError: null, checking: false })
  /** Which read is the newest, so an answer that lands after a later one is dropped. */
  const asked = useRef(0)
  const bridgesRef = useRef(bridges)
  useEffect(() => {
    bridgesRef.current = bridges
  }, [bridges])

  /** Asks `on` for its status; the answer lands only if no later read was asked since. */
  const request = useCallback((on: string) => {
    asked.current += 1
    const mine = asked.current
    void bridgesRef.current(on)
      .invoke('setup:status')
      .then(
        (status) => {
          if (mine === asked.current) setRead({ host: on, status, statusError: null, checking: false })
        },
        (error: unknown) => {
          if (mine !== asked.current) return
          setRead((current) =>
            current.host === on
              ? { ...current, statusError: readable(error), checking: false }
              : { host: on, status: null, statusError: readable(error), checking: false }
          )
        }
      )
  }, [])

  useEffect(() => {
    if (host !== null) request(host)
  }, [host, request])

  const recheck = useCallback(() => {
    if (host === null) return
    setRead((current) =>
      current.host === host ? { ...current, checking: true } : { host, status: null, statusError: null, checking: true }
    )
    request(host)
  }, [host, request])

  const mine = read.host === host
  return {
    status: mine ? read.status : null,
    statusError: mine ? read.statusError : null,
    checking: mine && read.checking,
    recheck
  }
}
