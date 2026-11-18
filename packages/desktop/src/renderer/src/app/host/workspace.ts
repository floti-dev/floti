import type { AppSettings, Project } from '@floti/core/types'
import { CLAUDE_CODE_BACKEND } from '../../../../shared/host/backend'
import type { HostConnection, HostSubscription } from '../../../../shared/host/client'
import type * as P from '../../../../shared/host/protocol'
import {
  cachedProjectsFromHost,
  claudeStatusFromHost,
  NO_CLAUDE_BACKEND,
  discoveryFromHost,
  gitStatesFromHost,
  scanStatusFromHost,
  settingsWithHost,
  splitSettingsPatch
} from '../../../../shared/host/records'
import type { ClaudeStatus, EventPayload, DesktopBridge } from '../../../../shared/ipc'
import { forAction, type HostReach } from './history'
import type { HostAdapter } from './router'

/**
 * Discovery, scan roots, workspaces, setup, templates and the settings. The host scans its own machine's folders,
 * keeps the scan roots, the pinned projects and whether setup was finished,
 * checks Claude Code, makes workspaces and keeps the templates, and the window
 * asks it over its own connection, in ipc.ts's shapes.
 *
 * The settings are split by who keeps them. The host's half
 * (`HOST_WORKSPACE_KEYS`: the scan roots, pins and setup stamp, the update
 * check, and every setting that governs what its sessions run and may do,
 * the `claude` among them) is read and written over this host's own
 * connection, and every window attached to the host hears each change on
 * `settings:changed`, whoever made it. The desktop's own half - the window,
 * theme, terminal look and browser preferences - stays with the desktop's
 * main process on the client-only `ownSettings:*`. The window is answered the
 * desktop's half with the host's laid over; while this PC's host cannot be
 * used it is the desktop's own, so the window still paints its tabs and saves
 * their layout, and the window's notice says why.
 *
 * The pickers are the window's own: `path:chooseFile` for the `claude`
 * program, which the host checks and keeps (`setup:locateBackend`).
 *
 * The first listener starts the window's first scan, as main did when the
 * window said it had painted; a host that cannot be reached says why on
 * `scan:status`. While the window has a scan from the host, its coming to the
 * front reads git again, which the host pushes on `git:updated`.
 *
 * Every request goes through `link.connection()`, which joins the first
 * connect while it is in flight, so a read asked before the window has reached
 * the host gets the host's answer, never an empty one first; with no host each
 * rejects with the link's sentence, never an empty list in its place; this
 * PC's link waits through a host that cannot be used instead. What the desktop keeps
 * itself (its settings, its build) is answered without the host meanwhile
 * (`hostIfUsable`).
 */

type Pushed = 'settings:changed' | 'discovery:updated' | 'scan:status' | 'git:updated'

export interface WorkspaceAdapterDeps {
  link: HostReach
  /** The preload's bridge, for the desktop's own settings, build and pickers. */
  own: Pick<DesktopBridge, 'invoke' | 'on'>
  /** Calls `listener` whenever the window comes to the front. */
  onFocus?: (listener: () => void) => void
  /**
   * Set for a host on another computer, named as the window names it. Its
   * router carries only that host's half of the settings: a write of this
   * device's own half is refused there, before anything is written, because
   * this device's settings are read and written only through this PC's bridge
   * and main. Its `claude` is not located from this PC's file picker either.
   */
  remote?: { name: () => string }
  log?: (line: string) => void
}

/** Why a host on another computer's router refuses a write of this device's own settings. */
export const ownSettingsElsewhere = (host: string): string =>
  `This device’s own settings are kept on This PC and never sent to ${host}.`

/** Why a host on another computer's `claude` is not located from this PC's file picker. */
export const pickerElsewhere = (host: string): string =>
  `This PC’s file picker browses this PC, so it cannot choose the claude ${host} runs.`

export interface WorkspaceAdapter extends HostAdapter {
  /** The workspaces and projects on the sidebar: the host's last scan, or its cached one before any. */
  scannedProjects(): Promise<readonly Project[]>
}

export function createWorkspaceAdapter({
  link,
  own,
  onFocus,
  remote,
  log = () => undefined
}: WorkspaceAdapterDeps): WorkspaceAdapter {
  const listeners: { [K in Pushed]: Set<(payload: EventPayload<K>) => void> } = {
    'settings:changed': new Set(),
    'discovery:updated': new Set(),
    'scan:status': new Set(),
    'git:updated': new Set()
  }
  let hostSettings: P.HostSettings | null = null
  let mine: AppSettings | null = null
  let scanned: readonly Project[] | null = null
  let started = false
  let subscriptions: HostSubscription[] = []

  const host = (): Promise<HostConnection> => link.connection()
  /** The host for a read that answers with the desktop's own half without it: no wait while this PC's host cannot be used. */
  const hostIfUsable = (): Promise<HostConnection> => link.connectionIfUsable?.() ?? link.connection()
  /** For what a person starts: fails at once, with a short sentence, while this PC's host cannot be used. */
  const act = forAction(link)
  /** The settings last told, which a listener that comes later hears at once. */
  let toldSettings: AppSettings | null = null
  const tell = <K extends Pushed>(channel: K, payload: EventPayload<K>): void => {
    if (channel === 'settings:changed') toldSettings = payload as AppSettings
    for (const listener of listeners[channel]) listener(payload)
  }

  const ownSettings = async (): Promise<AppSettings> => (mine ??= await own.invoke('ownSettings:read'))
  /**
   * Tells every listener the desktop's settings with the host's laid over once the
   * host's are known: read first when they are not yet, and the desktop's alone only
   * when the host cannot be reached. Told before then, the window would take
   * the host's half (its scan roots among them) for empty.
   */
  let hostAsked: Promise<void> | null = null
  /** Resolves once the host's settings are known, or the host could not be reached for them. */
  const hostKnown = (): Promise<void> => {
    if (hostSettings !== null) return Promise.resolve()
    hostAsked ??= hostIfUsable()
      .then((connection) => connection.request('settings:read', null))
      .then(
        (read) => {
          hostSettings ??= read
        },
        () => undefined
      )
      .finally(() => {
        hostAsked = null
      })
    return hostAsked
  }
  const tellSettled = (): void => {
    // Told at once when the host's are known: a scan waits on this telling.
    if (hostSettings !== null && mine !== null) {
      tell('settings:changed', settingsWithHost(mine, hostSettings))
      return
    }
    void hostKnown().then(() => {
      if (mine !== null) tell('settings:changed', settingsWithHost(mine, hostSettings))
    })
  }
  /** The desktop's settings with the host's laid over, told to every listener. */
  const settled = (settings: AppSettings): AppSettings => {
    mine = settings
    tellSettled()
    return settingsWithHost(settings, hostSettings)
  }
  const adoptHost = async (next: P.HostSettings): Promise<AppSettings> => {
    hostSettings = next
    // The desktop's half as it is when the answer lands, not when it was asked for.
    await ownSettings()
    return settled(mine ?? (await ownSettings()))
  }

  link.onConnected((connection) => {
    for (const subscription of subscriptions) subscription.unsubscribe()
    const ended = (channel: string) => (error: Error) => log(`host: ${channel} ended (${error.message})`)
    // The window is told this connection's settings before its first scan,
    // so nothing that offers a scan root shows the projects without them.
    let settingsTold: Promise<unknown> = Promise.resolve()
    const discovered = (result: P.DiscoveryResult): void => {
      const discovery = discoveryFromHost(result)
      void settingsTold.then(() => {
        scanned = discovery.projects
        tell('discovery:updated', discovery)
      })
    }
    subscriptions = [
      connection.subscribe('settings:changed', {
        snapshot: (state) => {
          settingsTold = adoptHost(state).catch(ended('settings:changed'))
        },
        event: (next) => void adoptHost(next).catch(ended('settings:changed')),
        error: ended('settings:changed')
      }),
      connection.subscribe('discovery:updated', {
        snapshot: (state) => {
          if (state !== null) discovered(state)
        },
        event: discovered,
        error: ended('discovery:updated')
      }),
      connection.subscribe('scan:status', {
        event: (status) => tell('scan:status', scanStatusFromHost(status)),
        error: ended('scan:status')
      }),
      connection.subscribe('git:updated', {
        event: (read) => tell('git:updated', gitStatesFromHost(read)),
        error: ended('git:updated')
      })
    ]
  })

  /** The window's first scan; a host that cannot be reached says why where the scan shows. */
  const start = (): void => {
    if (started) return
    started = true
    own.on('ownSettings:changed', (settings) => void settled(settings))
    void host()
      .then((connection) => connection.request('discovery:scan', { includeGit: true }))
      .catch((error: unknown) => {
        tell('scan:status', { running: false, error: error instanceof Error ? error.message : String(error) })
      })
  }

  let gitInFlight = false
  onFocus?.(() => {
    // Only a window that has the host's scan has projects whose git to read.
    if (gitInFlight || scanned === null) return
    gitInFlight = true
    void host()
      .then((connection) => connection.request('git:refresh', null))
      .catch(() => undefined)
      .finally(() => {
        gitInFlight = false
      })
  })

  const listen =
    <K extends Pushed>(channel: K) =>
    (listener: (payload: EventPayload<K>) => void): (() => void) => {
      listeners[channel].add(listener)
      // The connection may have told its settings before the window listened,
      // and a scan told after them must not land on a window without them.
      if (channel === 'settings:changed' && toldSettings !== null) listener(toldSettings as EventPayload<K>)
      start()
      return () => {
        listeners[channel].delete(listener)
      }
    }

  /** Setup's reading of Claude Code, out of the host's one per backend. */
  const claude = (status: P.SetupStatus): ClaudeStatus => {
    const found = claudeStatusFromHost(status.backends[CLAUDE_CODE_BACKEND])
    // A host with no Claude Code backend did not look: a failed read, not a missing claude.
    if (found === null) throw new Error(NO_CLAUDE_BACKEND)
    return found
  }

  return {
    async scannedProjects() {
      return scanned ?? cachedProjectsFromHost(await (await host()).request('discovery:cached', null))
    },

    requests: {
      'app:info': async () => {
        const build = await own.invoke('ownApp:info')
        let hostLogFile: string | null = null
        try {
          hostLogFile = (await (await hostIfUsable()).request('app:info', null)).logFile
        } catch (error) {
          log(`host: the host's log file could not be read (${(error as Error).message})`)
        }
        return { ...build, hostLogFile }
      },

      'settings:read': async () => {
        const settings = await ownSettings()
        let connection: HostConnection
        try {
          connection = await hostIfUsable()
        } catch {
          return settings
        }
        hostSettings = await connection.request('settings:read', null)
        return settingsWithHost(settings, hostSettings)
      },

      'settings:write': async (patch) => {
        // A change of the claude replaces the host's whole map of executables,
        // whose others are kept as the host holds them: read first when not known.
        if (patch.claudePath !== undefined) await hostKnown()
        const split = splitSettingsPatch(patch, hostSettings)
        if (remote !== undefined && Object.keys(split.own).length > 0) throw new Error(ownSettingsElsewhere(remote.name()))
        // A write of the desktop's half alone answers with the host's laid over, so
        // the host's are read first when they are not known yet.
        if (Object.keys(split.host).length === 0) await hostKnown()
        const settings =
          Object.keys(split.own).length > 0 ? await own.invoke('ownSettings:write', split.own) : await ownSettings()
        if (Object.keys(split.host).length === 0) return settled(settings)
        hostSettings = await (await act()).request('settings:write', split.host)
        return settled(settings)
      },

      'discovery:cached': async () => cachedProjectsFromHost(await (await host()).request('discovery:cached', null)),
      'discovery:scan': async (request) =>
        discoveryFromHost(await (await host()).request('discovery:scan', { includeGit: request?.includeGit ?? true })),

      'roots:suggest': async () => (await host()).request('roots:suggest', null),
      'roots:accept': async ({ path }) => (await act()).request('roots:accept', { path }),
      'roots:remove': async ({ path }) => (await act()).request('roots:remove', { path }),

      'setup:status': async () => claude(await (await host()).request('setup:status', null)),
      // The picker is the window's own; the host checks what it chose and
      // keeps it, and every window attached hears it on `settings:changed`.
      'setup:locateClaude': async () => {
        if (remote !== undefined) throw new Error(pickerElsewhere(remote.name()))
        // Said before the picker opens: a choice nobody could check would be lost.
        const connection = await act()
        const { path } = await own.invoke('path:chooseFile', { title: 'Locate the claude executable' })
        if (path === null) return claude(await connection.request('setup:status', null))
        const status = await connection.request('setup:locateBackend', { backend: CLAUDE_CODE_BACKEND, path })
        // What the host kept is in its settings now; the window is answered them before the status.
        if (status.error === null && status.installed) await adoptHost(await connection.request('settings:read', null))
        return claudeStatusFromHost(status)
      },
      'setup:complete': async () => adoptHost(await (await act()).request('setup:complete', null)),

      'workspace:create': async (request) =>
        (await act()).request('workspace:create', {
          mode: request.mode,
          dir: request.dir,
          name: request.name ?? null,
          template: request.mode === 'new' ? (request.template ?? null) : null
        }),

      'template:list': async () => (await host()).request('template:list', null),
      'template:preview': async ({ template, mode }) =>
        (await host()).request('template:preview', { template, mode: mode ?? null }),
      'template:detail': async ({ template }) => (await host()).request('template:detail', { template }),
      'template:create': async ({ name, label, description }) =>
        (await act()).request('template:create', { name, label: label ?? null, description: description ?? null }),
      'template:rename': async (request) => (await act()).request('template:rename', request),
      'template:delete': async ({ template }) => (await act()).request('template:delete', { template }),
      'template:metadata': async (request) => (await act()).request('template:metadata', request),
      'template:substitute': async (request) => (await act()).request('template:substitute', request),
      'template:import': async (request) => (await act()).request('template:import', request),
      'template:folderPreview': async (request) => (await host()).request('template:folderPreview', request),
      'template:fromFolder': async (request) => (await host()).request('template:fromFolder', request)
    },
    events: {
      'settings:changed': listen('settings:changed'),
      'discovery:updated': listen('discovery:updated'),
      'scan:status': listen('scan:status'),
      'git:updated': listen('git:updated')
    }
  }
}
