import type { RemoteHost, RemoteHostConnection } from '@floti/core/types'
import { classifyHostFailure, type HostFailureCause } from '@floti/core/types'
import type { TreeHostFailure, TreeHostPhase } from '@floti/ui'

/** What the window has read of a host's sessions (`useHostWorkspaces`). */
export interface HostListing {
  /** Whether it has listed its sessions in this run. Stays true through a drop: it is how a host that was live is told from one that never was. */
  listed: boolean
  /** Why its last listing failed, if it did. */
  listError?: string | undefined
}

/**
 * Where the window stands with a host on another computer, as its group in the
 * sidebar says it, from what its link last said (`connection`, null while
 * nothing has been heard) and what the window has read of its sessions.
 *
 * - Nothing heard, or retrying before it was ever live: connecting. A host
 *   whose floti starts at the next sign-in is connecting too, and says so.
 * - Connected: loading until its sessions are listed, then live; Unknown when
 *   listing them failed.
 * - After it was live, the link retrying, or failing the way a retry fails
 *   (the transport, or floti not found there): reconnecting, with why once a
 *   retry has failed. Main's link reports `failed` from the first failed retry
 *   and through every one after, and it is still trying.
 * - Never live in this run and failing that way, or stopped by what only the
 *   user can fix (a changed key, a floti too old or newer): unreachable, with
 *   the reason every surface gives (`classifyHostFailure`) and the line its
 *   link last wrote.
 */
export function treeHostPhase(
  host: Pick<RemoteHost, 'name' | 'transport'>,
  connection: RemoteHostConnection | null,
  { listed, listError }: HostListing
): TreeHostPhase {
  if (connection === null) return { kind: 'connecting' }
  switch (connection.state) {
    case 'connected':
      if (listed) return { kind: 'live' }
      return listError === undefined ? { kind: 'loading' } : { kind: 'unknown', reason: listError }
    case 'retrying':
      return listed ? { kind: 'reconnecting' } : { kind: 'connecting' }
    case 'startsAtSignIn':
      return { kind: 'connecting', note: 'Its floti starts when somebody next signs in there.' }
    case 'failed':
    case 'missing': {
      const failure = failureOf(host, connection.lastLine, true)
      return listed ? { kind: 'reconnecting', failure } : { kind: 'unreachable', failure }
    }
    case 'keyChanged':
      return { kind: 'unreachable', failure: failureOf(host, '', false, { kind: 'keyChanged' }) }
    case 'tooOld':
      return {
        kind: 'unreachable',
        failure: failureOf(host, '', false, { kind: 'tooOld', host: connection.host, ours: connection.client })
      }
    case 'newer':
      return {
        kind: 'unreachable',
        failure: failureOf(host, '', false, {
          kind: 'newer',
          host: connection.host,
          ours: connection.client,
          by: 'protocol'
        })
      }
  }
}

function failureOf(
  host: Pick<RemoteHost, 'name' | 'transport'>,
  lastLine: string,
  retrying: boolean,
  cause?: HostFailureCause
): TreeHostFailure {
  const failure = classifyHostFailure({ name: host.name, transport: host.transport, lastLine, cause, step: 'connect' })
  return { reason: failure.title, retrying, ...(lastLine === '' ? {} : { lastLine }) }
}
