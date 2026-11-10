import type { LaunchAgents } from '@helm/core/types'
import type { HostConnection } from '../../../../shared/host/client'
import { launchAgentsFromHost } from '../../../../shared/host/launch'
import type { HostReach } from './history'
import type { HostAdapter } from './router'

/**
 * A host's agents, as a launcher offers them: the window asks over its own
 * connection, and the host answers from its agents folder as it reads now.
 * The installed agent backends come from the host's welcome, which named
 * them when the connection was made.
 */
export function createAgentsAdapter(link: HostReach): HostAdapter {
  const host = (): Promise<HostConnection> => link.connection()

  return {
    requests: {
      'agents:list': async (): Promise<LaunchAgents> => {
        const connection = await host()
        const listing = await connection.request('agents:list', null)
        return launchAgentsFromHost(listing, connection.welcome.backends)
      }
    }
  }
}
