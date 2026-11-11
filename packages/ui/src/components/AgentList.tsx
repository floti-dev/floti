import { useState, type JSX, type ReactNode } from 'react'
import type { LaunchAgents } from '@helm/core/types'
import { cn } from '../lib/cn'
import { PlusIcon } from './icons'
import { Menu, type MenuEntry } from './Menu'
import { SidebarAction } from './Sidebar'

export interface AgentListProps {
  /** The host's agents as it lists them now; null while the first answer is on its way. */
  listing: LaunchAgents | null
  /** Why the list could not be read, as a sentence. */
  error: string | null
  /** The host, as the window names it ("This PC"). */
  hostName: string
  /** Whether the host is on another computer: its name is then machine data. */
  remote: boolean
  /** The agent or bad file whose page is in front, by name. */
  selected: string | null
  onOpen: (name: string) => void
  onAdd: (name: string) => void
}

const SECTION = 'flex items-center gap-1.5 px-2 pt-3 pb-1 text-[9.5px] font-semibold tracking-[.08em] text-fg-subtle uppercase'
const ROW = 'flex w-full min-w-0 flex-col gap-px rounded-raised px-2 py-row text-left transition-colors hover:bg-hover'

/**
 * The Agents view's list (DESIGN 5, the design's A4): the host's agents by
 * name, each with its first backend and its description; the files that are
 * not agents with the host's problem; and the shipped agents the folder lacks,
 * each with Add. A row carries nothing else: everything about an agent is on
 * its page. The foot names the folder and the host.
 */
export function AgentList({ listing, error, hostName, remote, selected, onOpen, onAdd }: AgentListProps): JSX.Element {
  if (listing === null) {
    return (
      <p data-agents-state className="px-3.5 py-2 text-[12px] leading-relaxed text-fg-subtle">
        {error ?? 'Reading the agents…'}
      </p>
    )
  }
  return (
    <div data-agent-list className="flex min-h-0 flex-1 flex-col">
      <div className="min-h-0 flex-1 overflow-y-auto px-1.5 pb-2">
        {error !== null && <p className="px-2 py-1.5 text-[11.5px] text-danger">{error}</p>}
        {listing.agents.length === 0 && (
          <p className="px-2 py-1.5 text-[12px] text-fg-subtle">No agents in this folder yet.</p>
        )}
        <ul className="m-0 list-none p-0">
          {listing.agents.map((agent) => (
            <li key={agent.name}>
              <button
                type="button"
                data-agent-row={agent.name}
                aria-current={selected === agent.name ? 'page' : undefined}
                onClick={() => onOpen(agent.name)}
                className={cn(ROW, selected === agent.name && 'bg-active hover:bg-active')}
              >
                <span className="flex min-w-0 items-baseline gap-2">
                  <span className="min-w-0 flex-1 truncate text-[12px] text-fg">{agent.name}</span>
                  {(agent.listedBackends[0] ?? agent.backends[0]) !== undefined && (
                    <span className="shrink-0 font-mono text-[10px] text-fg-subtle">
                      {agent.listedBackends[0] ?? agent.backends[0]}
                    </span>
                  )}
                </span>
                {agent.description !== '' && (
                  <span className="block truncate text-[10.5px] text-fg-muted">{agent.description}</span>
                )}
              </button>
            </li>
          ))}
        </ul>
        {listing.bad.length > 0 && (
          <Section title="Not loaded" count={listing.bad.length}>
            {listing.bad.map((bad) => (
              <li key={bad.file}>
                <button
                  type="button"
                  data-bad-agent-row={bad.name}
                  aria-current={selected === bad.name ? 'page' : undefined}
                  onClick={() => onOpen(bad.name)}
                  className={cn(ROW, selected === bad.name && 'bg-active hover:bg-active')}
                >
                  <span className="block truncate font-mono text-[11px] text-fg">{bad.file}</span>
                  <span className="line-clamp-2 text-[10.5px] leading-snug text-danger">
                    {bad.problems[0]?.message ?? bad.problem}
                  </span>
                </button>
              </li>
            ))}
          </Section>
        )}
        {listing.shipped.length > 0 && (
          <Section title="Shipped with Floti" count={listing.shipped.length}>
            {listing.shipped.map((shipped) => (
              <li
                key={shipped.name}
                data-shipped-agent-row={shipped.name}
                className="group flex min-w-0 flex-col gap-px rounded-raised px-2 py-row focus-within:bg-hover hover:bg-hover"
              >
                <span className="flex min-w-0 items-center gap-2">
                  <span className="min-w-0 flex-1 truncate text-[12px] text-fg-muted">{shipped.name}</span>
                  {listing.readOnly === null && (
                    <button
                      type="button"
                      data-add-shipped={shipped.name}
                      onClick={() => onAdd(shipped.name)}
                      className="shrink-0 rounded-well border border-border-strong px-[7px] py-px text-[10.5px] text-fg opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100 hover:bg-hover"
                    >
                      Add
                    </button>
                  )}
                </span>
                <span className="block truncate text-[10.5px] text-fg-subtle">{shipped.description}</span>
              </li>
            ))}
          </Section>
        )}
      </div>
      <p data-agents-folder className="m-0 border-t border-border px-3.5 py-2 text-[10.5px] leading-relaxed text-fg-subtle">
        Files in <span className="font-mono">{listing.folder}</span> on{' '}
        <span className={cn(remote && 'font-mono')}>{hostName}</span>, read afresh on every start.
        {listing.readOnly !== null && <span className="mt-1 block text-warn">{listing.readOnly}</span>}
      </p>
    </div>
  )
}

function Section({ title, count, children }: { title: string; count: number; children: ReactNode }): JSX.Element {
  return (
    <section>
      <h3 className={SECTION}>
        {title}
        <span className="font-mono font-normal tracking-normal">{count}</span>
      </h3>
      <ul className="m-0 list-none p-0">{children}</ul>
    </section>
  )
}

export interface NewAgentButtonProps {
  /** The agents a copy can be made of, by name. */
  agents: readonly string[]
  disabled: boolean
  onBlank: () => void
  onCopy: (name: string) => void
}

/** The + in the Agents header (the design's A11): a blank agent, or a copy of any agent. */
export function NewAgentButton({ agents, disabled, onBlank, onCopy }: NewAgentButtonProps): JSX.Element {
  const [at, setAt] = useState<DOMRect | null>(null)
  const entries: MenuEntry[] = [
    { kind: 'item', id: 'blank', label: 'Blank agent' },
    ...(agents.length === 0
      ? []
      : [
          { kind: 'heading', id: 'copies', label: 'Copy of' } as const,
          ...agents.map((name) => ({ kind: 'item', id: `copy:${name}`, label: name }) as const)
        ])
  ]
  return (
    <>
      <SidebarAction
        label="New agent"
        data-new-agent
        disabled={disabled}
        onClick={() => setAt((open) => (open === null ? (document.activeElement?.getBoundingClientRect() ?? null) : null))}
      >
        <PlusIcon width={13} height={13} />
      </SidebarAction>
      {at !== null && (
        <Menu
          label="New agent"
          entries={entries}
          at={{ below: at }}
          onDismiss={() => setAt(null)}
          onSelect={(id) => {
            setAt(null)
            if (id === 'blank') onBlank()
            else if (id.startsWith('copy:')) onCopy(id.slice('copy:'.length))
          }}
        />
      )}
    </>
  )
}
