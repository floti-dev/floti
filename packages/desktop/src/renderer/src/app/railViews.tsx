import { AgentCardIcon, LayersIcon, RAIL_ICON, TerminalIcon, type RailItem } from '@helm/ui'
import type { SidebarView } from './sidebarFor'

/**
 * The rail's first views, in their order: Sessions, Profiles, then Agents.
 * Each shows its list in the sidebar, and is current while the sidebar shows
 * it (`shown`, null while the sidebar is hidden).
 */
export function viewRailItems({
  shown,
  waiting,
  onToggle
}: {
  shown: SidebarView | null
  /** Whether a session is waiting on the user: Sessions' attention mark. */
  waiting: boolean
  onToggle: (view: SidebarView) => void
}): RailItem[] {
  return [
    {
      id: 'sessions',
      label: 'Sessions',
      icon: <TerminalIcon {...RAIL_ICON} />,
      kind: 'view',
      current: shown === 'sessions',
      attention: waiting,
      onSelect: () => onToggle('sessions')
    },
    {
      id: 'profiles',
      label: 'Profiles',
      icon: <LayersIcon {...RAIL_ICON} />,
      kind: 'view',
      current: shown === 'profiles',
      onSelect: () => onToggle('profiles'),
      hooks: { 'data-open-profiles': true }
    },
    {
      id: 'agents',
      label: 'Agents',
      icon: <AgentCardIcon {...RAIL_ICON} />,
      kind: 'view',
      current: shown === 'agents',
      onSelect: () => onToggle('agents'),
      hooks: { 'data-open-agents': true }
    }
  ]
}
