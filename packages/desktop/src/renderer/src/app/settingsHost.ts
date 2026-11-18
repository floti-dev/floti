import { HOST_WORKSPACE_KEYS, LOCAL_HOST_KEY, type AppSettings } from '@floti/core/types'
import type { SecretPluginChoice, SettingsHostView } from '@floti/ui'
import type { PluginInfo } from '../../../shared/ipc'
import { clockTime } from './useAwaySince'

/**
 * Settings' Floti host group (the design's S1-S8): the settings of the host
 * its switch picks, and where a change goes.
 *
 * - The values are that host's as it keeps them: This PC's are the window's
 *   own settings with its host's laid over; a host on another computer's are
 *   what its own bridge answered (`useHostWorkspaces`). A host the window
 *   cannot reach reads null, and Settings shows Unknown, never defaults.
 * - A change of a key a host keeps (`HOST_WORKSPACE_KEYS`) goes to the picked
 *   host alone. Every other key is this device's and goes to This PC
 *   whichever host is picked: device settings never follow the pick.
 * - A host on another computer that refuses a change keeps its value, so the
 *   control shows what it holds, and the refusal goes to the console. That is
 *   where This PC's own refusal goes, as an unhandled rejection of its write.
 */
export interface SettingsHostDeps {
  /** The picked host's id: `LOCAL_HOST_KEY` for This PC. */
  host: string
  /** Its name as the window shows it. */
  name: string
  /** Whether the window can reach it now. */
  reached: boolean
  /** This PC's settings with its host's laid over; null until read. */
  local: AppSettings | null
  /** A host on another computer's settings, read through its bridge; null while unread or not connected. */
  remote: (host: string) => AppSettings | null
  /** Writes through This PC's bridge, whose adapter sends its host's keys to its host. */
  writeLocal: (patch: Partial<AppSettings>) => void
  /** Writes through that host's own bridge; resolves with why it refused, or null. */
  writeRemote: (host: string, patch: Partial<AppSettings>) => Promise<string | null>
  /** When the window saw the host go out of reach, in ms since the epoch; undefined when it has not. */
  awaySince: number | undefined
  /** How a refused change is reported; the console outside a test. */
  log?: (line: string) => void
}

export interface SettingsHost {
  /** The picked host's settings; null is Unknown. */
  values: AppSettings | null
  /** What the pane needs to say of the host: its name, whether it is remote, why its values are Unknown. */
  view: SettingsHostView
  /** Sends a change: the host's keys to the picked host, this device's to This PC. */
  write: (patch: Partial<AppSettings>) => void
}

const HOST_KEYS: ReadonlySet<string> = new Set(HOST_WORKSPACE_KEYS)

export function settingsHostFor({
  host,
  name,
  reached,
  local,
  remote,
  writeLocal,
  writeRemote,
  awaySince,
  log = (line) => console.error(line)
}: SettingsHostDeps): SettingsHost {
  const onThisPc = host === LOCAL_HOST_KEY
  const values = !reached ? null : onThisPc ? local : remote(host)
  const view: SettingsHostView = {
    name,
    remote: !onThisPc,
    unknown: !reached
      ? { kind: 'away', failedAt: awaySince === undefined ? null : clockTime(awaySince) }
      : values === null
        ? { kind: 'reading' }
        : null
  }
  const write = (patch: Partial<AppSettings>): void => {
    if (onThisPc) {
      writeLocal(patch)
      return
    }
    const kept: Record<string, unknown> = {}
    const own: Record<string, unknown> = {}
    for (const [key, value] of Object.entries(patch)) (HOST_KEYS.has(key) ? kept : own)[key] = value
    if (Object.keys(own).length > 0) writeLocal(own as Partial<AppSettings>)
    if (Object.keys(kept).length === 0) return
    void writeRemote(host, kept as Partial<AppSettings>).then((refused) => {
      if (refused !== null) log(`Settings: ${name} did not keep the change: ${refused}`)
    })
  }
  return { values, view, write }
}

/**
 * The plugins a secret on `host` may be shared with: the ones on that host
 * that are on. Settings › Secrets passes the host its switch picks, and a
 * plugin's own request for a secret passes the host the plugin runs on.
 */
export function secretChoicesOn(
  host: string,
  pluginsOn: (host: string) => ReadonlyMap<string, PluginInfo>
): SecretPluginChoice[] {
  return [...pluginsOn(host).entries()].map(([id, plugin]) => ({
    id,
    name: plugin.name,
    network: plugin.network,
    secrets: plugin.secrets.map((entry) => entry.key)
  }))
}
