import type { JSX, KeyboardEvent, ReactNode } from 'react'
import { Fragment, useEffect, useId, useMemo, useRef, useState } from 'react'
import type { HistoryMatchLine, HistorySession, LaunchAgents, RestorableSession } from '@helm/core'
import { HISTORY_NAME_MAX } from '@helm/core/types'
import { cn } from '../lib/cn'
import {
  agentGone,
  clock,
  endedWord,
  historySpan,
  historyWhen,
  launchSentence,
  marks,
  paletteName,
  rowWord,
  snippet,
  unresumable,
  unresumableSentence,
  type HistoryRunning
} from '../lib/historyPalette'
import { hasPrimaryModifier, isMac } from '../lib/platform'
import { AgentTag } from './AgentTag'
import { HostSwitch, type HostChoice } from './HostSwitch'
import { KeyButton } from './KeyButton'
import { Overlay } from './Overlay'
import { HistoryIcon, ResumeIcon } from './icons'

/**
 * History, on Ctrl+H: a palette over the window wherever the user is, not a
 * page (DESIGN.md "History"). With nothing typed it lists the sessions worked
 * in last; typed into, it is two counted lists, Titles and prompts and
 * Conversations, with Tab between them. Enter resumes the highlighted session
 * where it ran, or shows one that runs now; Ctrl+Enter opens a conversation to
 * read in a tab, at the line that matched. Every read is the window's: this
 * component draws what it is given and says what each key would do.
 */

/** A list the palette was given: the sessions, or the sentence its read failed with; null while it is read. */
export type HistoryAnswer = { sessions: readonly HistorySession[]; total: number } | { error: string } | null

/** Which typed list is shown. */
export type HistoryListKind = 'titles' | 'conversations'

/** The sessions a stop took, kept at the top of the palette until each is back (H8). */
export interface StoppedGroup {
  /** When the host stopped, epoch ms, or null when not known. */
  at: number | null
  sessions: readonly RestorableSession[]
}

export interface HistoryPaletteProps {
  query: string
  onQueryChange: (query: string) => void
  /** The sessions worked in last, with nothing typed. */
  recent: HistoryAnswer
  /** How many sessions History holds, for "Type to search all 806"; null when not known. */
  total: number | null
  /** The sentence the read of how many it holds failed with; null when it did not fail. */
  totalError?: string | null | undefined
  /** The sentence the read of the recorded folders failed with: a folder's name in a query then narrows nothing. */
  foldersError?: string | null | undefined
  /** The history file could not be read: its path and the system's sentence. */
  readError: { file: string; message: string } | null
  /** Typed: the sessions whose names, titles, prompts or folder match. */
  titles: HistoryAnswer
  /** Typed: the archived conversations that mention it, each with its matching lines. */
  conversations: HistoryAnswer
  /** Each parent's children, by its session id, as they have been read; absent until asked for. */
  children: ReadonlyMap<string, HistoryAnswer>
  onUnfold: (parent: HistorySession) => void
  /** Sessions running now on the shown host, by lower-cased conversation id. */
  running: ReadonlyMap<string, HistoryRunning>
  /** The host's agents, to say an agent is gone; null when not known. */
  agents: Pick<LaunchAgents, 'agents' | 'bad'> | null
  /** A profile's name by its id, or null for one not known. */
  profileName: (id: number) => string | null
  stopped: StoppedGroup | null
  onResumeStopped: (sessions: readonly RestorableSession[]) => void
  onDismissStopped: () => void
  onResume: (session: HistorySession, options: { withoutAgent: boolean }) => void
  /** Shows a session that runs now: its tab here, or a new one. */
  onShow: (session: HistorySession) => void
  /** Opens the whole conversation read-only in a tab, at `at` when given. */
  onRead: (session: HistorySession, at: HistoryMatchLine | null) => void
  onRename: (session: HistorySession, name: string | null) => void
  /** Reads again after a read failed. */
  onRetry: () => void
  onDismiss: () => void
  /** The host switch, shown while more than one host is connected (H15). */
  hosts?: Partial<HostChoice> | undefined
  /** The shown host's name, for the sentences, when more than one is connected. */
  hostName?: string | undefined
  /**
   * A resume in flight: a session's id, or any other key for the Stopped
   * group's. While it is set nothing is resumed, shown or read, so a second
   * Enter or click never starts a second process.
   */
  resuming?: string | null | undefined
  resumeError?: string | null | undefined
}

type Item =
  | { kind: 'stopped-all'; key: string; sessions: readonly RestorableSession[] }
  | { kind: 'stopped'; key: string; lost: RestorableSession }
  | { kind: 'session'; key: string; session: HistorySession }
  | { kind: 'child'; key: string; session: HistorySession; parent: HistorySession; last: boolean }
  | { kind: 'line'; key: string; session: HistorySession; line: HistoryMatchLine }

type Shown =
  | { kind: 'section'; key: string; label: string; count?: number | undefined; dismiss?: boolean }
  | { kind: 'more'; key: string; text: string }
  | { kind: 'note'; key: string; text: string }
  | { kind: 'item'; key: string; item: Item; index: number }

const RECENT = 10

const CTRL = isMac() ? '⌘' : 'Ctrl'

export function HistoryPalette(props: HistoryPaletteProps): JSX.Element {
  const { query, recent, titles, conversations, children, running, agents, stopped } = props
  const ids = useId()
  const typed = query.trim() !== ''
  /** The list Tab or a click chose, for the query it was chosen in. */
  const [chosen, setChosen] = useState<{ query: string; list: HistoryListKind } | null>(null)
  /**
   * The highlighted item by its key, or null for where the list starts
   * (`start`): a list read again keeps it on the same session, wherever that
   * session now is.
   */
  const [cursorKey, setCursorKey] = useState<string | null>(null)
  const [open, setOpen] = useState<ReadonlySet<string>>(new Set())
  /** The session being renamed, the draft, and the name the draft started from. */
  const [renaming, setRenaming] = useState<{ session: HistorySession; draft: string; seed: string } | null>(null)
  const listRef = useRef<HTMLDivElement>(null)
  const fieldRef = useRef<HTMLInputElement>(null)

  // A query opens on whichever list has matches, Titles and prompts first,
  // until Tab or a click chooses the other for it.
  const list: HistoryListKind = chosen !== null && chosen.query === query ? chosen.list : opening(titles, conversations)
  const setList = (next: HistoryListKind): void => setChosen({ query, list: next })

  const shown = typed ? (list === 'titles' ? titles : conversations) : recent

  const rows = useMemo(() => {
    const out: Shown[] = []
    const items: Item[] = []
    const push = (item: Item): void => {
      out.push({ kind: 'item', key: item.key, item, index: items.length })
      items.push(item)
    }
    if (!typed && stopped !== null && stopped.sessions.length > 0) {
      const back = stopped.sessions.filter(
        (lost) => lost.blocked === null && lost.agentProblem === undefined && lost.conversationId !== undefined
      )
      out.push({
        kind: 'section',
        key: 'stopped',
        label: stopped.at === null ? 'Stopped' : `Stopped at ${clock(stopped.at)}`,
        count: stopped.sessions.length,
        dismiss: true
      })
      if (back.length > 0) push({ kind: 'stopped-all', key: 'stopped-all', sessions: back })
      for (const lost of stopped.sessions) push({ kind: 'stopped', key: `stopped-${String(lost.id)}`, lost })
      out.push({ kind: 'section', key: 'recent', label: 'Recent' })
    } else if (!typed) {
      out.push({ kind: 'section', key: 'recent', label: 'Recent' })
    }
    if (shown !== null && !('error' in shown)) {
      const sessions = typed ? shown.sessions : shown.sessions.slice(0, RECENT)
      for (const session of sessions) {
        push({ kind: 'session', key: session.sessionId, session })
        if (typed && list === 'conversations') {
          for (const line of session.matches ?? []) {
            push({ kind: 'line', key: `${session.sessionId}-${line.uuid}`, session, line })
          }
          const more = (session.matchCount ?? 0) - (session.matches?.length ?? 0)
          if (more > 0) {
            out.push({ kind: 'more', key: `${session.sessionId}-more`, text: `and ${String(more)} more in this conversation` })
          }
        }
        if (open.has(session.sessionId)) {
          const kids = children.get(session.sessionId)
          if (kids === undefined || kids === null) {
            out.push({ kind: 'note', key: `${session.sessionId}-reading`, text: 'Reading its children…' })
          } else if ('error' in kids) {
            out.push({ kind: 'note', key: `${session.sessionId}-error`, text: `Could not read its children: ${kids.error}` })
          } else {
            kids.sessions.forEach((child, at) =>
              push({
                kind: 'child',
                key: `${session.sessionId}>${child.sessionId}`,
                session: child,
                parent: session,
                last: at === kids.sessions.length - 1
              })
            )
          }
        }
      }
    }
    return { out, items }
  }, [typed, stopped, shown, list, open, children])

  const { items } = rows
  // Conversations start on the first line that matched, as Ctrl+Shift+F
  // does; every other list on its first row.
  const start = typed && list === 'conversations' ? Math.max(0, items.findIndex((item) => item.kind === 'line')) : 0
  const keyed = cursorKey === null ? -1 : items.findIndex((item) => item.key === cursorKey)
  const at = Math.min(keyed >= 0 ? keyed : start, Math.max(0, items.length - 1))
  const highlighted = items[at]
  /** Moves the cursor to the item at `index`; nothing moves it while a name is typed. */
  const setCursor = (index: number | null): void => {
    if (renaming !== null) return
    setCursorKey(index === null ? null : (items[index]?.key ?? null))
  }
  const busy = props.resuming !== null && props.resuming !== undefined

  useEffect(() => {
    listRef.current?.querySelector(`[data-index="${String(at)}"]`)?.scrollIntoView({ block: 'nearest' })
  }, [at])

  const isRunning = (session: HistorySession): HistoryRunning | null =>
    running.get(session.sessionId.toLowerCase()) ?? null
  const gone = (session: HistorySession): boolean => agentGone(session.agent, agents)

  const sessionOf = (item: Item | undefined): HistorySession | null =>
    item === undefined || item.kind === 'stopped-all' || item.kind === 'stopped' ? null : item.session

  const activate = (item: Item | undefined): void => {
    if (item === undefined || busy) return
    if (item.kind === 'stopped-all') return props.onResumeStopped(item.sessions)
    if (item.kind === 'stopped') return props.onResumeStopped([item.lost])
    const session = item.session
    if (isRunning(session) !== null) return props.onShow(session)
    if (unresumable(session) !== null) return props.onRead(session, item.kind === 'line' ? item.line : null)
    props.onResume(session, { withoutAgent: gone(session) })
  }

  const read = (item: Item | undefined): void => {
    const session = sessionOf(item)
    // Floti renders no part of a live session (H12).
    if (item === undefined || busy || session === null || isRunning(session) !== null) return
    props.onRead(session, item.kind === 'line' ? item.line : null)
  }

  const unfold = (session: HistorySession): void => {
    if ((session.children ?? 0) === 0 || open.has(session.sessionId)) return
    setOpen(new Set([...open, session.sessionId]))
    // Read once, and again after a read that failed.
    const kids = children.get(session.sessionId)
    if (kids === undefined || (kids !== null && 'error' in kids)) props.onUnfold(session)
  }

  const fold = (item: Item | undefined): void => {
    if (item === undefined) return
    const parent = item.kind === 'child' ? item.parent : item.kind === 'session' ? item.session : null
    if (parent === null || !open.has(parent.sessionId)) return
    const next = new Set(open)
    next.delete(parent.sessionId)
    setOpen(next)
    const index = items.findIndex((each) => each.kind === 'session' && each.session.sessionId === parent.sessionId)
    if (index >= 0) setCursor(index)
  }

  const startRename = (): void => {
    const session = sessionOf(highlighted)
    if (session === null || highlighted?.kind === 'line') return
    const seed = renameSeed(session)
    setRenaming({ session, draft: seed, seed })
  }

  /** Keeps the draft for the session it was typed for, wherever the list has moved; a draft left as it started saves nothing. */
  const finishRename = (keep: boolean): void => {
    if (keep && renaming !== null && renaming.draft.trim() !== renaming.seed.trim()) {
      const name = renaming.draft.trim()
      props.onRename(renaming.session, name === '' ? null : name)
    }
    setRenaming(null)
    fieldRef.current?.focus()
  }

  // What stands in for the list: a read that failed, or nothing yet.
  const blocked = blockedState(props, typed, list, shown)
  const session = sessionOf(highlighted)

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>): void => {
    if (renaming !== null) return
    const primary = hasPrimaryModifier(event)
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      if (items.length === 0) return
      setCursor((at + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length)
      return
    }
    if (event.key === 'Enter') {
      event.preventDefault()
      if (blocked !== null) return retries(blocked) ? props.onRetry() : undefined
      // Enter with the other platform's modifier (Ctrl on a Mac, the Windows
      // key elsewhere) is neither Enter nor Read, so it does nothing.
      if (!primary && (event.ctrlKey || event.metaKey)) return
      if (primary) read(highlighted)
      else activate(highlighted)
      return
    }
    if (event.key === 'Tab' && typed) {
      event.preventDefault()
      setList(list === 'titles' ? 'conversations' : 'titles')
      setCursor(null)
      return
    }
    if (event.key === 'F2') {
      event.preventDefault()
      startRename()
      return
    }
    const caretAtEnd =
      fieldRef.current === null || fieldRef.current.selectionStart === fieldRef.current.value.length
    if (event.key === 'ArrowRight' && caretAtEnd) {
      const session = highlighted?.kind === 'session' ? highlighted.session : null
      if (session !== null && (session.children ?? 0) > 0 && !open.has(session.sessionId)) {
        event.preventDefault()
        unfold(session)
      }
      return
    }
    if (event.key === 'ArrowLeft' && caretAtEnd && highlighted !== undefined) {
      const parent = highlighted.kind === 'child' ? highlighted.parent : highlighted.kind === 'session' ? highlighted.session : null
      if (parent !== null && open.has(parent.sessionId)) {
        event.preventDefault()
        fold(highlighted)
      }
    }
  }

  return (
    <Overlay
      aria-label="History"
      data-history-palette
      align="top"
      className="max-w-[680px] bg-surface-raised"
      onDismiss={() => {
        if (renaming !== null) return
        props.onDismiss()
      }}
      onKeyDown={onKeyDown}
    >
      <header className="flex h-[44px] shrink-0 items-center gap-[9px] border-b border-border pr-2.5 pl-[13px]">
        <HistoryIcon width={15} height={15} className="shrink-0 text-accent" />
        <input
          ref={fieldRef}
          autoFocus
          value={query}
          onChange={(event) => {
            props.onQueryChange(event.target.value)
            setCursor(null)
          }}
          role="combobox"
          aria-label="Search History"
          aria-expanded="true"
          aria-controls={`${ids}-list`}
          aria-activedescendant={highlighted === undefined ? undefined : `${ids}-${String(at)}`}
          aria-autocomplete="list"
          spellCheck={false}
          placeholder="Search sessions by title, prompt, folder or conversation"
          className="min-w-0 flex-1 bg-transparent text-[13.5px] text-fg outline-none placeholder:text-fg-subtle focus:outline-none"
        />
        {typed && (
          <ListSwitch
            list={list}
            titles={titles}
            conversations={conversations}
            onList={(next) => {
              setList(next)
              setCursor(null)
              fieldRef.current?.focus()
            }}
          />
        )}
      </header>

      {typed && props.foldersError !== null && props.foldersError !== undefined && (
        <p data-history-folders-error className="px-[13px] pt-1.5 text-[11px] text-warn">
          Folders could not be read: {props.foldersError}. A folder’s name is searched for as a word.
        </p>
      )}

      {blocked !== null ? (
        <BlockedBody state={blocked} />
      ) : (
        <>
          <div
            ref={listRef}
            id={`${ids}-list`}
            role="listbox"
            aria-label={typed ? (list === 'titles' ? 'Titles and prompts' : 'Conversations') : 'Recent sessions'}
            tabIndex={-1}
            className={cn('max-h-[420px] min-h-0 overflow-y-auto px-1 pb-1 outline-none', typed ? 'pt-1.5' : 'pt-0.5')}
          >
            {rows.out.map((row) => {
              if (row.kind === 'section') {
                return (
                  <div
                    key={row.key}
                    data-history-section={row.key}
                    className="flex h-[26px] items-center gap-1.5 px-[9px] pt-1 text-[10px] font-semibold tracking-[.07em] text-fg-subtle uppercase"
                  >
                    {row.label}
                    {row.count !== undefined && <span className="font-medium tracking-normal tabular-nums">{row.count}</span>}
                    {row.dismiss === true && (
                      <button
                        type="button"
                        data-history-dismiss-stopped
                        onMouseDown={keepFocus}
                        onClick={props.onDismissStopped}
                        className="ml-auto rounded-raised px-[7px] py-px text-[11px] font-normal tracking-normal text-fg-muted normal-case hover:bg-hover hover:text-fg"
                      >
                        Dismiss
                      </button>
                    )}
                  </div>
                )
              }
              if (row.kind === 'more') {
                return (
                  <div key={row.key} className="pt-0.5 pr-2.5 pb-1.5 pl-[90px] text-[11px] text-fg-subtle">
                    {row.text}
                  </div>
                )
              }
              if (row.kind === 'note') {
                return (
                  <div key={row.key} className="h-[30px] pt-[7px] pl-[46px] text-[11.5px] text-fg-muted">
                    {row.text}
                  </div>
                )
              }
              const chosen = row.index === at
              const common = {
                id: `${ids}-${String(row.index)}`,
                'data-index': row.index,
                role: 'option' as const,
                'aria-selected': chosen,
                onMouseDown: keepFocus,
                onMouseMove: chosen ? undefined : () => setCursor(row.index),
                onClick: () => setCursor(row.index),
                onDoubleClick: () => activate(row.item)
              }
              return (
                <ItemRow
                  key={row.key}
                  item={row.item}
                  chosen={chosen}
                  common={common}
                  query={typed ? query : ''}
                  running={sessionOf(row.item) === null ? null : isRunning(sessionOf(row.item) as HistorySession)}
                  gone={sessionOf(row.item) === null ? false : gone(sessionOf(row.item) as HistorySession)}
                  unfolded={row.item.kind === 'session' && open.has(row.item.session.sessionId)}
                  onToggle={(parent) => (open.has(parent.sessionId) ? fold(row.item) : unfold(parent))}
                  renaming={renaming !== null && renamed(row.item, renaming.session) ? renaming.draft : null}
                  onDraft={(draft) => setRenaming((current) => (current === null ? null : { ...current, draft }))}
                  onFinishRename={finishRename}
                />
              )
            })}
          </div>
          <Foot {...props} typed={typed} list={list} />
          {highlighted !== undefined && (
            <Preview
              item={highlighted}
              session={session}
              props={props}
              running={session === null ? null : isRunning(session)}
              gone={session === null ? false : gone(session)}
              renaming={renaming !== null}
            />
          )}
        </>
      )}

      <footer
        data-history-keys
        className="flex h-[32px] shrink-0 items-center gap-1 overflow-hidden border-t border-border bg-surface px-2 text-[11px] whitespace-nowrap text-fg-subtle"
      >
        <HostSwitch surface="History" {...props.hosts} className="mr-1" />
        <Keys
          blocked={blocked === null ? null : retries(blocked) ? 'retry' : 'still'}
          item={highlighted}
          session={session}
          running={session === null ? null : isRunning(session)}
          renaming={renaming !== null}
          typed={typed}
          list={list}
          onEnter={() => (blocked !== null ? props.onRetry() : activate(highlighted))}
          onRead={() => read(highlighted)}
          onRename={startRename}
          onKeep={() => finishRename(true)}
          onLeave={() => finishRename(false)}
          onFold={() => (highlighted?.kind === 'session' && !open.has(highlighted.session.sessionId) ? unfold(highlighted.session) : fold(highlighted))}
          unfolded={highlighted?.kind === 'child' || (highlighted?.kind === 'session' && open.has(highlighted.session.sessionId))}
          onTab={() => {
            setList(list === 'titles' ? 'conversations' : 'titles')
            setCursor(null)
          }}
          onClose={props.onDismiss}
        />
      </footer>
    </Overlay>
  )
}

/** The list a query opens on: Titles and prompts when it has matches or is still read, else Conversations when they have some. */
function opening(titles: HistoryAnswer, conversations: HistoryAnswer): HistoryListKind {
  const has = (answer: HistoryAnswer): boolean => answer !== null && !('error' in answer) && answer.sessions.length > 0
  if (has(titles) || titles === null) return 'titles'
  return has(conversations) ? 'conversations' : 'titles'
}

/** What F2's field starts with: the name given by hand, else the agent's title, else the first prompt's first line, unquoted. */
function renameSeed(session: HistorySession): string {
  const named = session.label ?? session.agentTitle
  if (named !== undefined && named !== null && named.trim() !== '') return named
  return (session.title.split(/\r?\n/, 1)[0]?.trim() ?? '').slice(0, HISTORY_NAME_MAX)
}

/** Whether `item` is the row a rename was started on: a session's or a child's, never a matched line. */
function renamed(item: Item, session: HistorySession): boolean {
  return (item.kind === 'session' || item.kind === 'child') && item.session.sessionId === session.sessionId
}

/** A row is chosen with the pointer without taking focus from the field. */
const keepFocus = (event: { preventDefault: () => void }): void => event.preventDefault()

type Blocked =
  | { kind: 'reading' }
  | { kind: 'empty' }
  | { kind: 'unreadable'; file: string; message: string }
  | { kind: 'archive'; message: string }
  | { kind: 'failed'; message: string }
  | { kind: 'uncounted'; message: string }
  | { kind: 'no-match'; other: HistoryListKind }

/** A read that failed, which Enter tries again; no match, reading and empty have nothing to try again. */
function retries(state: Blocked): boolean {
  return state.kind === 'unreadable' || state.kind === 'archive' || state.kind === 'failed' || state.kind === 'uncounted'
}

function blockedState(props: HistoryPaletteProps, typed: boolean, list: HistoryListKind, shown: HistoryAnswer): Blocked | null {
  if (props.readError !== null && (shown === null || 'error' in shown || shown.sessions.length === 0)) {
    return { kind: 'unreadable', ...props.readError }
  }
  const stoppedRows = !typed && props.stopped !== null && props.stopped.sessions.length > 0
  if (shown === null) return stoppedRows ? null : { kind: 'reading' }
  if ('error' in shown) {
    if (typed && list === 'conversations') return { kind: 'archive', message: shown.error }
    return { kind: 'failed', message: shown.error }
  }
  if (shown.sessions.length > 0 || stoppedRows) return null
  // "Nothing there" only when the host said it holds none: a count that could
  // not be read is said as that, and one not read yet is still reading.
  if (!typed) {
    if (props.total === 0) return { kind: 'empty' }
    if (props.totalError !== null && props.totalError !== undefined) return { kind: 'uncounted', message: props.totalError }
    return props.total === null ? { kind: 'reading' } : null
  }
  return { kind: 'no-match', other: list === 'titles' ? 'conversations' : 'titles' }
}

function BlockedBody({ state }: { state: Blocked }): JSX.Element {
  const body = (title: string, detail: ReactNode): JSX.Element => (
    <div data-history-state={state.kind} className="px-[13px] pt-3 pb-4 text-[12px] leading-[1.55]">
      <p className="text-fg">{title}</p>
      {detail !== null && <p className="mt-1 text-fg-muted">{detail}</p>}
    </div>
  )
  switch (state.kind) {
    case 'reading':
      return (
        <p data-history-state="reading" className="px-[13px] pt-3 pb-4 text-[12px] text-fg-muted">
          Reading the history file…
        </p>
      )
    case 'empty':
      return body('No sessions yet', 'A Claude Code session started anywhere on this PC is listed here.')
    case 'unreadable':
      return body(
        'Could not read the history file',
        <>
          <span className="font-mono text-[11px]">{state.file}</span> {state.message}
        </>
      )
    case 'archive':
      return body(
        'The archive could not be searched',
        <>
          Floti’s database answered: {state.message}. Titles and prompts were searched; switch to them with Tab.
        </>
      )
    case 'failed':
      return body('History could not be searched', state.message)
    case 'uncounted':
      return body('Could not tell what History holds', state.message)
    case 'no-match':
      return (
        <p data-history-state="no-match" className="px-[13px] pt-3 pb-4 text-[12px] text-fg-muted">
          No match.
        </p>
      )
  }
}

function countOf(answer: HistoryAnswer): string {
  if (answer === null) return '…'
  if ('error' in answer) return 'Unknown'
  return String(answer.total)
}

function ListSwitch({
  list,
  titles,
  conversations,
  onList
}: {
  list: HistoryListKind
  titles: HistoryAnswer
  conversations: HistoryAnswer
  onList: (list: HistoryListKind) => void
}): JSX.Element {
  const option = (kind: HistoryListKind, label: string, answer: HistoryAnswer): JSX.Element => (
    <button
      type="button"
      role="tab"
      aria-selected={list === kind}
      data-history-list={kind}
      onMouseDown={keepFocus}
      onClick={() => onList(kind)}
      className={cn(
        'flex items-center gap-1.5 rounded-raised px-2 py-0.5 text-[11px] transition-colors',
        list === kind ? 'bg-surface text-fg' : 'text-fg-muted hover:text-fg'
      )}
    >
      {label}
      <span className={cn('font-mono text-[10.5px] tabular-nums', list === kind ? 'text-fg-muted' : 'text-fg-subtle')}>
        {countOf(answer)}
      </span>
    </button>
  )
  return (
    <span role="tablist" aria-label="History lists" className="flex shrink-0 gap-0.5 rounded-well border border-border bg-surface-sunken p-0.5">
      {option('titles', 'Titles and prompts', titles)}
      {option('conversations', 'Conversations', conversations)}
    </span>
  )
}

function Marked({ text, query }: { text: string; query: string }): JSX.Element {
  return (
    <>
      {marks(text, query).map((part, index) =>
        part.hit ? (
          <span key={index} className="font-medium text-accent-text">
            {part.text}
          </span>
        ) : (
          <Fragment key={index}>{part.text}</Fragment>
        )
      )}
    </>
  )
}

const ROW = 'relative flex cursor-default items-center gap-2 rounded-raised whitespace-nowrap transition-colors'
const CHOSEN = 'bg-accent-soft'
const EDGE = <span aria-hidden className="absolute top-[7px] bottom-[7px] left-0 w-[2px] rounded-full bg-accent" />

const WORD_TONE = { ok: 'text-success', accent: 'text-accent-text', warn: 'text-warn', subtle: 'text-fg-subtle' } as const

function ItemRow({
  item,
  chosen,
  common,
  query,
  running,
  gone,
  unfolded,
  onToggle,
  renaming,
  onDraft,
  onFinishRename
}: {
  item: Item
  chosen: boolean
  common: Record<string, unknown>
  query: string
  running: HistoryRunning | null
  gone: boolean
  unfolded: boolean
  onToggle: (parent: HistorySession) => void
  renaming: string | null
  onDraft: (draft: string) => void
  onFinishRename: (keep: boolean) => void
}): JSX.Element {
  if (item.kind === 'stopped-all') {
    return (
      <div {...common} data-history-row="stopped-all" className={cn(ROW, 'h-[34px] pr-2.5 pl-[30px]', chosen ? CHOSEN : 'hover:bg-hover')}>
        {chosen && EDGE}
        <ResumeIcon width={12} height={12} className="absolute top-[11px] left-[15px] text-accent-text" />
        <span className="min-w-0 truncate text-accent-text">
          Resume the {item.sessions.length === 1 ? 'one' : String(item.sessions.length)} that can come back
        </span>
      </div>
    )
  }
  if (item.kind === 'stopped') {
    const lost = item.lost
    const word = lost.blocked !== null ? (/folder|disk/i.test(lost.blocked) ? 'folder gone' : 'cannot resume') : lost.agentProblem !== undefined ? 'agent gone' : 'stopped'
    return (
      <div {...common} data-history-row="stopped" data-history-lost={lost.id} className={cn(ROW, 'h-[34px] pr-2.5 pl-[30px] text-[12.5px]', chosen ? CHOSEN : 'hover:bg-hover')}>
        {chosen && EDGE}
        <span className={cn('min-w-0 truncate', lost.blocked === null ? 'text-fg' : 'text-fg-muted')}>{lost.title ?? lost.name}</span>
        {lost.agent !== undefined && <AgentTag name={lost.agent} gone={lost.agentProblem !== undefined} />}
        <span className="ml-auto flex shrink-0 items-center gap-2.5 text-[11px] text-fg-subtle">
          <span className={word === 'stopped' ? 'text-fg-subtle' : 'text-warn'}>{word}</span>
          <span className="font-mono">{folderName(lost.cwd)}</span>
          <span className="w-[58px] text-right tabular-nums">{lost.lastAt === null ? '' : clock(lost.lastAt)}</span>
        </span>
      </div>
    )
  }
  if (item.kind === 'line') {
    return (
      <div {...common} data-history-row="line" className={cn(ROW, 'h-[28px] items-baseline gap-2.5 pt-1.5 pr-2.5 pl-[46px] text-[11.5px] text-fg-muted', chosen ? CHOSEN : 'hover:bg-hover')}>
        {chosen && <span aria-hidden className="absolute top-[6px] bottom-[6px] left-0 w-[2px] rounded-full bg-accent" />}
        <span className="w-[34px] shrink-0 font-mono text-[10.5px] text-fg-subtle">{clock(item.line.at)}</span>
        <span className="min-w-0 truncate">
          <Marked text={snippet(item.line.text, query)} query={query} />
        </span>
      </div>
    )
  }
  const session = item.session
  const name = paletteName(session)
  const word = rowWord(session, running, gone)
  const muted = name.quoted || session.titleFallback || (running === null && unresumable(session) !== null)
  const title =
    renaming !== null ? (
      <input
        autoFocus
        aria-label="Name"
        data-history-rename
        value={renaming}
        maxLength={HISTORY_NAME_MAX}
        onChange={(event) => onDraft(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === 'Enter' || event.key === 'Escape') {
            event.preventDefault()
            event.stopPropagation()
            onFinishRename(event.key === 'Enter')
          }
        }}
        onBlur={() => onFinishRename(false)}
        className="h-[26px] min-w-0 flex-1 rounded-raised border border-accent bg-surface-sunken px-[7px] text-[12.5px] text-fg outline-none"
      />
    ) : (
      <span className={cn('min-w-0 truncate', muted ? 'text-fg-muted' : 'text-fg')}>
        <Marked text={name.text} query={query} />
      </span>
    )

  if (item.kind === 'child') {
    const ended = running === null ? endedWord(session) : null
    return (
      <div {...common} data-history-row="child" data-history-session={session.sessionId} className={cn(ROW, 'h-[30px] pr-2.5 pl-[46px] text-[12.5px]', chosen ? CHOSEN : 'hover:bg-hover')}>
        {chosen && <span aria-hidden className="absolute top-[6px] bottom-[6px] left-0 w-[2px] rounded-full bg-accent" />}
        <span aria-hidden className={cn('absolute top-[-4px] left-[21px] border-l border-border-strong', item.last ? 'bottom-[15px]' : 'bottom-0')} />
        <span aria-hidden className="absolute top-[15px] left-[21px] w-[9px] border-t border-border-strong" />
        {running !== null ? (
          <Dot working={running.working} className="absolute top-[12px] left-[34px]" />
        ) : (
          <span
            aria-hidden
            data-history-dot={ended?.danger === true ? 'failed' : 'ended'}
            className={cn('absolute top-[12px] left-[34px] size-[6px] rounded-full', ended?.danger === true ? 'bg-danger' : 'bg-fg-subtle')}
          />
        )}
        {title}
        {session.agent !== undefined && <AgentTag name={session.agent} gone={gone} />}
        <span className={cn('ml-auto shrink-0 text-[11px] tabular-nums', ended?.danger === true ? 'text-danger' : 'text-fg-subtle')}>
          {running !== null ? (running.working ? 'working' : 'running') : (ended?.text ?? historyWhen(session.lastAt))}
        </span>
      </div>
    )
  }

  const kids = session.children ?? 0
  return (
    <div
      {...common}
      data-history-row="session"
      data-history-session={session.sessionId}
      className={cn(ROW, 'h-[34px] pr-2.5 pl-[30px] text-[12.5px]', chosen ? CHOSEN : 'hover:bg-hover')}
    >
      {chosen && EDGE}
      {kids > 0 && (
        <button
          type="button"
          tabIndex={-1}
          aria-label={unfolded ? 'Fold its children' : 'Show its children'}
          aria-expanded={unfolded}
          onMouseDown={keepFocus}
          onClick={(event) => {
            event.stopPropagation()
            onToggle(session)
          }}
          className="absolute top-[10px] left-[6px] w-[9px] text-center text-[9px] text-fg-subtle"
        >
          {unfolded ? '▾' : '▸'}
        </button>
      )}
      {running !== null && <Dot working={running.working} className="absolute top-[14px] left-[18px]" />}
      {title}
      {session.agent !== undefined && <AgentTag name={session.agent} gone={gone} />}
      {kids > 0 && <span className="shrink-0 text-[11px] text-fg-subtle">{kids === 1 ? '1 child' : `${String(kids)} children`}</span>}
      <span className="ml-auto flex shrink-0 items-center gap-2.5 text-[11px] text-fg-subtle">
        {word !== null && <span className={WORD_TONE[word.tone]}>{word.text}</span>}
        <span className="font-mono">{session.projectName}</span>
        <span className="w-[58px] text-right tabular-nums">{historyWhen(session.lastAt)}</span>
      </span>
    </div>
  )
}

/** The state dot of a session running now (DESIGN.md "Status"). */
function Dot({ working, className }: { working: boolean; className?: string }): JSX.Element {
  return (
    <span
      aria-hidden
      data-history-dot={working ? 'working' : 'running'}
      className={cn('size-[6px] rounded-full', working ? 'bg-accent' : 'bg-success', className)}
    />
  )
}

function folderName(path: string): string {
  return path.replace(/[\\/]+$/, '').split(/[\\/]/).pop() ?? path
}

function Foot({
  typed,
  list,
  recent,
  titles,
  conversations,
  total,
  hostName
}: HistoryPaletteProps & { typed: boolean; list: HistoryListKind }): JSX.Element | null {
  const where = hostName === undefined ? '' : ` on ${hostName}`
  let text: string | null = null
  if (!typed) {
    if (recent !== null && !('error' in recent) && recent.sessions.length > 0) {
      const shown = Math.min(recent.sessions.length, RECENT)
      text = `The ${shown === 1 ? 'one' : String(shown)} you worked in last${where}.${total === null ? '' : ` Type to search all ${String(total)}.`}`
    }
  } else {
    const count = (answer: HistoryAnswer): number | null => (answer === null || 'error' in answer ? null : answer.total)
    const t = count(titles)
    const c = count(conversations)
    if (list === 'conversations' && c !== null && c > 0) {
      text = `${c === 1 ? 'One conversation mentions' : `${String(c)} conversations mention`} it, newest first.${t === null ? '' : ` Tab shows the ${String(t)} ${t === 1 ? 'session whose title or prompts do' : 'sessions whose titles or prompts do'}.`}`
    } else if (list === 'titles' && t !== null && t > 0) {
      text = `${t === 1 ? 'One session matches' : `${String(t)} sessions match`} in its name, title, prompts or folder, newest first.${c === null ? '' : ` Tab shows the ${String(c)} ${c === 1 ? 'conversation that mentions' : 'conversations that mention'} it.`}`
    }
  }
  if (text === null) return null
  return (
    <p data-history-foot className="px-[13px] pt-[3px] pb-2 text-[11px] text-fg-subtle">
      {text}
    </p>
  )
}

function Preview({
  item,
  session,
  props,
  running,
  gone,
  renaming
}: {
  item: Item
  session: HistorySession | null
  props: HistoryPaletteProps
  running: HistoryRunning | null
  gone: boolean
  renaming: boolean
}): JSX.Element {
  const box = (children: ReactNode): JSX.Element => (
    <div
      data-history-preview
      className="mx-2.5 mt-0.5 mb-2.5 flex flex-col gap-[5px] rounded-well border border-border bg-surface px-3 pt-[9px] pb-2.5 text-[11.5px] leading-[1.5] text-fg-muted"
    >
      {children}
      {props.resumeError !== null && props.resumeError !== undefined && (
        <p role="alert" className="text-danger">
          {props.resumeError}
        </p>
      )}
    </div>
  )
  if (item.kind === 'stopped-all' || item.kind === 'stopped') {
    const all = item.kind === 'stopped-all' ? item.sessions : [item.lost]
    const names = all.map((lost) => lost.title ?? lost.name)
    if (item.kind === 'stopped' && item.lost.blocked !== null) return box(<p className="text-warn">{item.lost.blocked}</p>)
    return box(
      <>
        {item.kind === 'stopped' && item.lost.agentProblem !== undefined && (
          <p className="text-warn">{item.lost.agentProblem} Enter resumes it without the agent.</p>
        )}
        <p className="text-[11px] text-fg-subtle">
          Enter resumes {listed(names)}, each in its own folder{item.kind === 'stopped' && item.lost.agentProblem !== undefined ? '' : ' with its agent'}, in a new tab.
        </p>
      </>
    )
  }
  if (session === null) return box(null)
  const profile = session.profileId === undefined ? null : props.profileName(session.profileId)
  const facts = [
    session.branch === undefined ? null : `› ${session.branch}`,
    profile,
    session.agent === undefined ? null : `${session.agent} agent`,
    `${String(session.promptCount)} ${session.promptCount === 1 ? 'prompt' : 'prompts'}`,
    historySpan(session.firstAt, session.lastAt)
  ].filter((fact) => fact !== null)
  const word = running === null ? unresumable(session) : null
  const sentence = launchSentence(session, { profile, withoutAgent: gone, hostName: props.hostName })
  const parent = item.kind === 'child' ? item.parent : null
  const ended = item.kind === 'child' ? endedWord(session) : null
  const name = paletteName(session)
  return box(
    <>
      <div className="flex min-w-0 flex-wrap gap-x-1.5 font-mono text-[10.5px]">
        <span className="text-fg-muted">{session.project}</span>
        {facts.map((fact) => (
          <span key={fact} className="text-fg-subtle">
            {fact.startsWith('›') ? fact : `· ${fact}`}
          </span>
        ))}
      </div>
      {parent !== null && (
        <p>
          Started by <span className="text-fg">{paletteName(parent).text}</span>
          {ended === null ? '.' : `; ${ended.text.replace(/^\w/, (c) => c.toLowerCase())}.`}
        </p>
      )}
      {session.lastReport !== undefined && (
        <p>
          <span className="font-medium text-accent-text">Reported {session.lastReport.state}.</span> {session.lastReport.text}
        </p>
      )}
      {item.kind === 'line' && <p>“{snippet(item.line.text, props.query, 80)}”</p>}
      {item.kind !== 'line' && session.lastReport === undefined && running === null && session.lastReply !== undefined && (
        <p>
          <span className="font-medium text-fg">Last reply.</span> {session.lastReply}
        </p>
      )}
      {renaming ? (
        <p className="text-[11px] text-fg-subtle">
          Your name replaces {name.quoted ? `${name.text}, the first line of its first prompt` : `“${name.text}”`}, here and in its tab when it is resumed. An agent’s title never overwrites it. Leave it empty to go back.
        </p>
      ) : running !== null ? (
        <p className="text-[11px] text-fg-subtle">
          It runs now{props.hostName === undefined ? '' : ` on ${props.hostName}`}. Enter {running.here ? 'shows its tab' : 'opens it in a tab here'}; Floti shows no part of a running conversation in History.
        </p>
      ) : word !== null ? (
        <p className="text-[11px] text-fg-subtle">
          {unresumableSentence(word, session.project)} Enter opens {word === 'prompts only' || word === 'dropped' ? 'what you typed' : 'the conversation'} to read.
        </p>
      ) : (
        <>
          {gone && <p className="text-warn">The {session.agent} agent is gone. Enter resumes it without the agent.</p>}
          <p className="text-[11px] text-fg-subtle">
            {item.kind === 'line' ? `${CTRL}+Enter opens this conversation read-only at ${clock(item.line.at)}. Enter resumes it: ` : 'Enter runs '}
            <LaunchSentence sentence={sentence} folder={session.project} />, in a new tab
            {parent !== null ? '. Its parent is not told; a lead resumes its own children itself.' : '.'}
          </p>
        </>
      )}
    </>
  )
}

/** The launch sentence with its command and folder in mono, as the rest of the sentence is prose. */
function LaunchSentence({ sentence, folder }: { sentence: string; folder: string }): JSX.Element {
  const mono = 'font-mono text-[10.5px] text-fg-muted'
  const command = 'claude --resume'
  const head = `${command} in ${folder}`
  if (!sentence.startsWith(head)) return <span className={mono}>{sentence}</span>
  return (
    <>
      <span className={mono}>{command}</span> in <span className={mono}>{folder}</span>
      {sentence.slice(head.length)}
    </>
  )
}

function listed(names: readonly string[]): string {
  if (names.length <= 1) return names[0] ?? ''
  return `${names.slice(0, -1).join(', ')} and ${names[names.length - 1] ?? ''}`
}

function Keys({
  blocked,
  item,
  session,
  running,
  renaming,
  typed,
  list,
  onEnter,
  onRead,
  onRename,
  onKeep,
  onLeave,
  onFold,
  unfolded,
  onTab,
  onClose
}: {
  /** Why nothing can be chosen: `retry` where a read failed and Enter tries again, `still` where there is nothing to try again. */
  blocked: 'retry' | 'still' | null
  item: Item | undefined
  session: HistorySession | null
  running: HistoryRunning | null
  renaming: boolean
  typed: boolean
  list: HistoryListKind
  onEnter: () => void
  onRead: () => void
  onRename: () => void
  onKeep: () => void
  onLeave: () => void
  onFold: () => void
  unfolded: boolean
  onTab: () => void
  onClose: () => void
}): JSX.Element {
  if (renaming) {
    return (
      <>
        <KeyButton keys="↵" main onClick={onKeep}>
          Keep
        </KeyButton>
        <span className="flex-1" />
        <KeyButton keys="Esc" onClick={onLeave}>
          Leave it as it was
        </KeyButton>
      </>
    )
  }
  const other = list === 'titles' ? 'Conversations' : 'Titles and prompts'
  const enter = blocked !== null
    ? blocked === 'retry'
      ? 'Try again'
      : null
    : item?.kind === 'stopped-all'
      ? `Resume ${String(item.sessions.length)}`
      : session === null
        ? item?.kind === 'stopped'
          ? 'Resume'
          : null
        : running !== null
          ? running.here
            ? 'Show'
            : 'Open'
          : unresumable(session) !== null
            ? 'Read'
            : 'Resume'
  const readable = blocked === null && session !== null && running === null && unresumable(session) === null
  return (
    <>
      {enter !== null && (
        <KeyButton keys="↵" main onClick={onEnter}>
          {enter}
        </KeyButton>
      )}
      {readable && (
        <KeyButton keys={`${CTRL} ↵`} onClick={onRead}>
          {item?.kind === 'line' ? `Read at ${clock(item.line.at)}` : 'Read'}
        </KeyButton>
      )}
      {blocked === null && (item?.kind === 'child' || (item?.kind === 'session' && (session?.children ?? 0) > 0)) && (
        <KeyButton keys={unfolded ? '←' : '→'} onClick={onFold}>
          {unfolded ? 'Fold' : 'Children'}
        </KeyButton>
      )}
      {blocked === null && session !== null && item?.kind !== 'line' && (
        <KeyButton keys="F2" onClick={onRename}>
          Rename
        </KeyButton>
      )}
      {typed && (
        <KeyButton keys="Tab" onClick={onTab}>
          {other}
        </KeyButton>
      )}
      <span className="flex-1" />
      <KeyButton keys="Esc" onClick={onClose}>
        Close
      </KeyButton>
    </>
  )
}
