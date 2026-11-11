import { useState, type JSX, type KeyboardEvent, type ReactNode } from 'react'
import {
  AGENT_EFFORTS,
  AGENT_PERMISSIONS,
  applyAgentDraftChange,
  readAgentDraft,
  type AgentDraftChange,
  type AgentProblem,
  type InstalledBackend
} from '@helm/core/types'
import { cn } from '../lib/cn'
import { Checkbox } from './Checkbox'
import { CloseIcon, GripIcon } from './icons'

/** The form's fields, by the field the host names in a problem. */
export const FORM_FIELDS = [
  'name',
  'description',
  'backend',
  'model',
  'effort',
  'permission',
  'profile',
  'allowed_tools',
  'disallowed_tools',
  'worktree',
  'can_start'
] as const

export const INPUT = cn(
  'h-[28px] w-full rounded-well border border-border bg-surface-sunken px-2.5 text-[12.5px]',
  'text-fg placeholder:text-fg-subtle select-text',
  'transition-colors hover:border-border-strong focus:border-accent focus:outline-none'
)
const LABEL = 'block text-[9.5px] font-semibold tracking-[.08em] text-fg-subtle uppercase'
const HINT = 'mt-[5px] block text-[10.5px] leading-snug text-fg-subtle'

export interface AgentFormProps {
  draft: string
  onDraftChange: (text: string) => void
  /** The name field: what is typed, and what it says of it. */
  name: { value: string; onChange: (name: string) => void; hint: ReactNode; warn: boolean }
  /** The host's problems with the last save, or with the file as read. */
  problems: readonly AgentProblem[]
  /** The host's agent backends, in its order. */
  backends: readonly InstalledBackend[]
  /** The host, as a backend it lacks is said to be missing on ("This PC"). */
  hostName: string
  /** The profiles the host has, by name; null when the window cannot list them, and the file's is shown unjudged. */
  profiles: readonly string[] | null
  readOnly: boolean
  /** The instructions' editor: the Files view's, which the page passes in. */
  instructions: ReactNode
  autoFocusName: boolean
}

/**
 * An agent's fields in its file's words (the design's A6): each control reads
 * its value from the draft text and writes a change back into it, so the form
 * and the Source view are two views of one draft. A field the host has a
 * problem with has its edge in danger and the host's sentence under it.
 */
export function AgentForm(props: AgentFormProps): JSX.Element {
  const { draft, onDraftChange, name, problems, backends, hostName, profiles, readOnly } = props
  const fields = readAgentDraft(draft)
  const change = (next: AgentDraftChange): void => onDraftChange(applyAgentDraftChange(draft, next))
  const problemOf = (field: string): AgentProblem[] => problems.filter((p) => p.field === field)

  const installedIds = new Set(backends.map((b) => b.id))
  const rows: Array<{ id: string; name: string; installed: boolean }> = [
    ...fields.backends.map((id) => ({
      id,
      name: backends.find((b) => b.id === id)?.name ?? id,
      installed: installedIds.has(id)
    })),
    ...backends.filter((b) => !fields.backends.includes(b.id)).map((b) => ({ ...b, installed: true }))
  ]
  const permissionKnown = fields.permission === null || AGENT_PERMISSIONS.some((p) => p.word === fields.permission)
  const profileKnown = fields.profile === null || profiles === null || profiles.includes(fields.profile)

  return (
    <div data-agent-form className="flex flex-col gap-4">
      <Field label="Name" problems={problemOf('name')} warn={name.warn} hint={name.hint}>
        <input
          data-agent-field="name"
          aria-label="Name"
          value={name.value}
          readOnly={readOnly}
          autoFocus={props.autoFocusName}
          spellCheck={false}
          onChange={(e) => name.onChange(e.target.value)}
          className={cn(INPUT, 'font-mono text-[12px]', name.warn && 'border-warn')}
        />
      </Field>
      <Field
        label="Description"
        problems={problemOf('description')}
        hint="Shown in the list and the Agent picker, and to a session choosing an agent for a child."
      >
        <input
          data-agent-field="description"
          aria-label="Description"
          value={fields.description}
          readOnly={readOnly}
          onChange={(e) => change({ field: 'description', value: e.target.value })}
          className={cn(INPUT, problemOf('description').length > 0 && 'border-danger')}
        />
      </Field>
      <Field
        label="Backends"
        problems={[...problemOf('backend'), ...problemOf('model'), ...problemOf('effort')]}
        hint="Tried in this order. With none ticked, any installed agent backend."
      >
        <BackendTable
          rows={rows}
          ticked={fields.backends}
          models={fields.models}
          efforts={fields.efforts}
          hostName={hostName}
          readOnly={readOnly}
          danger={{
            backend: problemOf('backend').length > 0,
            model: cellsMarked(problemOf('model')),
            effort: cellsMarked(problemOf('effort'))
          }}
          onTicked={(value) => change({ field: 'backends', value })}
          onModels={(value) => change({ field: 'models', value })}
          onEfforts={(value) => change({ field: 'efforts', value })}
        />
      </Field>
      <div className="grid grid-cols-2 gap-4">
        <Field
          label="Permissions"
          problems={problemOf('permission')}
          hint="Spelled as the launcher spells them. A child's is never above its parent's."
        >
          <Select
            field="permission"
            label="Permissions"
            value={fields.permission ?? ''}
            disabled={readOnly}
            danger={problemOf('permission').length > 0 || !permissionKnown}
            onChange={(value) => change({ field: 'permission', value: value === '' ? null : value })}
          >
            <option value="">Default</option>
            {AGENT_PERMISSIONS.map((p) => (
              <option key={p.word} value={p.word}>
                {p.label}
              </option>
            ))}
            {!permissionKnown && <option value={fields.permission ?? ''}>{fields.permission}</option>}
          </Select>
        </Field>
        <Field
          label="Profile"
          problems={problemOf('profile')}
          hint="Its folders and access are composed in. A profile picked when starting wins."
        >
          <Select
            field="profile"
            label="Profile"
            value={fields.profile ?? ''}
            disabled={readOnly}
            danger={problemOf('profile').length > 0 || !profileKnown}
            onChange={(value) => change({ field: 'profile', value: value === '' ? null : value })}
          >
            <option value="">No profile</option>
            {(profiles ?? []).map((p) => (
              <option key={p} value={p}>
                {p}
              </option>
            ))}
            {fields.profile !== null && !(profiles ?? []).includes(fields.profile) && (
              <option value={fields.profile}>{profileKnown ? fields.profile : `${fields.profile} (no such profile)`}</option>
            )}
          </Select>
        </Field>
      </div>
      <Field label="Allowed tools" problems={problemOf('allowed_tools')} hint="Used without asking. Written as Claude Code's permission rules.">
        <RuleWell
          field="allowed_tools"
          rules={fields.allowedTools}
          readOnly={readOnly}
          danger={problemOf('allowed_tools').length > 0}
          onChange={(value) => change({ field: 'allowedTools', value })}
        />
      </Field>
      <Field label="Disallowed tools" problems={problemOf('disallowed_tools')} hint="Never used, whatever else allows them.">
        <RuleWell
          field="disallowed_tools"
          rules={fields.disallowedTools}
          readOnly={readOnly}
          danger={problemOf('disallowed_tools').length > 0}
          onChange={(value) => change({ field: 'disallowedTools', value })}
        />
      </Field>
      <Flag
        field="worktree"
        label="Works in a worktree of its own"
        hint="A new agent/<name> branch in .floti/worktrees, cut from what the folder has checked out. Needs a git repository."
        checked={fields.worktree}
        readOnly={readOnly}
        problems={problemOf('worktree')}
        onChange={(value) => change({ field: 'worktree', value })}
      />
      <Flag
        field="can_start"
        label="Can start agents of its own"
        hint="Gets the floti-agents tools and floti agent, while Sessions can start agents is on in Settings."
        checked={fields.canStart}
        readOnly={readOnly}
        problems={problemOf('can_start')}
        onChange={(value) => change({ field: 'canStart', value })}
      />
      <div>
        <span className={LABEL}>Instructions</span>
        <span className={cn(HINT, 'mb-1.5')}>
          Composed into the session's instructions after the project's. Never typed into its terminal.
        </span>
        {props.instructions}
      </div>
    </div>
  )
}

function Field({
  label,
  hint,
  problems,
  warn = false,
  children
}: {
  label: string
  hint?: ReactNode
  problems: readonly AgentProblem[]
  warn?: boolean
  children: ReactNode
}): JSX.Element {
  return (
    <div data-agent-field-box={label} data-problem={problems.length > 0 || undefined}>
      <span className={cn(LABEL, 'mb-1.5')}>{label}</span>
      {children}
      {problems.map((p) => (
        <span key={`${p.line ?? ''}${p.message}`} data-field-problem className="mt-[5px] block text-[10.5px] text-danger">
          {p.message}
        </span>
      ))}
      {hint !== undefined && <span className={cn(HINT, warn && 'text-warn')}>{hint}</span>}
    </div>
  )
}

function Select({
  field,
  label,
  value,
  onChange,
  disabled,
  danger,
  children
}: {
  field: string
  label: string
  value: string
  onChange: (value: string) => void
  disabled: boolean
  danger: boolean
  children: ReactNode
}): JSX.Element {
  return (
    <select
      data-agent-field={field}
      aria-label={label}
      value={value}
      disabled={disabled}
      onChange={(e) => onChange(e.target.value)}
      className={cn(INPUT, 'pr-6', danger && 'border-danger')}
    >
      {children}
    </select>
  )
}

function Flag({
  field,
  label,
  hint,
  checked,
  readOnly,
  problems,
  onChange
}: {
  field: string
  label: string
  hint: string
  checked: boolean
  readOnly: boolean
  problems: readonly AgentProblem[]
  onChange: (value: boolean) => void
}): JSX.Element {
  return (
    <label data-agent-field={field} className="flex items-start gap-[9px]">
      <Checkbox checked={checked} onChange={() => !readOnly && onChange(!checked)} label={label} />
      <span className="min-w-0">
        <span className="block text-[12.5px] text-fg">{label}</span>
        <span className="block text-[10.5px] leading-snug text-fg-subtle">{hint}</span>
        {problems.map((p) => (
          <span key={p.message} data-field-problem className="block text-[10.5px] text-danger">
            {p.message}
          </span>
        ))}
      </span>
    </label>
  )
}

/**
 * Which backends' cells a field's problems mark: the one a problem names, or
 * every one for a problem with the whole field (a model that is not a map).
 */
function cellsMarked(problems: readonly AgentProblem[]): (backend: string) => boolean {
  if (problems.some((p) => p.backend === undefined)) return () => true
  const named = new Set(problems.map((p) => p.backend))
  return (backend) => named.has(backend)
}

interface BackendRow {
  id: string
  name: string
  installed: boolean
}

function BackendTable({
  rows,
  ticked,
  models,
  efforts,
  hostName,
  readOnly,
  danger,
  onTicked,
  onModels,
  onEfforts
}: {
  rows: BackendRow[]
  ticked: string[]
  models: Record<string, string>
  efforts: Record<string, string>
  hostName: string
  readOnly: boolean
  /** Whether the table's edge is marked, and which backends' model and effort cells are. */
  danger: { backend: boolean; model: (backend: string) => boolean; effort: (backend: string) => boolean }
  onTicked: (ids: string[]) => void
  onModels: (models: Record<string, string>) => void
  onEfforts: (efforts: Record<string, string>) => void
}): JSX.Element {
  const [dragging, setDragging] = useState<string | null>(null)
  /** `id` moved to where `target` is, among the ticked rows. */
  const move = (id: string, target: string): void => {
    if (id === target || !ticked.includes(id) || !ticked.includes(target)) return
    const without = ticked.filter((each) => each !== id)
    without.splice(without.indexOf(target) + (ticked.indexOf(id) < ticked.indexOf(target) ? 1 : 0), 0, id)
    onTicked(without)
  }
  const nudge = (event: KeyboardEvent, id: string): void => {
    const at = ticked.indexOf(id)
    if (at < 0) return
    const target = event.key === 'ArrowUp' ? ticked[at - 1] : event.key === 'ArrowDown' ? ticked[at + 1] : undefined
    if (target === undefined) return
    event.preventDefault()
    move(id, target)
  }
  const efforts_ = (id: string): string => efforts[id] ?? ''
  return (
    <div
      data-agent-backends
      className={cn('overflow-hidden rounded-well border border-border', danger.backend && 'border-danger')}
    >
      <div className="grid grid-cols-[18px_18px_minmax(0,1fr)_140px_120px] items-center gap-2 border-b border-border bg-surface-sunken px-2 py-1 text-[9.5px] font-semibold tracking-[.08em] text-fg-subtle uppercase">
        <span />
        <span />
        <span>Backend</span>
        <span>Model</span>
        <span>Effort</span>
      </div>
      {rows.map((row) => {
        const on = ticked.includes(row.id)
        const effort = efforts_(row.id)
        const effortKnown = effort === '' || AGENT_EFFORTS.includes(effort)
        return (
          <div
            key={row.id}
            data-agent-backend={row.id}
            onDragOver={(e) => dragging !== null && on && e.preventDefault()}
            onDrop={() => dragging !== null && move(dragging, row.id)}
            className={cn(
              'grid grid-cols-[18px_18px_minmax(0,1fr)_140px_120px] items-center gap-2 border-b border-border px-2 py-1.5 last:border-b-0',
              !row.installed && 'opacity-60',
              dragging === row.id && 'opacity-40'
            )}
          >
            <button
              type="button"
              aria-label={`Move ${row.name}`}
              draggable={on && !readOnly}
              disabled={!on || readOnly}
              onDragStart={() => setDragging(row.id)}
              onDragEnd={() => setDragging(null)}
              onKeyDown={(e) => nudge(e, row.id)}
              className="grid size-[18px] cursor-grab place-items-center text-fg-subtle disabled:cursor-default disabled:opacity-40"
            >
              <GripIcon width={10} height={10} />
            </button>
            <Checkbox
              checked={on}
              label={`Run on ${row.name}`}
              onChange={() => !readOnly && onTicked(on ? ticked.filter((id) => id !== row.id) : [...ticked, row.id])}
            />
            <span className="flex min-w-0 items-baseline gap-2">
              <span className="truncate text-[12px] text-fg">{row.name}</span>
              <span className="shrink-0 font-mono text-[10px] text-fg-subtle">{row.id}</span>
              {!row.installed && <span className="truncate text-[10.5px] text-fg-subtle">not installed on {hostName}</span>}
            </span>
            <input
              data-agent-model={row.id}
              aria-label={`Model for ${row.name}`}
              placeholder="Default"
              value={models[row.id] ?? ''}
              readOnly={readOnly}
              spellCheck={false}
              onChange={(e) => onModels({ ...models, [row.id]: e.target.value.trim() })}
              className={cn(INPUT, 'h-[24px] font-mono text-[11px]', danger.model(row.id) && 'border-danger')}
            />
            <select
              data-agent-effort={row.id}
              aria-label={`Effort for ${row.name}`}
              value={effort}
              disabled={readOnly}
              onChange={(e) => onEfforts({ ...efforts, [row.id]: e.target.value })}
              className={cn(INPUT, 'h-[24px] text-[11.5px]', (danger.effort(row.id) || !effortKnown) && 'border-danger')}
            >
              <option value="">Default</option>
              {AGENT_EFFORTS.map((level) => (
                <option key={level} value={level}>
                  {level}
                </option>
              ))}
              {!effortKnown && <option value={effort}>{effort}</option>}
            </select>
          </div>
        )
      })}
    </div>
  )
}

function RuleWell({
  field,
  rules,
  readOnly,
  danger,
  onChange
}: {
  field: string
  rules: string[]
  readOnly: boolean
  danger: boolean
  onChange: (rules: string[]) => void
}): JSX.Element {
  const [typed, setTyped] = useState('')
  const add = (): void => {
    const rule = typed.trim()
    if (rule === '') return
    if (!rules.includes(rule)) onChange([...rules, rule])
    setTyped('')
  }
  return (
    <div
      data-agent-field={field}
      className={cn(
        'flex min-h-[28px] flex-wrap items-center gap-1 rounded-well border border-border bg-surface-sunken px-1.5 py-1',
        'focus-within:border-accent',
        danger && 'border-danger'
      )}
    >
      {rules.map((rule) => (
        <span
          key={rule}
          data-rule={rule}
          className="flex items-center gap-1 rounded-raised border border-border bg-surface px-1.5 py-px font-mono text-[11px] text-fg"
        >
          {rule}
          {!readOnly && (
            <button
              type="button"
              aria-label={`Remove ${rule}`}
              onClick={() => onChange(rules.filter((each) => each !== rule))}
              className="text-fg-subtle hover:text-fg"
            >
              <CloseIcon width={8} height={8} />
            </button>
          )}
        </span>
      ))}
      {!readOnly && (
        <input
          aria-label={`Add a rule to ${field === 'allowed_tools' ? 'Allowed tools' : 'Disallowed tools'}`}
          placeholder="Add a rule"
          value={typed}
          spellCheck={false}
          onChange={(e) => setTyped(e.target.value)}
          onBlur={add}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault()
              add()
            } else if (e.key === 'Backspace' && typed === '' && rules.length > 0) {
              onChange(rules.slice(0, -1))
            }
          }}
          className="min-w-[90px] flex-1 bg-transparent px-1 font-mono text-[11px] text-fg placeholder:font-sans placeholder:text-fg-subtle focus:outline-none"
        />
      )}
    </div>
  )
}
