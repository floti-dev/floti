import type { SurfaceKind } from '@floti-dev/plugin-sdk'
import { LOCAL_HOST_KEY, type SessionRecord } from '@helm/core/types'
import type { HostConnection, HostSubscription } from '../../../../shared/host/client'
import type * as P from '../../../../shared/host/protocol'
import { deliveryFromHost, logFromHost, pluginFromHost, uiFromHost } from '../../../../shared/host/records'
import type { HelmBridge, PluginCallOutcome, PluginCallRequest, PluginInfo, PluginUiRequest } from '../../../../shared/ipc'
import { CLIENT_METHODS, createClientCalls } from '../../../../shared/plugins/client-calls'
import { failure, PluginCallError } from '../../../../shared/plugins/errors'
import { createLocalEvents, type LocalEvents } from './events'
import type { HostReach } from './history'
import type { HostAdapter } from './router'

/**
 * Plugins, over the window's own connection. The host registers the
 * folders, keeps their switches, setting values and secrets, runs their
 * services and backgrounds and answers their pages' calls: `floti.fetch`,
 * `exec`, settings, secrets, sessions, status and badge, and a
 * `BroadcastChannel` post, which the host hands every page of the plugin on
 * every client and its background.
 *
 * The window (`createPluginsAdapter`) draws Settings and the rail, and the
 * plugin's panels and tabs. It draws what the host asks of it on this
 * connection (`plugins:ui`: the secret and session dialogs), answers on the
 * same connection, which is the one the host checks, and takes every
 * delivery but those for the background.
 *
 * What concerns only the page on screen is answered in the page that draws
 * it, with the checks in `client-calls.ts`:
 * opening one of the plugin's tabs, and opening a link in the Browser tab,
 * which the window opens through `browser:open` and the reach rule. A call the
 * host cannot be reached for settles in the page as unavailable, with the
 * link's sentence, and nowhere in Helm's chrome.
 *
 * Every read goes through `link.connection()`, so one asked before the first
 * connect waits for it, and one asked when the host cannot be reached rejects
 * with the link's sentence, never an empty list.
 */

const SURFACES: readonly SurfaceKind[] = ['panel', 'tab']

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error))

type Own = Pick<HelmBridge, 'invoke'>

interface CoreDeps {
  link: HostReach
  /** The host these plugins are on: `LOCAL_HOST_KEY`, or a remote host's id, whose page URLs move onto its own origins. */
  host: string
  /** Where the client calls hand what they open: false when there is nothing to hand it to. */
  ui: (request: PluginUiRequest) => boolean
  /** What the host asked of this page on its connection. */
  asked: (request: PluginUiRequest) => void
  /** Whether a delivery is for this page's frames. */
  delivers: (delivery: P.PluginDelivery) => boolean
  /** Subscribes to secrets as well: only Settings, in the window, shows them. */
  secrets: boolean
  log: (line: string) => void
}

/** The host's list, its subscriptions and a page's calls. */
interface PluginsCore {
  events: LocalEvents
  host(): Promise<HostConnection>
  current(): HostConnection | null
  /** The host's list as the page shows it, after waiting for the first read. */
  list(): Promise<PluginInfo[]>
  /** The host's list, now the one this page has; the list as the page shows it. */
  take(listed: readonly P.PluginInfo[]): PluginInfo[]
  call(request: PluginCallRequest): Promise<PluginCallOutcome>
  cancel(callId: string): void
}

function createPluginsCore(deps: CoreDeps): PluginsCore {
  const { link, log } = deps
  const host = (): Promise<HostConnection> => link.connection()
  const events = createLocalEvents(() => {
    void host().catch(() => undefined)
  })
  /** The host's last list, mapped; null until the first one. */
  let hosted: PluginInfo[] | null = null
  /** The list as last told, so a list that moves nothing tells nobody. */
  let said = ''
  let current: HostConnection | null = null
  let subscriptions: HostSubscription[] = []

  const take = (listed: readonly P.PluginInfo[]): PluginInfo[] => {
    const list = listed.map((plugin) => pluginFromHost(plugin, deps.host))
    hosted = list
    const text = JSON.stringify(list)
    if (text !== said) {
      said = text
      events.tell('plugins:changed', list)
    }
    return list
  }

  const live = (plugin: string): PluginInfo | null =>
    hosted?.find((info) => info.id === plugin && info.enabled && info.error === null) ?? null

  const clientCalls = createClientCalls({ ui: deps.ui })

  /** A call answered here: one that concerns only the page on screen. */
  const answerOnClient = async (request: PluginCallRequest): Promise<PluginCallOutcome> => {
    if (hosted === null) take(await (await host()).request('plugins:list', null))
    const plugin = live(request.plugin)
    if (plugin === null) return { ok: false, code: 'unavailable', message: 'the plugin is not enabled' }
    clientCalls.run(
      request.method,
      { plugin: request.plugin, declaresTab: (tab) => Object.hasOwn(plugin.tabs, tab), surface: request.surface },
      Array.isArray(request.args) ? request.args : []
    )
    return { ok: true, value: undefined }
  }

  link.onConnected((connection) => {
    current = connection
    void connection.ended.then(() => {
      if (current === connection) current = null
    })
    for (const subscription of subscriptions) subscription.unsubscribe()
    const failed = (channel: string) => (error: Error) => log(`host: ${channel} ended (${error.message})`)
    subscriptions = [
      connection.subscribe('plugins:changed', {
        snapshot: (listed) => take(listed),
        event: (listed) => take(listed),
        error: failed('plugins:changed')
      }),
      connection.subscribe('plugins:ui', {
        event: (request) => deps.asked(uiFromHost(request)),
        error: failed('plugins:ui')
      }),
      connection.subscribe('plugins:deliver', {
        event: (delivery) => {
          if (deps.delivers(delivery)) events.tell('plugins:deliver', deliveryFromHost(delivery))
        },
        error: failed('plugins:deliver')
      }),
      ...(deps.secrets
        ? [
            connection.subscribe('secrets:changed', {
              snapshot: (state) => events.tell('secrets:changed', state),
              event: (state) => events.tell('secrets:changed', state),
              error: failed('secrets:changed')
            })
          ]
        : [])
    ]
  })

  return {
    events,
    host,
    current: () => current,
    async list() {
      return take(await (await host()).request('plugins:list', null))
    },
    take,
    async call(request) {
      try {
        if (!SURFACES.includes(request.surface)) return { ok: false, code: 'invalid', message: 'unknown surface' }
        if (CLIENT_METHODS.has(request.method)) return await answerOnClient(request)
        const connection = await host()
        return await connection.request('plugins:call', {
          plugin: request.plugin,
          surface: request.surface,
          method: request.method,
          args: Array.isArray(request.args) ? request.args : [],
          // The host keys a call by the connection it came on, so the relay's own id is enough.
          callId: request.callId
        })
      } catch (error) {
        // A check here refused it, or the host could not be reached or refused
        // the request itself: the page is told why, as a call that failed, and
        // the desktop's chrome hears nothing.
        if (error instanceof PluginCallError) return failure(error)
        return { ok: false, code: 'unavailable', message: messageOf(error) }
      }
    },
    cancel(callId) {
      // A call in flight went out on the connection there is now; with none,
      // nothing is in flight to cancel.
      current?.request('plugins:cancel', { callId }).catch((error: unknown) => {
        log(`host: plugins:cancel was not taken (${messageOf(error)})`)
      })
    }
  }
}

export interface PluginsAdapterDeps {
  link: HostReach
  /** The host this window's connection reaches: This PC's (`LOCAL_HOST_KEY`) when absent. */
  host?: string
  /** The preload's bridge: the folder picker. */
  own: Own
  /** A session the host started for a plugin, kept with the window's sessions so it opens as a tab. */
  adopt: (row: P.SessionRecord) => SessionRecord
  log?: (line: string) => void
}

/** The window's plugins: Settings and the rail, and the plugin pages it frames. */
export function createPluginsAdapter(deps: PluginsAdapterDeps): HostAdapter {
  const { own } = deps
  const log = deps.log ?? (() => undefined)

  const core: PluginsCore = createPluginsCore({
    link: deps.link,
    host: deps.host ?? LOCAL_HOST_KEY,
    ui: (request) => {
      if (core.events.count('plugins:ui') === 0) return false
      core.events.tell('plugins:ui', request)
      return true
    },
    asked: (request) => core.events.tell('plugins:ui', request),
    delivers: (delivery) => delivery.to !== 'background',
    secrets: true,
    log
  })

  const reach = (): Promise<HostConnection> => core.host()
  const listed = async (answer: Promise<readonly P.PluginInfo[]>): Promise<PluginInfo[]> => core.take(await answer)

  return {
    requests: {
      'plugins:list': async () => listed((await reach()).request('plugins:list', null)),
      'plugins:add': async (request) => {
        // A typed path is the host's own; with none, the picker browses this PC
        // (and is refused for a host on another computer).
        const typed = typeof request?.path === 'string' && request.path !== '' ? request.path : null
        const path = typed ?? (await own.invoke('path:chooseDirectory', { title: 'Add a plugin folder' })).path
        if (path === null) {
          return { path: null, error: null, plugins: await listed((await reach()).request('plugins:list', null)) }
        }
        const added = await (await reach()).request('plugins:add', { path })
        return { path: added.path, error: added.error, plugins: await listed(Promise.resolve(added.plugins)) }
      },
      'plugins:remove': async ({ path, deleteSecrets }) =>
        listed((await reach()).request('plugins:remove', { path, deleteSecrets: [...deleteSecrets] })),
      'plugins:setEnabled': async ({ path, enabled }) =>
        listed((await reach()).request('plugins:setEnabled', { path, enabled })),
      'plugins:setTools': async ({ path, enabled }) => listed((await reach()).request('plugins:setTools', { path, enabled })),
      'plugins:reload': async ({ path }) => listed((await reach()).request('plugins:reload', { path })),
      'plugins:setSetting': async ({ plugin, key, value }) =>
        listed((await reach()).request('plugins:setSetting', { plugin, key, value })),
      'plugins:log': async ({ path }) => logFromHost(await (await reach()).request('plugins:log', { path })),
      'plugins:metrics': async () => (await reach()).request('plugins:metrics', null),
      'plugins:ownSecrets': async ({ path }) => (await reach()).request('plugins:ownSecrets', { path }),
      'plugins:command': async ({ plugin, id }) => {
        await (await reach()).request('plugins:command', { plugin, id })
      },
      'plugins:call': (request) => core.call(request),
      'plugins:session': async (answer) => {
        const row = await (await reach()).request('plugins:session', answer)
        return row === null ? null : deps.adopt(row)
      },
      'secrets:list': async () => (await reach()).request('secrets:list', null),
      'secrets:save': async (input) => (await reach()).request('secrets:save', input),
      'secrets:remove': async ({ key }) => (await reach()).request('secrets:remove', { key })
    },
    sends: {
      'plugins:cancel': ({ callId }) => core.cancel(callId),
      'plugins:answer': ({ requestId }) => {
        reach()
          .then((connection) => connection.request('plugins:answer', { requestId }))
          .catch((error: unknown) => log(`host: plugins:answer was not taken (${messageOf(error)})`))
      }
    },
    events: {
      'plugins:changed': core.events.listen('plugins:changed'),
      'secrets:changed': core.events.listen('secrets:changed'),
      'plugins:deliver': core.events.listen('plugins:deliver'),
      'plugins:ui': core.events.listen('plugins:ui')
    }
  }
}
