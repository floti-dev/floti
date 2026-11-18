import { useId, type JSX } from 'react'
import { cn } from '../lib/cn'
import { ROW_SELECTED } from '../lib/rows'
import { HostSwitch, type HostChoice } from './HostSwitch'

/**
 * Settings, one section at a time: the sections listed in the sidebar, the one
 * picked drawn in the pane (the Appearance board's layout).
 *
 * The page it replaces was every group on one scroll, twelve of them, which
 * is a page you search rather than read. A section is a handful of related
 * groups, named for what somebody comes to change.
 *
 * The sections sit in two groups. Floti desktop is this computer's window and
 * what it keeps in its own store. Floti host is what each host keeps and
 * enforces for every session on it, whichever window started it: its label
 * carries the host switch, which picks for every section in the group, so a
 * change always lands on the host the page names.
 */
export type SettingsSectionId =
  | 'general'
  | 'appearance'
  | 'terminal'
  | 'sessions'
  | 'browser'
  | 'files'
  /** The hosts on other computers. */
  | 'hosts'
  /** The devices attached to each connected host. */
  | 'devices'
  /** The programs a host runs: its Claude CLI and the shell for project panes. */
  | 'programs'
  /** Which of Floti's tools a host's sessions get. */
  | 'tools'
  /** A host's agents, and the limits on the sessions they start. */
  | 'agents'
  | 'workspace'
  | 'plugins'
  | 'secrets'
  | 'archive'
  | 'updates'
  /** One plugin's page, by its folder: a plugin whose manifest failed has no id. */
  | `plugin:${string}`

/** Which group a section sits in: this desktop's own, or the picked host's. */
export type SettingsGroup = 'desktop' | 'host'

export interface SettingsSection {
  id: SettingsSectionId
  label: string
  group: SettingsGroup
  /**
   * The line under the section's title. Only a section holding more than one
   * group has one here; a section that is a single group says its group's own
   * hint there instead.
   */
  hint?: string
}

/** In the order the sidebar lists them: this desktop's first, then the host's; in each, the daily things first. */
export const SETTINGS_SECTIONS: readonly SettingsSection[] = [
  { id: 'general', label: 'General', group: 'desktop' },
  { id: 'appearance', label: 'Appearance', group: 'desktop' },
  { id: 'terminal', label: 'Terminal', group: 'desktop' },
  { id: 'sessions', label: 'Sessions', group: 'desktop' },
  {
    id: 'browser',
    label: 'Browser',
    group: 'desktop',
    hint: 'Pages in the Browser tab. Sites may copy to your clipboard and go full screen; downloads go to your own browser, and nothing else a site asks for is allowed.'
  },
  { id: 'files', label: 'Files', group: 'desktop' },
  { id: 'hosts', label: 'Hosts', group: 'desktop' },
  { id: 'devices', label: 'Devices', group: 'desktop' },
  { id: 'programs', label: 'Programs', group: 'host' },
  { id: 'tools', label: 'Tools', group: 'host' },
  { id: 'agents', label: 'Agents', group: 'host' },
  { id: 'workspace', label: 'Workspace', group: 'host' },
  { id: 'plugins', label: 'Plugins', group: 'host' },
  { id: 'secrets', label: 'Secrets', group: 'host' },
  { id: 'archive', label: 'Archive', group: 'host' },
  { id: 'updates', label: 'Updates', group: 'host' }
]

/** Which group `id` sits in; a plugin's page is in Plugins', the host's. */
export function settingsGroupOf(id: SettingsSectionId): SettingsGroup {
  if (id.startsWith('plugin:')) return 'host'
  return SETTINGS_SECTIONS.find((section) => section.id === id)?.group ?? 'desktop'
}

/** A registered plugin, listed under Plugins with a page of its own. */
export interface SettingsPluginLink {
  path: string
  name: string
  /** Turned off, or failed to load: said beside its name. */
  state: 'on' | 'off' | 'error'
}

/** The host the Floti host group shows, and the switch to the others. */
export interface SettingsHostPick {
  /** The picked host's name: beside the label, in mono, while there is no switch. */
  name: string
  /** This PC and each host on another computer, while there is more than one to pick from. */
  choice?: HostChoice | undefined
}

export interface SettingsSectionsProps {
  current: SettingsSectionId
  onSelect: (id: SettingsSectionId) => void
  plugins?: readonly SettingsPluginLink[] | undefined
  /** The host the Floti host group shows; This PC when not given. */
  host?: SettingsHostPick | undefined
}

export const pluginSection = (path: string): SettingsSectionId => `plugin:${path}`

const THIS_PC: SettingsHostPick = { name: 'This PC' }

/** A group's caps label, in the sidebar's recipe. */
const GROUP_LABEL = 'shrink-0 text-[10px] leading-[1.4] font-semibold tracking-[.07em] text-fg-subtle uppercase'

/** The sidebar's list of sections, in its two groups, with the sidebar's own selected-row recipe. */
export function SettingsSections({
  current,
  onSelect,
  plugins = [],
  host = THIS_PC
}: SettingsSectionsProps): JSX.Element {
  const ids = useId()
  const row = (section: SettingsSection): JSX.Element => {
    const selected = section.id === current
    const link = (
      <button
        key={section.id}
        type="button"
        data-settings-section-link={section.id}
        aria-current={selected ? 'page' : undefined}
        onClick={() => onSelect(section.id)}
        className={cn(
          'relative flex h-line w-full items-center rounded-raised px-2.5 text-left text-[12.5px] transition-colors',
          selected ? cn(ROW_SELECTED, 'font-medium text-fg') : 'text-fg-muted hover:bg-hover hover:text-fg'
        )}
      >
        {selected && <span aria-hidden className="absolute top-1.5 bottom-1.5 left-0 w-[2px] rounded-full bg-accent" />}
        {section.label}
      </button>
    )
    if (section.id !== 'plugins' || plugins.length === 0) return link
    // Each plugin's own page, under Plugins, a step in.
    return (
      <div key={section.id} className="contents">
        {link}
        {plugins.map((plugin) => {
          const id = pluginSection(plugin.path)
          const chosen = id === current
          return (
            <button
              key={id}
              type="button"
              data-settings-plugin-link={plugin.path}
              aria-current={chosen ? 'page' : undefined}
              title={plugin.path}
              onClick={() => onSelect(id)}
              className={cn(
                'relative flex h-line w-full items-center gap-2 rounded-raised pr-2.5 pl-6 text-left text-[12.5px] transition-colors',
                chosen ? cn(ROW_SELECTED, 'font-medium text-fg') : 'text-fg-muted hover:bg-hover hover:text-fg'
              )}
            >
              {chosen && (
                <span aria-hidden className="absolute top-1.5 bottom-1.5 left-0 w-[2px] rounded-full bg-accent" />
              )}
              <span className="min-w-0 flex-1 truncate">{plugin.name}</span>
              {plugin.state === 'off' && <span className="shrink-0 text-[11px] font-normal text-fg-subtle">Off</span>}
              {plugin.state === 'error' && (
                <span className="shrink-0 text-[11px] font-normal text-danger">Not loaded</span>
              )}
            </button>
          )
        })}
      </div>
    )
  }
  const desktop = SETTINGS_SECTIONS.filter((section) => section.group === 'desktop')
  const hosted = SETTINGS_SECTIONS.filter((section) => section.group === 'host')

  return (
    <nav aria-label="Settings sections" className="flex flex-col gap-px px-1.5 pb-1.5">
      <div role="group" aria-labelledby={`${ids}-desktop`} data-settings-group-list="desktop" className="contents">
        <div className="flex items-center gap-2 px-2 pt-3 pb-[5px]">
          <span id={`${ids}-desktop`} className={GROUP_LABEL}>
            Floti desktop
          </span>
        </div>
        {desktop.map(row)}
      </div>
      <div aria-hidden className="mx-2 mt-2 h-px bg-border" />
      <div role="group" aria-labelledby={`${ids}-host`} data-settings-group-list="host" className="contents">
        <div data-settings-host-label className="flex min-w-0 items-center gap-2 px-2 pt-3 pb-[5px]">
          <span id={`${ids}-host`} className={GROUP_LABEL}>
            Floti host
          </span>
          {host.choice === undefined ? (
            <span data-settings-host-name className="min-w-0 truncate font-mono text-[10.5px] text-fg-muted" title={host.name}>
              {host.name}
            </span>
          ) : (
            <HostSwitch surface="Settings" {...host.choice} size="small" />
          )}
        </div>
        {hosted.map(row)}
      </div>
    </nav>
  )
}
