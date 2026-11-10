import type { JSX } from 'react'
import type { AgentOption } from '../lib/launchAgent'
import { AgentCardIcon } from './icons'
import { Picker } from './Picker'

/** Two lines to an option: the name, and what it is or why it cannot be picked. */
const OPTION = 'flex-col !items-start justify-center gap-px py-[5px]'
const DETAIL = 'max-w-[300px] truncate text-[10.5px] leading-[1.35] text-fg-subtle'

/**
 * The Agent picker (A17): No agent first, then the host's agents, each with
 * its description under its name. One no backend here can run is listed
 * disabled with the reason; a file that cannot be loaded is set apart under a
 * rule, disabled, with its problem in danger. It is the select's own list,
 * drawn by CSS as every select is (DESIGN 4), so the keyboard and type-ahead
 * are the platform's and a driver can set it; the closed control shows the
 * name alone (`label`).
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
      <option value="" label="No agent" className={OPTION}>
        <span>No agent</span>
        <span className={DETAIL}>A plain session, as now.</span>
      </option>
      {agents.map((option) => (
        <option
          key={option.name}
          value={option.name}
          label={option.name}
          disabled={option.disabled}
          title={option.detail}
          data-agent-option={option.name}
          className={OPTION}
        >
          <span>{option.name}</span>
          <span className={DETAIL}>{option.detail}</span>
        </option>
      ))}
      {bad.length > 0 && <hr />}
      {bad.map((option) => (
        <option
          key={`bad-${option.name}`}
          value={`bad:${option.name}`}
          label={option.name}
          disabled
          title={option.detail}
          data-agent-option={option.name}
          data-agent-bad
          className={OPTION}
        >
          <span>{option.name}</span>
          <span className={`${DETAIL} !text-danger`}>{option.detail}</span>
        </option>
      ))}
    </Picker>
  )
}
