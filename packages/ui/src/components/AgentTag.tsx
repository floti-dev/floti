import type { JSX } from 'react'
import { cn } from '../lib/cn'

/**
 * The 16px pill a crumb's tag is drawn as (DESIGN 5, "The crumb row"): 10px
 * Inter at 500 with 7px of padding. A ring is drawn inside it, as an inset
 * shadow, so a ringed pill takes no more room than a tinted one.
 */
export const TAG_PILL = 'inline-block max-w-[160px] shrink-0 truncate rounded-full px-[7px] align-middle font-sans text-[10px] leading-4 font-medium'

/** The tag's tint: accent text on accent-soft. */
export const TAG_TINT = 'bg-accent-soft text-accent-text'

/** A tag with no tint: muted text on a hairline ring (a command child's, a gone agent's). */
export const TAG_RING = 'text-fg-muted ring-1 ring-border-strong ring-inset'

export interface AgentTagProps {
  /** The agent the session was started as (`lead`, `research-lead`). */
  name: string
  /**
   * The agent is no longer among its host's agents: the tag loses its tint and
   * keeps a hairline ring, so it still says what the session was started as.
   */
  gone?: boolean | undefined
  className?: string | undefined
  /** The crumb names its tag for the drivers (`data-crumb="tag"`). */
  'data-crumb'?: string | undefined
  'data-crumb-tag'?: string | undefined
}

/**
 * The agent tag: marks a session started as an agent everywhere it is named,
 * after its name (on its tab, its row in the Sessions tree, the sessions pane
 * and History) and leading its crumb. The session is named after its folder,
 * as any session is; the tag says what it was started as.
 */
export function AgentTag({ name, gone = false, className, ...data }: AgentTagProps): JSX.Element {
  return (
    <span
      data-agent-tag={name}
      data-agent-gone={gone ? 'true' : undefined}
      className={cn(TAG_PILL, gone ? TAG_RING : TAG_TINT, className)}
      {...data}
    >
      {name}
    </span>
  )
}
