import type { HostConnection } from '../src/shared/host/client'
import type { LocalHost } from '../src/main/host/launcher'
import type { HostLink } from '../src/main/host/link'
import type { FakeConnection } from './fake-connection'

export { FakeConnection, FakeStream, fakeReach, type SentRequest } from './fake-connection'

/** A `LocalHost` over `connection`, recording whether it was closed and whether its bridge's processes were ended. */
export function fakeLocalHost(
  connection: FakeConnection,
  startedHost = true
): LocalHost & { closed: boolean; killed: boolean } {
  const local = {
    connection: connection.asConnection(),
    startedHost,
    closed: false,
    killed: false,
    close: () => {
      local.closed = true
      connection.close()
      return Promise.resolve()
    },
    kill: () => {
      local.killed = true
      connection.close()
    }
  }
  return local
}

/** A link already connected to `connection`, or failing with `failure`, for the adapters. */
export function fakeLink(
  connection: FakeConnection | null,
  failure = 'No host.'
): HostLink & {
  connect(): Promise<void>
} {
  const listeners: Array<(connection: HostConnection) => void | Promise<void>> = []
  let connected = false
  let attempt: Promise<HostConnection> | null = null
  const link = {
    connection(): Promise<HostConnection> {
      if (connection === null) return Promise.reject(new Error(failure))
      attempt ??= (async () => {
        // Current before the listeners run, as the real link's is.
        connected = true
        for (const listener of listeners) await listener(connection.asConnection())
        return connection.asConnection()
      })()
      return attempt
    },
    usable(): Promise<HostConnection> {
      return link.connection()
    },
    current: () => (connected && connection !== null ? connection.asConnection() : null),
    settled: () => (attempt === null ? Promise.resolve() : attempt.then(() => undefined)),
    onConnected(listener: (connection: HostConnection) => void | Promise<void>) {
      listeners.push(listener)
    },
    writeSettings: () => undefined,
    quit: () => Promise.resolve(),
    release: () => Promise.resolve(),
    keepConnected: () => undefined,
    reconnectNow: () => undefined,
    drop: () => undefined,
    stopHost: () => Promise.resolve(),
    endedOnPurpose: () => false,
    restartsHost: () => true,
    restartHost: () => Promise.resolve(),
    restarting: () => false,
    async connect() {
      await link.connection()
    }
  }
  return link
}

/** A link whose connection can be replaced, the way a host that restarted is reached again. */
export function reconnectingLink(first: FakeConnection): HostLink & { reconnect(next: FakeConnection): Promise<void> } {
  const listeners: Array<(connection: HostConnection) => void | Promise<void>> = []
  let current = first
  let attempt: Promise<HostConnection> | null = null
  const connect = async (): Promise<HostConnection> => {
    for (const listener of listeners) await listener(current.asConnection())
    return current.asConnection()
  }
  return {
    connection: () => (attempt ??= connect()),
    usable: () => (attempt ??= connect()),
    current: () => current.asConnection(),
    settled: () => Promise.resolve(),
    onConnected: (listener) => {
      listeners.push(listener)
    },
    writeSettings: () => undefined,
    quit: () => Promise.resolve(),
    release: () => Promise.resolve(),
    keepConnected: () => undefined,
    reconnectNow: () => undefined,
    drop: () => undefined,
    stopHost: () => Promise.resolve(),
    endedOnPurpose: () => false,
    restartsHost: () => true,
    restartHost: () => Promise.resolve(),
    restarting: () => false,
    async reconnect(next) {
      current = next
      attempt = connect()
      await attempt
    }
  }
}
