import type { RemoteHost, RemoteHostConnection } from '@helm/core/types'
import { classifyHostFailure, type HostFailureCause } from '@helm/core/types'
import type { TreeHostPhase } from '@helm/ui'

/**
 * Where the window stands with a host on another computer, as its group in the
 * sidebar says it, from what its link last said (`connection`, null while
 * nothing has been heard) and whether the window has listed its sessions
 * (`listed`, which stays true through a drop: it is how a host that was live
 * before is told from one that never was).
 *
 * - Nothing heard, or retrying before it was ever live: connecting. A host
 *   whose floti starts at the next sign-in is connecting too, and says so.
 * - Connected: loading until its sessions are listed, then live.
 * - Retrying after it was live: reconnecting.
 * - A link that could not reach it (failed, floti missing, a changed key, a
 *   floti too old or newer): unreachable, with the reason every surface gives
 *   (`classifyHostFailure`) and the line its link last wrote.
 */
export function treeHostPhase(
  host: Pick<RemoteHost, 'name' | 'transport'>,
  connection: RemoteHostConnection | null,
  listed: boolean
): TreeHostPhase {
  if (connection === null) return { kind: 'connecting' }
  switch (connection.state) {
    case 'connected':
      return listed ? { kind: 'live' } : { kind: 'loading' }
    case 'retrying':
      return listed ? { kind: 'reconnecting' } : { kind: 'connecting' }
    case 'startsAtSignIn':
      return { kind: 'connecting', note: 'Its floti starts when somebody next signs in there.' }
    case 'failed':
    case 'missing':
      return unreachable(host, connection.lastLine)
    case 'keyChanged':
      return unreachable(host, '', { kind: 'keyChanged' })
    case 'tooOld':
      return unreachable(host, '', { kind: 'tooOld', host: connection.host, ours: connection.client })
    case 'newer':
      return unreachable(host, '', { kind: 'newer', host: connection.host, ours: connection.client, by: 'protocol' })
  }
}

function unreachable(
  host: Pick<RemoteHost, 'name' | 'transport'>,
  lastLine: string,
  cause?: HostFailureCause
): TreeHostPhase {
  const failure = classifyHostFailure({ name: host.name, transport: host.transport, lastLine, cause, step: 'connect' })
  return { kind: 'unreachable', reason: failure.title, ...(lastLine === '' ? {} : { lastLine }) }
}
