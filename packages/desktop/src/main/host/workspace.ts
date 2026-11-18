import { DEFAULT_SETTINGS, HOST_SETTING_KEYS } from '@floti/core'
import type { AppSettings, CachedProject, DiscoveryResult, HostSettingKey } from '@floti/core'
import type {
  FolderTemplateKind,
  FolderTemplatePreview,
  SaveFolderAsTemplateResult,
  TemplateDeleteResult,
  TemplateDetail,
  TemplateImportResult,
  TemplateWriteResult
} from '@floti/core'
import type { ClaudeStatus, IpcRequests } from '../../shared/ipc'
import type { HostConnection, HostSubscription } from './client'
import type { HostLink } from './link'
import type * as P from './protocol'
import {
  cachedProjectsFromHost,
  claudeNeedsHostSettings,
  claudeStatusFromHost,
  NO_CLAUDE_BACKEND,
  discoveryFromHost,
  gitStatesFromHost,
  scanStatusFromHost,
  settingsWithHost,
  splitSettingsPatch
} from './records'
import { CLAUDE_CODE_BACKEND, type Emit } from './sessions'

/**
 * Discovery, scan roots, workspaces, setup, templates and the host's half of
 * the settings, the update check among them. The local host scans its own
 * machine's folders, keeps the scan roots, the pinned projects and whether
 * setup was finished, checks Claude Code for setup, makes workspaces and keeps
 * the templates; the desktop's main process does none of it.
 *
 * The settings are split by who owns them. The host's keys
 * (`HOST_WORKSPACE_KEYS`: the workspace keys, the update check and every
 * setting that governs what its sessions run and may do) are the host's and
 * are held here in memory, never written to `desktop.db`; nothing is pushed to
 * the host on connect. Everything else is the desktop's own. What the window
 * reads is the desktop's settings with the host's keys laid over; while the
 * host cannot be reached it is the desktop's own, so the window still paints,
 * and the sidebar says why from its own refused read. Main itself reads the
 * local host's `claude` from here, for its spike page and self-test
 * (`claudeChosen`), and the browser tools' switch (`overlay`).
 *
 * On the first connect to a host that has never been set up - no scan roots
 * and no finished setup - the desktop's own scan roots, pins and setup stamp are
 * written to it once, before `connection()` resolves, so a user who turns the
 * setting on keeps the sidebar and the finished setup they had.
 *
 * On the first connect after the host came to keep `HOST_SETTING_KEYS`, the
 * values this device kept for them in `desktop.db` are carried to the local
 * host once (`carryHostSettings`), and `desktop.db` keeps none from then on.
 *
 * On every connect it subscribes to `settings:changed`, `discovery:updated`,
 * `scan:status` and `git:updated` and hands each to the window, and it keeps
 * the host's last scan for what main asks of it (`knownProject`, the config
 * console's scope list).
 *
 * Every request goes through `link.connection()`, which joins the first
 * connect while it is in flight, so a window that asks before the desktop has reached
 * the host gets the host's answer, never an empty one first. When the host
 * cannot be reached, each rejects with the link's sentence; none answers an
 * empty list in its place.
 */

type Request<K extends keyof IpcRequests> = IpcRequests[K]['request']
type Response<K extends keyof IpcRequests> = IpcRequests[K]['response']

export interface WorkspaceTemplates extends TemplateService {
  list(): Promise<Response<'template:list'>>
  preview(request: Request<'template:preview'>): Promise<Response<'template:preview'>>
}

export interface WorkspaceService {
  /** The settings as the window sees them. */
  settings(): Promise<AppSettings>
  /** Writes a patch to wherever each key lives, and answers the settings after. */
  writeSettings(patch: Partial<AppSettings>): Promise<AppSettings>
  cached(): Promise<Response<'discovery:cached'>>
  /** A scan; the window is told it is running, its result and that it stopped. */
  scan(includeGit: boolean): Promise<Response<'discovery:scan'>>
  suggestRoots(): Promise<string[]>
  /** Adds each path that is not a root already; answers the roots after. */
  acceptRoots(paths: readonly string[]): Promise<string[]>
  removeRoot(path: string): Promise<string[]>
  setupStatus(): Promise<ClaudeStatus>
  /** Checks a picked executable, keeps it when it answers, and says what setup finds after. */
  locateClaude(path: string): Promise<ClaudeStatus>
  completeSetup(): Promise<AppSettings>
  createWorkspace(request: Request<'workspace:create'>): Promise<Response<'workspace:create'>>
  templates: WorkspaceTemplates
}

export interface TemplateService {
  detail: (template: string) => Promise<TemplateDetail>
  create: (req: {
    name: string
    label?: string | undefined
    description?: string | undefined
  }) => Promise<TemplateWriteResult>
  rename: (req: { template: string; name: string }) => Promise<TemplateWriteResult>
  remove: (template: string) => Promise<TemplateDeleteResult>
  metadata: (req: {
    template: string
    label: string
    description: string
  }) => Promise<TemplateWriteResult>
  substitute: (req: { template: string; path: string }) => Promise<TemplateWriteResult>
  importFiles: (req: {
    template: string
    scopePath: string
    paths: string[]
  }) => Promise<TemplateImportResult>
  folderPreview: (req: {
    dir: string
    kind: FolderTemplateKind
  }) => Promise<FolderTemplatePreview>
  fromFolder: (req: {
    dir: string
    kind: FolderTemplateKind
    name: string
    label: string
    description: string
    include: string[]
  }) => Promise<SaveFolderAsTemplateResult>
}

export interface HostWorkspaceDeps {
  link: HostLink
  emit: Emit
  /** The desktop's own settings, as they are now. */
  own: () => AppSettings
  /** Writes the desktop's own settings, and answers them after. */
  writeOwn: (patch: Partial<AppSettings>) => AppSettings
  /**
   * This device's values for `HOST_SETTING_KEYS` still waiting for the one
   * carry to the local host, as `desktop.db` holds them; null once carried.
   */
  hostSettingsToCarry: () => Partial<Pick<AppSettings, HostSettingKey>> | null
  /** Records the carry: `desktop.db` drops its rows for `HOST_SETTING_KEYS` and never carries again. */
  hostSettingsCarried: () => void
  /** The `claude` this computer finds by itself, without any chosen one. */
  discoveredClaude: () => string | null
  /** The `claude` the local host chose for its sessions, or null where it finds its own, each time it is heard. */
  claudeChosen: (path: string | null) => void
  log: (line: string) => void
}

const same = (a: unknown, b: unknown): boolean => JSON.stringify(a) === JSON.stringify(b)

/**
 * What the carry writes to the local host: each of this device's values that
 * differs from its default. The `claude` goes only where this device chose
 * one. The desktop used to write the one it found by itself when none was
 * chosen, so a host still holding exactly that one has it taken out, and finds
 * its own.
 */
export function carriedHostSettings(
  rows: Partial<Pick<AppSettings, HostSettingKey>>,
  current: P.HostSettings,
  discovered: string | null
): P.HostSettingsPatch {
  const differing: Partial<AppSettings> = {}
  for (const key of HOST_SETTING_KEYS) {
    if (rows[key] !== undefined && !same(rows[key], DEFAULT_SETTINGS[key])) Object.assign(differing, { [key]: rows[key] })
  }
  const held = current.backendPaths[CLAUDE_CODE_BACKEND]
  if (differing.claudePath === undefined && discovered !== null && held === discovered) differing.claudePath = null
  return splitSettingsPatch(differing, current).host
}

export interface HostWorkspace extends WorkspaceService {
  /** The desktop's settings as they are now, with the host's keys laid over once it has said them. */
  overlay(own: AppSettings): AppSettings
  /** The host's last scan, or null before it has said one. */
  lastScan(): DiscoveryResult | null
  /** The discovered project at `cwd`, by the path the host's discovery spells it, or null. */
  knownProject(cwd: string): string | null
  /** Asks the host to read git again; the window hears the answer on `git:updated`. */
  refreshGit(): Promise<void>
  shutdown(): void
}

export function createHostWorkspace({
  link,
  emit,
  own,
  writeOwn,
  hostSettingsToCarry,
  hostSettingsCarried,
  discoveredClaude,
  claudeChosen,
  log
}: HostWorkspaceDeps): HostWorkspace {
  let hostSettings: P.HostSettings | null = null
  let scanned: DiscoveryResult | null = null
  let cached: CachedProject[] = []
  let subscriptions: HostSubscription[] = []

  const host = (): Promise<HostConnection> => link.connection()
  const overlay = (settings: AppSettings): AppSettings => settingsWithHost(settings, hostSettings)

  /** The host's settings as last heard, and the `claude` they choose told to main. */
  const heard = (next: P.HostSettings): void => {
    hostSettings = next
    claudeChosen(next.backendPaths[CLAUDE_CODE_BACKEND] ?? null)
  }

  const adoptSettings = (next: P.HostSettings): AppSettings => {
    heard(next)
    return overlay(own())
  }

  const adoptScan = (result: P.DiscoveryResult): DiscoveryResult => {
    scanned = discoveryFromHost(result)
    return scanned
  }

  /** The desktop's scan roots, pins and setup stamp, written once to a host that has never been set up. */
  const copyOnFirstConnect = async (connection: HostConnection, current: P.HostSettings): Promise<P.HostSettings> => {
    const fresh = current.scanRoots.length === 0 && current.firstRunCompletedAt === null
    const mine = own()
    if (!fresh || (mine.scanRoots.length === 0 && mine.firstRunCompletedAt === null)) return current
    return connection.request('settings:write', {
      scanRoots: mine.scanRoots,
      pinnedProjects: mine.pinnedProjects,
      firstRunCompletedAt: mine.firstRunCompletedAt
    })
  }

  /**
   * A desktop that had the update check turned off before the host kept it says
   * so to the host, once: the desktop's own row then goes back to its default, so a
   * later change made at the host is never undone from here.
   */
  const carryUpdateCheck = async (connection: HostConnection, current: P.HostSettings): Promise<P.HostSettings> => {
    if (own().updateCheck) return current
    const next = current.updateCheck ? await connection.request('settings:write', { updateCheck: false }) : current
    writeOwn({ updateCheck: true })
    return next
  }

  /**
   * This device's values for `HOST_SETTING_KEYS`, carried to the local host
   * once, in one write, and then dropped from `desktop.db` with a marker that
   * keeps the carry from running again. A write the host refuses keeps both
   * as they were, and the next connect tries again; a host on another
   * computer is never carried to.
   */
  const carryHostSettings = async (connection: HostConnection, current: P.HostSettings): Promise<P.HostSettings> => {
    try {
      const rows = hostSettingsToCarry()
      if (rows === null) return current
      const patch = carriedHostSettings(rows, current, discoveredClaude())
      const next = Object.keys(patch).length === 0 ? current : await connection.request('settings:write', patch)
      hostSettingsCarried()
      return next
    } catch (error) {
      log(
        `host: this device's settings for its host were not carried to the local host, and are tried again on the next connect (${(error as Error).message})`
      )
      return current
    }
  }

  link.onConnected(async (connection) => {
    for (const subscription of subscriptions) subscription.unsubscribe()
    const ended = (channel: string) => (error: Error) => log(`host: ${channel} ended (${error.message})`)
    try {
      const read = await connection.request('settings:read', null)
      const copied = await carryUpdateCheck(connection, await copyOnFirstConnect(connection, read))
      adoptSettings(await carryHostSettings(connection, copied))
    } catch (error) {
      log(`host: could not read the local host's settings (${(error as Error).message})`)
    }
    subscriptions = [
      connection.subscribe('settings:changed', {
        snapshot: heard,
        event: (next) => emit('settings:changed', adoptSettings(next)),
        error: ended('settings:changed')
      }),
      connection.subscribe('discovery:updated', {
        snapshot: (state) => {
          if (state !== null) adoptScan(state)
        },
        event: (result) => emit('discovery:updated', adoptScan(result)),
        error: ended('discovery:updated')
      }),
      connection.subscribe('scan:status', {
        event: (status) => emit('scan:status', scanStatusFromHost(status)),
        error: ended('scan:status')
      }),
      connection.subscribe('git:updated', {
        event: (read) => {
          const states = gitStatesFromHost(read)
          if (scanned !== null) {
            for (const project of scanned.projects) {
              if (project.path in states) project.git = states[project.path] ?? null
            }
          }
          emit('git:updated', states)
        },
        error: ended('git:updated')
      })
    ]
  })

  /** Setup's reading of Claude Code, out of the host's one per backend. */
  const claude = (status: P.SetupStatus): ClaudeStatus => {
    const found = claudeStatusFromHost(status.backends[CLAUDE_CODE_BACKEND])
    // A host with no Claude Code backend did not look: a failed read, not a missing claude.
    if (found === null) throw new Error(NO_CLAUDE_BACKEND)
    return found
  }

  const workspace: HostWorkspace = {
    overlay,

    lastScan: () => scanned,

    knownProject(cwd) {
      const wanted = cwd.toLowerCase()
      const projects: ReadonlyArray<{ path: string }> = scanned?.projects ?? cached
      return projects.find((project) => project.path.toLowerCase() === wanted)?.path ?? null
    },

    async refreshGit() {
      // The host pushes what it read on `git:updated`, which is relayed above.
      await (await host()).request('git:refresh', null)
    },

    async settings() {
      try {
        await host()
      } catch {
        return own()
      }
      return overlay(own())
    },

    async writeSettings(patch) {
      // A change of the claude replaces the host's whole map of executables,
      // whose others are kept as the host holds them: they are read first
      // when not known, and with them unreadable nothing is written.
      if (patch.claudePath !== undefined) {
        const connection = await host()
        if (hostSettings === null) {
          try {
            heard(await connection.request('settings:read', null))
          } catch (error) {
            throw new Error(claudeNeedsHostSettings((error as Error).message), { cause: error })
          }
        }
      }
      const split = splitSettingsPatch(patch, hostSettings)
      const mine = Object.keys(split.own).length > 0 ? writeOwn(split.own) : own()
      if (Object.keys(split.host).length === 0) return overlay(mine)
      return adoptSettings(await (await host()).request('settings:write', split.host))
    },

    async cached() {
      cached = cachedProjectsFromHost(await (await host()).request('discovery:cached', null))
      return cached
    },

    async scan(includeGit) {
      return adoptScan(await (await host()).request('discovery:scan', { includeGit }))
    },

    async suggestRoots() {
      return (await host()).request('roots:suggest', null)
    },

    async acceptRoots(paths) {
      const connection = await host()
      let roots = hostSettings?.scanRoots ?? []
      for (const path of paths) roots = await connection.request('roots:accept', { path })
      return roots
    },

    async removeRoot(path) {
      return (await host()).request('roots:remove', { path })
    },

    async setupStatus() {
      return claude(await (await host()).request('setup:status', null))
    },

    async locateClaude(path) {
      const connection = await host()
      const status = await connection.request('setup:locateBackend', { backend: CLAUDE_CODE_BACKEND, path })
      // Kept by the host only once it ran, in its settings, which every window hears.
      if (status.error === null && status.installed) {
        emit('settings:changed', adoptSettings(await connection.request('settings:read', null)))
      }
      return claudeStatusFromHost(status)
    },

    async completeSetup() {
      return adoptSettings(await (await host()).request('setup:complete', null))
    },

    async createWorkspace(request) {
      return (await host()).request('workspace:create', {
        mode: request.mode,
        dir: request.dir,
        name: request.name ?? null,
        template: request.mode === 'new' ? (request.template ?? null) : null
      })
    },

    templates: {
      list: async () => (await host()).request('template:list', null),
      preview: async ({ template, mode }) =>
        (await host()).request('template:preview', {
          template,
          mode: mode ?? null
        }),
      detail: async (template) => (await host()).request('template:detail', { template }),
      create: async ({ name, label, description }) =>
        (await host()).request('template:create', {
          name,
          label: label ?? null,
          description: description ?? null
        }),
      rename: async (request) => (await host()).request('template:rename', request),
      remove: async (template) => (await host()).request('template:delete', { template }),
      metadata: async (request) => (await host()).request('template:metadata', request),
      substitute: async (request) => (await host()).request('template:substitute', request),
      importFiles: async (request) => (await host()).request('template:import', request),
      folderPreview: async (request) => (await host()).request('template:folderPreview', request),
      fromFolder: async (request) => (await host()).request('template:fromFolder', request)
    },

    shutdown() {
      for (const subscription of subscriptions) subscription.unsubscribe()
      subscriptions = []
    }
  }
  return workspace
}
