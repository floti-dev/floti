import type { JSX } from 'react'
import type { AgentOption } from '../lib/launchAgent'
import { AgentCardIcon } from './icons'
import { Picker } from './Picker'

/**
 * The Agent picker (A17): No agent first, then the host's agents, each with
 * its description under its name. One no backend here can run is listed
 * disabled with the reason; a file that cannot be loaded is set apart under a
 * rule, disabled, with its problem in danger. It is the select's own list,
 * drawn by CSS as every select is (DESIGN 4), so the keyboard and type-ahead
 * are the platform's and a driver can set it.
 *
 * An option's text is the agent's name alone, which is what the closed control
 * shows; the second line is its `data-detail`, drawn under the name in the
 * open list by `theme.css` (`option[data-detail]::after`).
 */
export function AgentPicker({
  id,
  value,
  options,
  onChange,
  className
}: {
  id: string
  /** The agent picked, or null for No agent. */
  value: string | null
  options: readonly AgentOption[]
  onChange: (agent: string | null) => void
  className?: string
}): JSX.Element {
  const agents = options.filter((option) => !option.bad)
  const bad = options.filter((option) => option.bad)
  return (
    <Picker
      id={id}
      icon={<AgentCardIcon width={13} height={13} className="text-fg-muted" />}
      value={value ?? ''}
      onChange={(picked) => onChange(picked === '' ? null : picked)}
      {...(className === undefined ? {} : { className })}
    >
      <option value="" data-detail="A plain session, as now.">
        No agent
      </option>
      {agents.map((option) => (
        <option
          key={option.name}
          value={option.name}
          disabled={option.disabled}
          title={option.detail}
          data-agent-option={option.name}
          data-detail={option.detail}
        >
          {option.name}
        </option>
      ))}
      {bad.length > 0 && <hr />}
      {bad.map((option) => (
        <option
          key={`bad-${option.name}`}
          value={`bad:${option.name}`}
          disabled
          title={option.detail}
          data-agent-option={option.name}
          data-agent-bad
          data-detail={option.detail}
        >
          {option.name}
        </option>
      ))}
    </Picker>
  )
}
