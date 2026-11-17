import { hostLabel } from '../../shared/plugins/origin'
import type { HostLink } from './link'
import { createHostPlugins, type HostPlugins } from './plugins'

/**
 * Each remote host's plugins, as main keeps them: the page files This PC's
 * host has (`plugins.ts`), one set per host, each on that host's own main
 * link.
 *
 * A host's plugin pages are served on its own origin for each plugin
 * (`floti-plugin://<id>.<label>/`, `shared/plugins/origin.ts`), from that
 * host's files only, cached per plugin version and revision, never older
 * than the newest read the host has shown (`plugins.ts`).
 *
 * The desktop's own plugins are never copied into a remote host: it keeps
 * its own.
 */

export interface RemotePluginsDeps {
  log: (line: string) => void
}

export interface RemotePlugins {
  /** Starts host `id`'s plugins on its main link; what `stop` ends when the link is let go of. */
  attach(id: string, link: HostLink): { stop(): void }
  /** The plugins of the host whose origin label is `label`, or null when no attached host has it. */
  byLabel(label: string): HostPlugins | null
  /** The desktop is quitting: every host's plugins shut down. */
  shutdown(): void
}

export function createRemotePlugins(deps: RemotePluginsDeps): RemotePlugins {
  const hosts = new Map<string, HostPlugins>()

  const end = (id: string, plugins: HostPlugins): void => {
    plugins.shutdown()
    if (hosts.get(id) === plugins) hosts.delete(id)
  }

  return {
    attach(id, link) {
      const previous = hosts.get(id)
      if (previous !== undefined) end(id, previous)
      const plugins = createHostPlugins({ link, host: id, log: deps.log })
      hosts.set(id, plugins)
      return { stop: () => end(id, plugins) }
    },

    byLabel(label) {
      for (const [id, plugins] of hosts) if (hostLabel(id) === label) return plugins
      return null
    },

    shutdown() {
      for (const [id, plugins] of [...hosts]) end(id, plugins)
    }
  }
}
