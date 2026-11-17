import type { SettingSpec } from '@floti-dev/plugin-sdk'
import type { PluginDelivery, PluginInfo, PluginLogLine, PluginUiRequest } from '../../ipc'
import { LOCAL_HOST_KEY } from '@helm/core/types'
import { onHostOrigin } from '../../plugins/origin'
import type * as P from '../protocol'

/**
 * The host's plugins in the window's vocabulary. Nothing here reaches Node,
 * so main and the window's adapters share it.
 */

/**
 * A plugin as the host lists it, in the shape Helm's window draws. A remote
 * host's page and background URLs are moved onto that host's own origin for
 * the plugin (`shared/plugins/origin.ts`), so its pages never share an origin,
 * or storage, with This PC's copy of the same plugin. This PC's stay as the
 * host wrote them.
 */
export function pluginFromHost(plugin: P.PluginInfo, host: string = LOCAL_HOST_KEY): PluginInfo {
  const id = plugin.id
  const at = (url: string): string => (id === null ? url : onHostOrigin(url, { host, plugin: id }))
  return {
    path: plugin.path,
    id: plugin.id,
    name: plugin.name,
    version: plugin.version,
    description: plugin.description,
    enabled: plugin.enabled,
    error: plugin.error,
    warnings: plugin.warnings,
    revision: plugin.revision,
    icon: plugin.icon,
    rail: plugin.rail,
    panels: mapValues(defined(plugin.panels), (panel) => ({ ...panel, url: at(panel.url) })),
    tabs: mapValues(defined(plugin.tabs), (tab) => ({ ...tab, url: at(tab.url) })),
    pageStrip: plugin.pageStrip,
    background: plugin.background === null ? null : { ...plugin.background, url: at(plugin.background.url) },
    commands: plugin.commands,
    settings: plugin.settings as SettingSpec[],
    settingValues: defined(plugin.settingValues),
    network: plugin.network,
    secrets: plugin.secrets,
    exec: plugin.exec,
    service: plugin.service,
    runsPrograms: plugin.runsPrograms,
    agent:
      plugin.agent === null
        ? null
        : {
            enabled: plugin.agent.enabled,
            server: plugin.agent.server,
            instructions: plugin.agent.instructions,
            tools: plugin.agent.tools.map((tool) => ({
              name: tool.name,
              description: tool.description,
              readOnly: tool.readOnly
            }))
          },
    startsSessions: plugin.startsSessions,
    seesSessions: plugin.seesSessions,
    status: plugin.status,
    badge: plugin.badge
  }
}

/** What the host asks of a page's window, in the shape the window reads. */
export function uiFromHost(request: P.PluginUiRequest): PluginUiRequest {
  return request.kind === 'openTab' ? { ...request, params: defined(request.params) } : request
}

/** What the host hands a plugin's pages, in the shape the relay reads. */
export function deliveryFromHost(delivery: P.PluginDelivery): PluginDelivery {
  return { plugin: delivery.plugin, event: delivery.event, data: delivery.data, to: delivery.to }
}

/** The host's own log lines are what Helm's log calls its own. */
export function logFromHost(lines: readonly P.PluginLogLine[]): PluginLogLine[] {
  return lines.map((line) => ({ at: line.at, stream: line.stream === 'host' ? 'helm' : line.stream, text: line.text }))
}

function mapValues<T, U>(record: Record<string, T>, map: (value: T) => U): Record<string, U> {
  const out: Record<string, U> = {}
  for (const [key, value] of Object.entries(record)) out[key] = map(value)
  return out
}

/** A record whose values the protocol marks optional, with only the ones that are there. */
function defined<T>(record: { [key in string]?: T }): Record<string, T> {
  const out: Record<string, T> = {}
  for (const [key, value] of Object.entries(record)) if (value !== undefined) out[key] = value
  return out
}
