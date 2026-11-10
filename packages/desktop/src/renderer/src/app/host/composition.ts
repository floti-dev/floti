import type { Project } from '@helm/core/types'
import type { HostConnection } from '../../../../shared/host/client'
import { effectiveRequestForHost } from '../../../../shared/host/records'
import { configScopesWithScan } from '../../../../shared/host/records/content'
import type { HostReach } from './history'
import type { HostAdapter } from './router'

/**
 * What composition reads of an agent's config, over the window's own
 * connection: the scopes and their trees a template imports from
 * (`config:scopes`, `config:tree`) and the effective view the profile editor
 * shows of what a profile composes (`config:effective`). The host reads; none
 * of them writes, and nothing in Floti writes into an agent's config folder.
 *
 * The scope list is the host's, with the sidebar's harnesses and projects
 * beside it. Every request goes through `link.connection()`, which joins the
 * first connect while it is in flight; with no host each rejects with the
 * link's sentence, never an empty scope list or tree in its place.
 */

export interface CompositionAdapterDeps {
  link: HostReach
  /** The harnesses and projects on the sidebar, which the scope list carries beside the host's scopes. */
  scanned: () => Promise<readonly Project[]>
}

export function createCompositionAdapter({ link, scanned }: CompositionAdapterDeps): HostAdapter {
  const host = (): Promise<HostConnection> => link.connection()

  return {
    requests: {
      'config:scopes': async () => {
        const connection = await host()
        const [listed, folders] = await Promise.all([connection.request('config:scopes', null), scanned()])
        return configScopesWithScan(listed, folders)
      },
      'config:tree': async ({ scopePath }) => (await host()).request('config:tree', { scopePath }),
      'config:effective': async (request) => (await host()).request('config:effective', effectiveRequestForHost(request))
    }
  }
}
