import type { PluginFolder } from '@helm/core'
import { LOCAL_HOST_KEY } from '@helm/core/types'
import type { SettingValue } from '@floti-dev/plugin-sdk'
import type { PluginInfo } from '../../shared/ipc'
import { HostRequestError, type HostConnection, type HostSubscription } from './client'
import type { HostLink } from './link'
import type * as P from './protocol'
import { pluginFromHost } from './records'

/**
 * What the desktop's main process keeps of a host's plugins. The host
 * registers the folders, reads them, keeps their switches, setting values and
 * secrets, runs their services and backgrounds and answers their pages'
 * calls; the window asks it over its own connection
 * (`renderer/src/app/host/plugins.ts`).
 *
 * Main keeps only what only it can do, on its own subscription to the host's
 * `plugins:changed`: the `floti-plugin://<id>/` scheme, a plugin's page files
 * from the host (`file`), cached here per plugin version and the host's
 * revision of its read, and served only for a plugin the host has on
 * (`serves`). A page's document is always read from the host, so its answer
 * says when the host has read the plugin again before main's list has heard
 * it.
 *
 * The first time the desktop ever connects to a host, it copies its own
 * plugin folders into it, with each one's on/off and tools switches and its
 * setting values, skipping a folder the host has. Secrets are not copied:
 * the user types them again in Settings. That it ran is recorded, so a plugin
 * removed in the host later is not copied back, and the desktop's own plugin
 * records are never written by it: with the setting off again, they are as
 * they were.
 */

/** One of a plugin's page files, as the scheme answers it. */
export interface PluginPageFile {
  status: number
  contentType: string
  body: Uint8Array
}

export interface HostPluginsDeps {
  link: HostLink
  /**
   * The host these are: `LOCAL_HOST_KEY` (the default) for This PC's, or a
   * remote host's id, whose page URLs are moved onto that host's own origin
   * for each plugin.
   */
  host?: string
  /** Helm's own plugins, copied into This PC's host on its first connect; never into a remote host. */
  copyOwn?: OwnPluginsCopy
  log: (line: string) => void
}

/** What copying Helm's own plugins into This PC's host reads and records. */
export interface OwnPluginsCopy {
  /** Helm's own plugin folders, in the order they were added: what the first connect copies. */
  folders: () => PluginFolder[]
  /** Helm's own setting values for a plugin, by its id. */
  settings: (plugin: string) => Record<string, SettingValue>
  /** Whether Helm's plugins have been copied into a host before. */
  copied: () => boolean
  /** Records that they have, so no later start copies them again. */
  markCopied: () => void
}

export interface HostPlugins {
  /** Whether a plugin is on in the host, so its pages are served. Rejects when the host cannot be reached. */
  serves(plugin: string): Promise<boolean>
  /** One of a plugin's page files, from this run's cache when the host's read of the plugin has not moved. */
  file(plugin: string, path: string): Promise<PluginPageFile>
  shutdown(): void
}

/** One plugin's cached page files, for the version and revision they were read at. */
interface Cached {
  version: string | null
  revision: number
  files: Map<string, PluginPageFile>
}

/**
 * A page's document, which a frame asks for first whenever it loads. It is
 * always read from the host, never the cache: its answer names the host's
 * current read of the plugin, which main's own list may not have heard yet.
 */
const isDocument = (path: string): boolean => /\.html?$/i.test(path)

export function createHostPlugins(deps: HostPluginsDeps): HostPlugins {
  const { link } = deps
  const hostKey = deps.host ?? LOCAL_HOST_KEY
  /** The host's last list, as it sent it; null until the first one. */
  let hosted: PluginInfo[] | null = null
  const cache = new Map<string, Cached>()
  /**
   * The newest read of each plugin this connection's host has shown, in its
   * list or in a file's answer. The window hears the host's list on its own
   * connection, which can be ahead of main's, and loads a reloaded plugin's
   * pages from the new read before main's list says it exists; files of an
   * older read than this are never served from the cache or kept in it.
   */
  const newest = new Map<string, number>()
  let subscriptions: HostSubscription[] = []

  const host = (): Promise<HostConnection> => link.connection()

  const live = (plugin: string): PluginInfo | null =>
    hosted?.find((info) => info.id === plugin && info.enabled && info.error === null) ?? null

  /** The host has shown a read of `plugin` numbered `revision`: the cached files of any older read go. */
  const saw = (plugin: string, revision: number): void => {
    if (revision <= (newest.get(plugin) ?? 0)) return
    newest.set(plugin, revision)
    const cached = cache.get(plugin)
    if (cached !== undefined && cached.revision < revision) cache.delete(plugin)
  }

  /** The host's list, now the one Helm has: a plugin's cached files go once the host has read it again. */
  const take = (listed: readonly P.PluginInfo[]): void => {
    hosted = listed.map((plugin) => pluginFromHost(plugin, hostKey))
    for (const plugin of listed) if (plugin.id !== null) saw(plugin.id, plugin.revision)
    for (const [id, cached] of cache) {
      const now = live(id)
      if (now === null || now.revision !== cached.revision || now.version !== cached.version) cache.delete(id)
    }
  }

  /** Helm's own plugins into a host that has never been given them. */
  const copyOwnPlugins = async (connection: HostConnection, own: OwnPluginsCopy): Promise<void> => {
    if (own.copied()) return
    const registered = new Set((await connection.request('plugins:list', null)).map((plugin) => plugin.path))
    for (const folder of own.folders()) {
      if (registered.has(folder.path)) continue
      const added = await connection.request('plugins:add', { path: folder.path })
      if (added.error !== null) {
        deps.log(`host: the plugin folder ${folder.path} was not copied into the local host (${added.error})`)
        continue
      }
      registered.add(folder.path)
      const id = added.plugins.find((plugin) => plugin.path === folder.path)?.id ?? null
      // Values first, while it is on: they are the host's to check against
      // the manifest, and one it refuses is said and left out.
      if (id !== null) {
        for (const [key, value] of Object.entries(own.settings(id))) {
          await refusedIsLogged(
            () => connection.request('plugins:setSetting', { plugin: id, key, value }),
            `host: the setting “${key}” of ${folder.path} was not copied into the local host`
          )
        }
      }
      if (!folder.toolsEnabled) await connection.request('plugins:setTools', { path: folder.path, enabled: false })
      if (!folder.enabled) await connection.request('plugins:setEnabled', { path: folder.path, enabled: false })
    }
    own.markCopied()
  }

  /** A request the host refused is logged and the copy goes on; one that never reached it ends the copy. */
  const refusedIsLogged = async (request: () => Promise<unknown>, what: string): Promise<void> => {
    try {
      await request()
    } catch (error) {
      if (error instanceof HostRequestError && error.code !== 'unavailable') {
        deps.log(`${what} (${error.message})`)
        return
      }
      throw error
    }
  }

  link.onConnected(async (connection) => {
    for (const subscription of subscriptions) subscription.unsubscribe()
    // A host reached again may be one that started over and numbers its reads
    // afresh: nothing read from the last one is served or compared again.
    cache.clear()
    newest.clear()
    subscriptions = [
      connection.subscribe('plugins:changed', {
        snapshot: (listed) => take(listed),
        event: (listed) => take(listed),
        error: (error) => deps.log(`host: plugins:changed ended (${error.message})`)
      })
    ]
    // Only This PC's host is given Helm's own plugins: a remote host keeps its own.
    if (deps.copyOwn === undefined) return
    try {
      await copyOwnPlugins(connection, deps.copyOwn)
    } catch (error) {
      // Not recorded as done, so the next start tries again; a folder copied
      // this time is skipped then.
      deps.log(`host: Floti's plugins were not copied into the local host (${(error as Error).message})`)
    }
  })

  /** Reads one file a piece at a time until it is whole. */
  const read = async (connection: HostConnection, plugin: string, path: string): Promise<{ file: PluginPageFile; answer: P.PluginFile }> => {
    const first = await connection.request('plugins:file', { plugin, path, offset: 0 })
    // A refusal is its whole text whatever the offset; only a 200 comes in pieces.
    if (first.status !== 200) return { file: { status: first.status, contentType: first.contentType, body: first.body }, answer: first }
    const pieces: Uint8Array[] = [first.body]
    let received = first.body.length
    while (received < first.totalBytes) {
      const next = await connection.request('plugins:file', { plugin, path, offset: received })
      if (next.status !== 200) return { file: { status: next.status, contentType: next.contentType, body: next.body }, answer: next }
      // A file that shrank under the read ends where it ends.
      if (next.body.length === 0) break
      pieces.push(next.body)
      received += next.body.length
    }
    const body = new Uint8Array(received)
    let at = 0
    for (const piece of pieces) {
      body.set(piece, at)
      at += piece.length
    }
    return { file: { status: 200, contentType: first.contentType, body }, answer: first }
  }

  return {
    async serves(plugin) {
      if (hosted === null) take(await (await host()).request('plugins:list', null))
      return live(plugin) !== null
    },

    async file(plugin, path) {
      const connection = await host()
      const known = live(plugin)
      const cached = cache.get(plugin)
      const hit =
        !isDocument(path) &&
        known !== null &&
        cached !== undefined &&
        cached.revision === known.revision &&
        cached.version === known.version
          ? cached.files.get(path)
          : undefined
      if (hit !== undefined) return hit
      const { file, answer } = await read(connection, plugin, path)
      saw(plugin, answer.revision)
      // Kept only for the read main's list has, when the host has shown none
      // newer: a file from a read the list has not caught up with is served,
      // and asked for again next time. A document is never kept.
      if (
        file.status === 200 &&
        !isDocument(path) &&
        known !== null &&
        answer.revision === known.revision &&
        answer.version === known.version &&
        answer.revision === newest.get(plugin)
      ) {
        let entry = cache.get(plugin)
        if (entry === undefined || entry.revision !== answer.revision || entry.version !== answer.version) {
          entry = { version: answer.version, revision: answer.revision, files: new Map() }
          cache.set(plugin, entry)
        }
        entry.files.set(path, file)
      }
      return file
    },

    shutdown() {
      for (const subscription of subscriptions) subscription.unsubscribe()
      subscriptions = []
      cache.clear()
    }
  }
}
