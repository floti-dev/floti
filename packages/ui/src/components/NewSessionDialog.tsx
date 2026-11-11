import type { JSX, KeyboardEvent, ReactNode } from 'react'
import { useEffect, useId, useMemo, useRef, useState } from 'react'
import {
  EFFORT_LEVELS,
  historyTitle,
  liveSessionsIn,
  type EffortLevel,
  type HistorySession,
  type LaunchAgents,
  type LiveSession,
  type PermissionMode,
  type Profile,
  type Project
} from '@helm/core/types'
import { cn } from '../lib/cn'
import { hasPrimaryModifier, isMac, shortcutLabel } from '../lib/platform'
import {
  homePath,
  launchSentence,
  PERMISSION_CHOICES,
  profileFor,
  profilesInOrder,
  rankProjects,
  type ProjectMatch
} from '../lib/launcher'
import {
  agentOptions,
  agentValuesNote,
  MODEL_CHOICES,
  resolveLaunch,
  runnableAgent,
  worktreePlan,
  type Touched
} from '../lib/launchAgent'
import { PROJECT_KIND_ICON } from '../lib/projectIcons'
import { ROW_SELECTED } from '../lib/rows'
import { SEGMENT_ON } from '../lib/segmented'
import { formatAge, formatMoment } from '../lib/time'
import { AgentPicker } from './AgentPicker'
import { HistoryIcon, LayersIcon, TerminalIcon } from './icons'
import { LaunchSentence, RunningHere } from './LaunchNotes'
import { Overlay } from './Overlay'
import { Picker } from './Picker'

/** What was on screen when the session was asked for. */
export interface LaunchChoice {
  project: Project
  profileId: number | null
  permissionMode: PermissionMode | null
  /** The conversation to reopen, or null for a new one. */
  resume: HistorySession | null
  /** In the pane beside the focused one. */
  beside: boolean
  /** The agent to start it as, or null for none. */
  agent?: string | null
  /** The agent backend to run, or null for the host's choice. */
  backend?: string | null
  /** The model and effort shown, each null for Default: what is sent is what was shown. */
  chosen?: { model: string | null; effort: EffortLevel | null }
}

export interface NewSessionDialogProps {
  projects: readonly Project[]
  /** When each folder was last worked in, by lower-cased path: the order with nothing typed. */
  recency: ReadonlyMap<string, number>
  /** Every live session on the machine, for "2 running" and the launch warning. */
  live: readonly LiveSession[]
  profiles: readonly Profile[]
  /** For writing paths from `~`. */
  home: string | null
  /** The folder highlighted when it opens - the one in front, usually. */
  initialPath: string | null
  /** The agent picked when it opens: an agent's page's Start a session… names its own. Absent for No agent. */
  initialAgent?: string | null | undefined
  /** Conversations that can be reopened, by lower-cased folder path. Absent means not read yet. */
  resumable: ReadonlyMap<string, readonly HistorySession[]>
  /** The folders on screen, so theirs can be read before the highlight reaches them. */
  onShowing: (paths: readonly string[]) => void
  /** A launch is in flight. */
  busy: boolean
  /** Why the last launch failed. */
  error: string | null
  now: number
  onStart: (choice: LaunchChoice) => void
  /**
   * A plain terminal in the highlighted folder instead of a session (Alt+Enter),
   * beside the focused pane with Ctrl. Absent, the launcher offers none.
   */
  onTerminal?: ((project: Project, beside: boolean) => void) | undefined
  onDismiss: () => void
  /**
   * The hosts it can start on, this PC first, while more than one is
   * connected. The launcher names the one it starts on (`host`) and offers a
   * switch to the others; what it lists (`projects`, `profiles`, `recency`) is
   * that host's. Absent, it names no host: there is only this PC.
   */
  hosts?: readonly LauncherHost[] | undefined
  /** The id of the host it starts on, one of `hosts`. */
  host?: string | undefined
  /** Another host was chosen: the caller lists that host's folders and profiles instead. */
  onHostChange?: ((host: string) => void) | undefined
  /**
   * The agents of the host it starts on and its installed agent backends, for
   * the Agent and Backend pickers; null while they are being read, or when
   * they could not be, which offers No agent alone.
   */
  agents?: LaunchAgents | null | undefined
}

/** A host the launcher can start a session on. */
export interface LauncherHost {
  id: string
  name: string
  /** A host on another computer, whose name is set in mono. */
  remote: boolean
}

/**
 * Folders listed at once. A launcher is typed into: past a handful, the next
 * keystroke is quicker than the arrow keys, and a list that stays short keeps
 * the field, the folders and the footer in one glance.
 */
const SHOWN = 6
/** Conversations offered under the highlighted folder. */
const RESUMES = 3

const key = (path: string): string => path.toLowerCase()

/** One row of the list, in the order the arrow keys walk it. */
type Row =
  | { kind: 'project'; id: string; match: ProjectMatch }
  | { kind: 'resume'; id: string; project: Project; session: HistorySession }

/** Which row is highlighted, by identity, so a list that changes keeps it where it can. */
interface Cursor {
  path: string
  resume: string | null
}

/**
 * Ctrl+N: a folder, an agent, a profile, a model, an effort and a permission
 * mode, or a conversation to reopen there - and Enter.
 *
 * Starting sessions is most of what Helm is used for, so this is built for the
 * keyboard: type to narrow the folders, the arrows to move, Tab to the agent,
 * then the profile and the rest, Enter to start and Ctrl+Enter to start beside. Nothing here
 * launches on a click; a click highlights, which is what puts the launch
 * sentence for that row on screen *before* anything runs (DESIGN.md's launch
 * disclosure). A double click, or the Start button, then starts it.
 *
 * **The conversations sit under their folder**, not in a section of their own.
 * They belong to one folder, and a section at the foot of the list could only
 * be reached by arrowing through every other folder on the way - each of which
 * would take the section over as the highlight passed. Under the highlighted
 * folder they are the next rows down.
 *
 * **The profile follows the folder until it is chosen.** Each folder starts
 * with the profile most about it (`profileFor`); once somebody picks one, that
 * is the choice for whatever they move to next. The model, effort and
 * permission mode work the same way against the profile, so each shows the
 * profile's until it is changed - and what is shown is exactly what is sent.
 *
 * **An agent fills the pickers nobody touched** (`resolveLaunch`): its model
 * and effort for the backend, its permission and its profile, each still the
 * person's to change, with a line saying whose they are. The Backend picker is
 * drawn only while the host has more than one agent backend, narrowed to the
 * agent's. An agent that works in a worktree names it in the sentence, and
 * outside a git repository the sentence is the refusal and Start is off.
 */
export function NewSessionDialog({
  projects,
  recency,
  live,
  profiles,
  home,
  initialPath,
  initialAgent = null,
  resumable,
  onShowing,
  busy,
  error,
  now,
  onStart,
  onTerminal,
  onDismiss,
  hosts,
  host,
  onHostChange,
  agents = null
}: NewSessionDialogProps): JSX.Element {
  const ids = useId()
  const inputRef = useRef<HTMLInputElement>(null)
  /**
   * Where focus was, to hand it back when this closes without starting
   * anything. Read while rendering, because by the first effect the field
   * has already taken it.
   */
  const [returnTo] = useState(() => document.activeElement)
  const [query, setQuery] = useState('')
  const [cursor, setCursor] = useState<Cursor | null>(
    initialPath === null ? null : { path: initialPath, resume: null }
  )
  /** The agent picked. Not remembered: every opening starts at No agent (A17), or at the agent whose page opened it. */
  const [chosenAgent, setChosenAgent] = useState<string | null>(initialAgent)
  /** The pickers the person changed: the rest follow the folder and the agent. */
  const [touched, setTouched] = useState<Touched>({})
  const hostName = hosts?.find((each) => each.id === host)?.name ?? 'This PC'

  const dismiss = (): void => {
    if (returnTo instanceof HTMLElement && returnTo.isConnected) returnTo.focus()
    onDismiss()
  }

  const matches = useMemo(() => {
    const ranked = rankProjects(projects, query, recency)
    if (query.trim() !== '' || initialPath === null) return ranked
    // With nothing typed, the folder it was opened on leads.
    const at = ranked.findIndex((match) => key(match.project.path) === key(initialPath))
    if (at <= 0) return ranked
    return [ranked[at]!, ...ranked.slice(0, at), ...ranked.slice(at + 1)]
  }, [projects, query, recency, initialPath])
  const shown = matches.slice(0, SHOWN)

  const selected =
    shown.find((match) => cursor !== null && key(match.project.path) === key(cursor.path))?.project ??
    shown[0]?.project ??
    null
  const resumes = selected === null ? [] : (resumable.get(key(selected.path)) ?? []).slice(0, RESUMES)

  // Ids by position: a path may hold a space, and an id may not.
  const rows: Row[] = shown.flatMap((match, index): Row[] => [
    { kind: 'project', id: `${ids}-p${String(index)}`, match },
    ...(match.project === selected
      ? resumes.map(
          (session): Row => ({
            kind: 'resume',
            id: `${ids}-r-${session.sessionId}`,
            project: match.project,
            session
          })
        )
      : [])
  ])
  const highlighted =
    rows.find((row) =>
      row.kind === 'resume'
        ? cursor?.resume === row.session.sessionId
        : cursor !== null &&
          cursor.resume === null &&
          key(row.match.project.path) === key(cursor.path)
    ) ??
    rows.find((row) => row.kind === 'project' && row.match.project === selected) ??
    null

  const shownKey = shown.map((match) => match.project.path).join('\n')
  useEffect(() => {
    onShowing(shownKey === '' ? [] : shownKey.split('\n'))
  }, [shownKey, onShowing])

  // The highlighted row stays in view as the arrows move it.
  const highlightedId = highlighted?.id ?? null
  useEffect(() => {
    if (highlightedId === null) return
    document.getElementById(highlightedId)?.scrollIntoView?.({ block: 'nearest' })
  }, [highlightedId])

  // Another host's agents (A25): the agent picked stays only where that
  // host has one by its name, and the line under the pickers says when it
  // does not.
  const dropped =
    chosenAgent !== null && agents !== null && !agents.agents.some((each) => each.name === chosenAgent)
      ? chosenAgent
      : null

  const sortedProfiles = useMemo(() => profilesInOrder(profiles), [profiles])
  const agent = runnableAgent(agents, chosenAgent)
  const launch = resolveLaunch({
    agent,
    installed: agents?.backends ?? [],
    profiles,
    folderProfile: selected === null ? null : profileFor(selected.path, profiles),
    touched
  })
  const { profile, mode, model, effort } = launch
  const resume = highlighted?.kind === 'resume' ? highlighted.session : null
  const here = selected === null ? [] : liveSessionsIn(live, selected.path)
  const plan =
    selected === null || resume !== null
      ? null
      : worktreePlan(agent, selected, selected.name, homePath(selected.path, home))
  const refused = plan?.kind === 'refused' ? plan.reason : null
  const note = agent === null ? null : agentValuesNote(agent.name, launch.fromAgent)
  const options = agentOptions(agents, hostName)
  const modelChoices = model !== null && !MODEL_CHOICES.some((each) => each === model) ? [...MODEL_CHOICES, model] : MODEL_CHOICES

  const start = (beside: boolean): void => {
    if (busy || selected === null || refused !== null) return
    onStart({
      project: selected,
      profileId: profile?.id ?? null,
      permissionMode: mode,
      resume,
      beside,
      agent: agent?.name ?? null,
      backend: launch.backend,
      chosen: { model, effort }
    })
  }

  const terminal = (beside: boolean): void => {
    if (busy || selected === null || onTerminal === undefined) return
    onTerminal(selected, beside)
  }

  const move = (step: 1 | -1): void => {
    if (rows.length === 0) return
    const at = highlighted === null ? -1 : rows.indexOf(highlighted)
    const next = rows[Math.max(0, Math.min(rows.length - 1, at + step))]
    if (next === undefined) return
    setCursor(
      next.kind === 'project'
        ? { path: next.match.project.path, resume: null }
        : { path: next.project.path, resume: next.session.sessionId }
    )
  }

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>): void => {
    const target = event.target as HTMLElement
    if (event.key === 'Enter' && event.altKey && !event.shiftKey && target.tagName !== 'BUTTON') {
      event.preventDefault()
      terminal(hasPrimaryModifier(event))
      return
    }
    if (event.key === 'Enter' && !event.altKey && !event.shiftKey && target.tagName !== 'BUTTON') {
      event.preventDefault()
      start(hasPrimaryModifier(event))
      return
    }
    // The arrows belong to the field. On a picker they change its value.
    if (target !== inputRef.current) return
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      move(event.key === 'ArrowDown' ? 1 : -1)
    }
  }

  const sentence =
    selected === null || refused !== null
      ? null
      : launchSentence({
          cwd: selected.path,
          home,
          profile,
          permissionMode: mode,
          resume: resume === null ? null : historyTitle(resume),
          model,
          effort,
          agent,
          rules: 'flags',
          worktree: plan?.kind === 'made' ? plan : null
        })

  return (
    <Overlay
      aria-label="New session"
      align="top"
      className="max-w-[620px] bg-surface-raised"
      onDismiss={dismiss}
      onKeyDown={onKeyDown}
    >
      <header className="flex h-[50px] shrink-0 items-center gap-2.5 border-b border-border px-4">
        <TerminalIcon width={17} height={17} className="shrink-0 text-accent" />
        <input
          ref={inputRef}
          autoFocus
          value={query}
          onChange={(event) => {
            setQuery(event.target.value)
            setCursor(null)
          }}
          role="combobox"
          aria-label="Folder"
          aria-expanded="true"
          aria-controls={`${ids}-list`}
          aria-activedescendant={highlighted?.id}
          aria-autocomplete="list"
          spellCheck={false}
          placeholder="Type a folder's name"
          // The field is the whole header, so it carries no well of its own:
          // a box drawn inside the palette's own top edge would be two frames
          // around one thing.
          className="min-w-0 flex-1 bg-transparent text-[15px] text-fg outline-none placeholder:text-fg-subtle focus:outline-none"
        />
        {hosts !== undefined && hosts.length > 1 ? (
          <span className="flex shrink-0 items-center gap-2 text-[11.5px] text-fg-subtle">
            Start a session on
            <span
              role="radiogroup"
              aria-label="Host"
              className="flex gap-0.5 rounded-well border border-border bg-surface-sunken p-0.5"
            >
              {hosts.map((each) => (
                <button
                  key={each.id}
                  type="button"
                  role="radio"
                  aria-checked={each.id === host}
                  data-launcher-host={each.id}
                  onClick={() => {
                    if (each.id !== host) onHostChange?.(each.id)
                  }}
                  className={cn(
                    'rounded-raised px-2 py-0.5 text-[11.5px] transition-colors',
                    each.remote && 'font-mono text-[11px]',
                    each.id === host ? SEGMENT_ON : 'text-fg-muted hover:text-fg'
                  )}
                >
                  {each.name}
                </button>
              ))}
            </span>
          </span>
        ) : (
          <span className="shrink-0 text-[11.5px] text-fg-subtle">Start a session in…</span>
        )}
      </header>

      <div
        id={`${ids}-list`}
        role="listbox"
        aria-label="Folders"
        // Out of the tab order: Chromium makes a scrolling box focusable when
        // nothing in it is, and Tab is the way to the profile.
        tabIndex={-1}
        className="min-h-0 overflow-y-auto px-1.5 pt-1.5 pb-1 outline-none"
      >
        <div className={cn(CAPS, 'px-2.5 pt-2 pb-[5px]')}>Projects</div>
        {rows.map((row) =>
          row.kind === 'project' ? (
            <ProjectOption
              key={row.id}
              id={row.id}
              match={row.match}
              home={home}
              running={liveSessionsIn(live, row.match.project.path).length}
              highlighted={row === highlighted}
              onPick={() => setCursor({ path: row.match.project.path, resume: null })}
              onStart={() => start(false)}
            />
          ) : (
            <ResumeOption
              key={row.id}
              id={row.id}
              session={row.session}
              now={now}
              highlighted={row === highlighted}
              onPick={() => setCursor({ path: row.project.path, resume: row.session.sessionId })}
              onStart={() => start(false)}
            />
          )
        )}
        {rows.length === 0 && (
          <p className="px-2.5 pt-1 pb-3 text-[12px] text-fg-muted">
            {projects.length === 0
              ? 'No folders yet. Add one to scan from the sidebar.'
              : `Nothing is called “${query.trim()}”.`}
          </p>
        )}
        {matches.length > shown.length && (
          <p className="px-2.5 pt-1 pb-2 text-[11px] text-fg-subtle">
            {matches.length - shown.length} more - keep typing to narrow them
          </p>
        )}
      </div>

      <div className="flex shrink-0 flex-col gap-2.5 border-t border-border bg-surface px-4 py-3">
        <div className="flex flex-wrap items-center gap-2">
          <label htmlFor={`${ids}-agent`} className="w-[52px] text-[12px] text-fg-subtle">
            Agent
          </label>
          <AgentPicker
            id={`${ids}-agent`}
            value={agent?.name ?? null}
            options={options}
            onChange={setChosenAgent}
            className="w-[190px]"
          />
          <label htmlFor={`${ids}-profile`} className="ml-2.5 text-[12px] text-fg-subtle">
            Profile
          </label>
          <Picker
            id={`${ids}-profile`}
            icon={<LayersIcon width={13} height={13} className="text-fg-muted" />}
            value={profile === null ? '' : String(profile.id)}
            onChange={(value) => setTouched((was) => ({ ...was, profile: { id: value === '' ? null : Number(value) } }))}
            className="w-[170px]"
          >
            <option value="">No profile</option>
            {sortedProfiles.map((candidate) => (
              <option key={candidate.id} value={String(candidate.id)}>
                {candidate.name}
              </option>
            ))}
          </Picker>
          {launch.backends.length > 1 && (
            <>
              <label htmlFor={`${ids}-backend`} className="ml-2.5 text-[12px] text-fg-subtle">
                Backend
              </label>
              <Picker
                id={`${ids}-backend`}
                value={launch.backend ?? ''}
                onChange={(value) => setTouched((was) => ({ ...was, backend: { id: value } }))}
                className="w-[130px]"
              >
                {launch.backends.map((backend) => (
                  <option key={backend.id} value={backend.id}>
                    {backend.name}
                  </option>
                ))}
              </Picker>
            </>
          )}
        </div>
        {dropped !== null && (
          <p data-agent-dropped className="-mt-1 pl-[60px] text-[10.5px] leading-[1.5] text-fg-muted">
            {hostName} has no agent called {dropped}, so Agent is back to No agent.
          </p>
        )}
        <div className="flex flex-wrap items-center gap-2">
          <label htmlFor={`${ids}-model`} className="w-[52px] text-[12px] text-fg-subtle">
            Model
          </label>
          <Picker
            id={`${ids}-model`}
            value={model ?? ''}
            onChange={(value) => setTouched((was) => ({ ...was, model: { model: value === '' ? null : value } }))}
            className="w-[118px]"
          >
            <option value="">Default</option>
            {modelChoices.map((choice) => (
              <option key={choice} value={choice}>
                {choice}
              </option>
            ))}
          </Picker>
          <label htmlFor={`${ids}-effort`} className="ml-2.5 text-[12px] text-fg-subtle">
            Effort
          </label>
          <Picker
            id={`${ids}-effort`}
            value={effort ?? ''}
            onChange={(value) =>
              setTouched((was) => ({ ...was, effort: { effort: value === '' ? null : (value as EffortLevel) } }))
            }
            className="w-[110px]"
          >
            <option value="">Default</option>
            {EFFORT_LEVELS.map((level) => (
              <option key={level} value={level}>
                {level}
              </option>
            ))}
          </Picker>
          <label htmlFor={`${ids}-mode`} className="ml-2.5 text-[12px] text-fg-subtle">
            Permissions
          </label>
          <Picker
            id={`${ids}-mode`}
            value={mode ?? ''}
            onChange={(value) =>
              setTouched((was) => ({ ...was, mode: { mode: value === '' ? null : (value as PermissionMode) } }))
            }
            className="w-[128px]"
          >
            <option value="">Default</option>
            {PERMISSION_CHOICES.map((choice) => (
              <option key={choice.mode} value={choice.mode}>
                {choice.label}
              </option>
            ))}
          </Picker>
        </div>
        {note !== null && (
          <p data-agent-note className="-mt-1 pl-[60px] text-[10.5px] leading-[1.5] text-fg-subtle">
            {note}
          </p>
        )}

        {sentence !== null && <LaunchSentence parts={sentence} />}
        {refused !== null && (
          <p data-launch-refused role="alert" className="text-[11.5px] leading-[1.55] text-danger">
            {refused}
          </p>
        )}

        <RunningHere sessions={here} />

        {error !== null && (
          <p role="alert" className="text-[11.5px] leading-[1.55] text-danger">
            {error}
          </p>
        )}
      </div>

      <footer className="flex h-[34px] shrink-0 items-center gap-1 border-t border-border bg-surface px-2.5 text-[11px] text-fg-subtle">
        <KeyButton keys="↵" onClick={() => start(false)} disabled={busy || selected === null || refused !== null}>
          {busy ? 'Starting…' : 'Start'}
        </KeyButton>
        <KeyButton
          keys={shortcutLabel('↵', { separator: ' ' })}
          onClick={() => start(true)}
          disabled={busy || selected === null || refused !== null}
        >
          Start beside
        </KeyButton>
        {onTerminal !== undefined && (
          <KeyButton keys={isMac() ? '⌥↵' : 'Alt ↵'} onClick={() => terminal(false)} disabled={busy || selected === null}>
            Terminal
          </KeyButton>
        )}
        <span className="px-1.5">
          <span className="font-mono text-fg-muted">Tab</span> Agent
        </span>
        <span className="flex-1" />
        <KeyButton keys="Esc" onClick={dismiss}>
          Close
        </KeyButton>
      </footer>
    </Overlay>
  )
}

/** The caps label (DESIGN.md "Section labels"). */
const CAPS = 'text-[10px] font-semibold tracking-[.07em] text-fg-subtle uppercase'

/**
 * A row is chosen with the pointer without taking focus from the field, so the
 * arrows and Enter go on working from wherever the pointer left the highlight.
 */
const keepFocus = (event: { preventDefault: () => void }): void => event.preventDefault()

function ProjectOption({
  id,
  match,
  home,
  running,
  highlighted,
  onPick,
  onStart
}: {
  id: string
  match: ProjectMatch
  home: string | null
  running: number
  highlighted: boolean
  onPick: () => void
  onStart: () => void
}): JSX.Element {
  const { project, hit } = match
  const Icon = PROJECT_KIND_ICON[project.kind]
  return (
    <div
      id={id}
      role="option"
      aria-selected={highlighted}
      aria-label={project.name}
      title={project.path}
      onMouseDown={keepFocus}
      onClick={onPick}
      onDoubleClick={onStart}
      className={cn(
        'relative flex h-[38px] items-center gap-2.5 rounded-raised px-2.5 transition-colors',
        highlighted ? ROW_SELECTED : 'hover:bg-hover'
      )}
    >
      {highlighted && (
        <span aria-hidden className="absolute top-[9px] bottom-[9px] left-0 w-[2px] rounded-full bg-accent" />
      )}
      <Icon width={15} height={15} className="shrink-0 text-fg-muted" />
      <span className="min-w-[150px] shrink-0 truncate text-[13px] text-fg">
        {hit === null ? (
          project.name
        ) : (
          <>
            {project.name.slice(0, hit[0])}
            <span className="text-accent-text">{project.name.slice(hit[0], hit[1])}</span>
            {project.name.slice(hit[1])}
          </>
        )}
      </span>
      <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-fg-subtle">
        {homePath(project.path, home)}
      </span>
      {running > 0 && (
        <span className="shrink-0 text-[11.5px] text-accent-text">{running} running</span>
      )}
    </div>
  )
}

function ResumeOption({
  id,
  session,
  now,
  highlighted,
  onPick,
  onStart
}: {
  id: string
  session: HistorySession
  now: number
  highlighted: boolean
  onPick: () => void
  onStart: () => void
}): JSX.Element {
  const title = historyTitle(session)
  return (
    <div
      id={id}
      role="option"
      aria-selected={highlighted}
      aria-label={`Resume ${title}`}
      onMouseDown={keepFocus}
      onClick={onPick}
      onDoubleClick={onStart}
      className={cn(
        // Indented to the folder's name: these belong to the row above.
        'relative flex h-[34px] items-center gap-2.5 rounded-raised pr-2.5 pl-[35px] transition-colors',
        highlighted ? ROW_SELECTED : 'hover:bg-hover'
      )}
    >
      {highlighted && (
        <span aria-hidden className="absolute top-[8px] bottom-[8px] left-0 w-[2px] rounded-full bg-accent" />
      )}
      <HistoryIcon width={14} height={14} className="shrink-0 text-fg-subtle" />
      <span className={cn('min-w-0 flex-1 truncate text-[12.5px]', highlighted ? 'text-fg' : 'text-fg-muted')}>
        {title}
      </span>
      <span className="shrink-0 text-[11.5px] text-fg-subtle" title={formatMoment(session.lastAt)}>
        {formatAge(session.lastAt, now)}
      </span>
    </div>
  )
}

/** A hint in the key strip that is also the control it names. */
function KeyButton({
  keys,
  onClick,
  disabled = false,
  children
}: {
  keys: string
  onClick: () => void
  disabled?: boolean
  children: ReactNode
}): JSX.Element {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={cn(
        'flex h-[24px] items-center gap-1.5 rounded-raised px-1.5 transition-colors',
        'hover:bg-hover hover:text-fg disabled:cursor-default disabled:opacity-50 disabled:hover:bg-transparent'
      )}
    >
      <span className="font-mono text-fg-muted">{keys}</span>
      {children}
    </button>
  )
}
