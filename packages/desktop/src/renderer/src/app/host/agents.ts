import type { AgentChangeResult, AgentFileText, LaunchAgents } from '@helm/core/types'
import type { HostConnection, HostSubscription } from '../../../../shared/host/client'
import { agentProblemFromHost, launchAgentsFromHost } from '../../../../shared/host/launch'
import type * as P from '../../../../shared/host/protocol'
import type { HostReach } from './history'
import type { HostAdapter } from './router'

/**
 * A host's agents: what a launcher offers and what the Agents page lists,
 * reads and edits. The window asks over its own connection, and the host
 * answers from its agents folder as it reads now. The installed agent
 * backends come from the host's welcome, which named them when the
 * connection was made.
 *
 * The host watches its agents folder only while a client listens on
 * `agents:changed`, so the subscription is held only while the window has a
 * listener, and made again on each connection while it does.
 */
export function createAgentsAdapter(link: HostReach, log: (line: string) => void = () => undefined): HostAdapter {
  const host = (): Promise<HostConnection> => link.connection()
  const listeners = new Set<(change: { names: string[] }) => void>()
  let connection: HostConnection | null = null
  let subscription: HostSubscription | null = null

  const subscribe = (): void => {
    subscription?.unsubscribe()
    subscription = null
    if (connection === null || listeners.size === 0) return
    subscription = connection.subscribe('agents:changed', {
      event: (change) => {
        const names = [...change.names]
        for (const listener of listeners) listener({ names })
      },
      error: (error) => log(`host: agents:changed ended (${error.message})`)
    })
  }

  link.onConnected((made) => {
    connection = made
    subscribe()
  })

  return {
    requests: {
      'agents:list': async (): Promise<LaunchAgents> => {
        const connection = await host()
        const listing = await connection.request('agents:list', null)
        return launchAgentsFromHost(listing, connection.welcome.backends)
      },
      'agents:read': async ({ name }) => agentFileFromHost(await (await host()).request('agents:read', { name })),
      'agents:write': async ({ name, text, expectedHash, renameFrom }) =>
        agentChangeFromHost(
          await (await host()).request('agents:write', { name, text, expectedHash, renameFrom })
        ),
      'agents:delete': async ({ name, expectedHash }) =>
        agentChangeFromHost(await (await host()).request('agents:delete', { name, expectedHash })),
      'agents:restore': async ({ name }) =>
        agentChangeFromHost(await (await host()).request('agents:restore', { name })),
      'agents:addShipped': async ({ name }) =>
        agentChangeFromHost(await (await host()).request('agents:addShipped', { name })),
      'agents:render': async ({ text }) => (await host()).request('agents:render', { text })
    },
    events: {
      'agents:changed': (listener) => {
        listeners.add(listener)
        if (listeners.size === 1) {
          if (connection === null) void host().catch(() => undefined)
          else subscribe()
        }
        return () => {
          listeners.delete(listener)
          if (listeners.size === 0) subscribe()
        }
      }
    }
  }
}

function agentFileFromHost(file: P.AgentFile): AgentFileText {
  return {
    name: file.name,
    file: file.file,
    text: file.text,
    hash: file.hash,
    problems: file.problems.map(agentProblemFromHost)
  }
}

function agentChangeFromHost(change: P.AgentChange): AgentChangeResult {
  return {
    changed: change.changed,
    file: change.file === null ? null : agentFileFromHost(change.file),
    refusal: change.refusal,
    problems: change.problems.map(agentProblemFromHost),
    conflict:
      change.conflict === null
        ? null
        : { onDiskHash: change.conflict.onDiskHash, onDiskText: change.conflict.onDiskText }
  }
}
