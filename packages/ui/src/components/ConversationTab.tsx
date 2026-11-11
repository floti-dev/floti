import type { JSX } from 'react'
import { useEffect, useMemo, useRef, useState } from 'react'
import type { ArchiveMessage, ArchivedConversation, HistoryPrompt } from '@helm/core'
import { HISTORY_NAME_MAX } from '@helm/core/types'
import { cn } from '../lib/cn'
import { clock, marks, queryWords } from '../lib/historyPalette'
import { formatMoment } from '../lib/time'
import { AgentTag } from './AgentTag'
import { ICON_BUTTON } from '../lib/page'
import { PencilIcon, ResumeIcon, SearchIcon } from './icons'

/**
 * A conversation from History, read-only in a tab (H7): named by its title,
 * opened at the line it was asked for, with Find and its count in the bar
 * beside Rename, Show in Explorer and Resume, and a status line that says it
 * is read only. Turns run across the column, the user's tinted with the
 * accent edge, and a run of tool calls is one mono line. Only an archived
 * conversation is drawn: Floti renders no part of a live one. Where the archive
 * has none, what the user typed is shown instead.
 */
export interface ConversationTabProps {
  title: string
  agent?: string | undefined
  /** The folder it ran in, and the facts after it: branch, profile, span, prompts. */
  project: string
  facts: readonly string[]
  /** The archived conversation; null when the archive has none; undefined while it is read. */
  conversation: ArchivedConversation | null | undefined
  /** The read failed, with the host's sentence. */
  error?: string | null | undefined
  /** What was typed, shown when the archive has no conversation. */
  prompts?: readonly HistoryPrompt[] | null | undefined
  /** The message to open at, by its id. */
  at?: string | null | undefined
  /** What Find starts with: the words searched for in History. */
  find?: string | undefined
  /** Absent when it cannot be resumed. */
  onResume?: (() => void) | undefined
  onRename: (name: string | null) => void
  /** Absent when its folder cannot be shown from here. */
  onReveal?: (() => void) | undefined
  resumeError?: string | null | undefined
}

const TOOL_MARKER = /^\[tool(?::\s*(.+))?\]$/

/** A message that is nothing but tool calls, as the archive stores them (`[tool: Name]`), or null. */
function toolNames(text: string): string[] | null {
  const names: string[] = []
  for (const part of text.split('\n\n')) {
    const matched = TOOL_MARKER.exec(part.trim())
    if (matched === null) return null
    names.push(matched[1] ?? 'tool')
  }
  return names.length > 0 ? names : null
}

type Turn = { kind: 'message'; message: ArchiveMessage } | { kind: 'tools'; key: string; names: string[] }

/** The messages, with each run of tool calls folded into one line. */
export function readAsTurns(messages: readonly ArchiveMessage[]): Turn[] {
  const turns: Turn[] = []
  for (const message of messages) {
    const names = toolNames(message.text)
    if (names === null) {
      turns.push({ kind: 'message', message })
      continue
    }
    const previous = turns.at(-1)
    if (previous?.kind === 'tools') previous.names.push(...names)
    else turns.push({ kind: 'tools', key: message.uuid, names })
  }
  return turns
}

/** `Read ×3 · Bash`: consecutive repeats counted rather than listed. */
export function summariseTools(names: readonly string[]): string {
  const runs: { name: string; n: number }[] = []
  for (const name of names) {
    const last = runs.at(-1)
    if (last?.name === name) last.n++
    else runs.push({ name, n: 1 })
  }
  return runs.map((run) => (run.n === 1 ? run.name : `${run.name} ×${String(run.n)}`)).join(' · ')
}

function matchesQuery(text: string, words: readonly string[]): boolean {
  const lower = text.toLowerCase()
  return words.length > 0 && words.every((word) => lower.includes(word))
}

export function ConversationTab(props: ConversationTabProps): JSX.Element {
  const { conversation, at } = props
  const [find, setFind] = useState(props.find ?? '')
  const [renaming, setRenaming] = useState<string | null>(null)
  const turns = useMemo(
    () => (conversation === null || conversation === undefined ? [] : readAsTurns(conversation.messages)),
    [conversation]
  )
  const words = useMemo(() => queryWords(find), [find])
  const hits = useMemo(
    () =>
      turns.flatMap((turn) => (turn.kind === 'message' && matchesQuery(turn.message.text, words) ? [turn.message.uuid] : [])),
    [turns, words]
  )
  // Opened at the line it was asked for (that match, else the first) until
  // Find is stepped or changed.
  const [stepped, setStepped] = useState<number | null>(null)
  const current = stepped ?? Math.max(0, at === null || at === undefined ? 0 : hits.indexOf(at))
  const setCurrent = (index: number): void => setStepped(index)
  const shownHit = hits[Math.min(current, Math.max(0, hits.length - 1))]
  const target = shownHit ?? (stepped === null ? (at ?? undefined) : undefined)

  const column = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (target === undefined) return
    const node = column.current?.querySelector(`[data-message="${CSS.escape(target)}"]`)
    if (node instanceof HTMLElement) node.scrollIntoView?.({ block: 'center' })
  }, [target, turns])

  const step = (by: number): void => {
    if (hits.length === 0) return
    setCurrent((Math.min(current, hits.length - 1) + by + hits.length) % hits.length)
  }

  const facts = (
    <span className="min-w-0 truncate font-mono text-[10.5px] text-fg-subtle">
      <span className="text-fg-muted">{props.project}</span>
      {props.facts.map((fact) => ` · ${fact}`).join('')}
    </span>
  )

  return (
    <div data-conversation-tab className="flex h-full min-h-0 flex-col">
      <div className="flex min-h-[40px] shrink-0 flex-wrap items-center gap-x-2.5 gap-y-1 border-b border-border px-3 py-1.5">
        <div className="flex min-w-0 flex-1 items-center gap-2">
          {renaming === null ? (
            <h2 className="min-w-0 truncate text-[13px] font-medium text-fg">{props.title}</h2>
          ) : (
            <input
              autoFocus
              aria-label="Name"
              data-conversation-rename
              value={renaming}
              maxLength={HISTORY_NAME_MAX}
              onChange={(event) => setRenaming(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  const name = renaming.trim()
                  props.onRename(name === '' ? null : name)
                  setRenaming(null)
                } else if (event.key === 'Escape') {
                  event.stopPropagation()
                  setRenaming(null)
                }
              }}
              onBlur={() => setRenaming(null)}
              className="h-[26px] min-w-0 flex-1 rounded-raised border border-accent bg-surface-sunken px-[7px] text-[12.5px] text-fg outline-none"
            />
          )}
          {props.agent !== undefined && <AgentTag name={props.agent} />}
          {facts}
        </div>
        <label className="flex h-[26px] w-[200px] items-center gap-1.5 rounded-raised border border-border bg-surface-sunken px-2">
          <SearchIcon width={12} height={12} className="shrink-0 text-fg-subtle" />
          <input
            aria-label="Find in the conversation"
            data-conversation-find
            value={find}
            onChange={(event) => {
              setFind(event.target.value)
              setCurrent(0)
            }}
            onKeyDown={(event) => {
              if (event.key === 'Enter') step(event.shiftKey ? -1 : 1)
            }}
            spellCheck={false}
            placeholder="Find"
            className="min-w-0 flex-1 bg-transparent text-[12px] text-fg outline-none placeholder:text-fg-subtle"
          />
          {words.length > 0 && (
            <span data-conversation-count className="shrink-0 font-mono text-[10.5px] text-fg-subtle tabular-nums">
              {hits.length === 0 ? '0' : `${String(Math.min(current, hits.length - 1) + 1)} of ${String(hits.length)}`}
            </span>
          )}
        </label>
        <button type="button" aria-label="Previous match" className={ICON_BUTTON} onClick={() => step(-1)} disabled={hits.length === 0}>
          ‹
        </button>
        <button type="button" aria-label="Next match" className={ICON_BUTTON} onClick={() => step(1)} disabled={hits.length === 0}>
          ›
        </button>
        <button type="button" aria-label="Rename" title="Rename" className={ICON_BUTTON} onClick={() => setRenaming(props.title)}>
          <PencilIcon width={13} height={13} />
        </button>
        {props.onReveal !== undefined && (
          <button
            type="button"
            data-conversation-reveal
            onClick={props.onReveal}
            className="h-[26px] rounded-raised px-2 text-[11.5px] text-fg-muted hover:bg-hover hover:text-fg"
          >
            Show in Explorer
          </button>
        )}
        {props.onResume !== undefined && (
          <button
            type="button"
            data-conversation-resume
            onClick={props.onResume}
            className="flex h-[26px] items-center gap-1.5 rounded-raised border border-border-strong px-2.5 text-[11.5px] text-accent-text hover:bg-hover"
          >
            <ResumeIcon width={12} height={12} />
            Resume
          </button>
        )}
      </div>
      {props.resumeError !== null && props.resumeError !== undefined && (
        <p role="alert" className="border-b border-border px-3 py-1.5 text-[11.5px] text-danger">
          {props.resumeError}
        </p>
      )}

      <div ref={column} className="min-h-0 flex-1 overflow-y-auto px-4 py-3">
        <Body {...props} turns={turns} words={words} current={shownHit} />
      </div>

      <footer className="flex h-[24px] shrink-0 items-center gap-3 border-t border-border bg-surface px-3 text-[11px] text-fg-subtle">
        <span className="text-fg-muted">Read only</span>
        {conversation !== null && conversation !== undefined && (
          <span title={`Captured ${conversation.capturedAt}`}>
            {conversation.messageCount} {conversation.messageCount === 1 ? 'message' : 'messages'} archived
          </span>
        )}
        <span className="flex-1" />
        <span>Opened from History · Ctrl+H</span>
      </footer>
    </div>
  )
}

function Body({
  conversation,
  error,
  prompts,
  turns,
  words,
  current
}: ConversationTabProps & { turns: Turn[]; words: readonly string[]; current: string | undefined }): JSX.Element {
  if (error !== null && error !== undefined) {
    return (
      <p role="alert" className="text-[12px] text-danger">
        The conversation could not be read: {error}
      </p>
    )
  }
  if (conversation === undefined) return <p className="text-[12px] text-fg-subtle">Reading the conversation…</p>
  if (conversation === null || conversation.state !== 'archived') {
    const why =
      conversation?.state === 'evicted'
        ? 'Floti dropped this conversation to stay under the limit in Settings › Archive. What you typed is kept.'
        : 'Floti has no archive of this conversation. What you typed is kept.'
    return (
      <div className="flex flex-col gap-3">
        <p className="text-[12px] text-fg-muted">{why}</p>
        {(prompts ?? []).map((prompt) => (
          <Turn key={prompt.seq} mine at={prompt.at} text={prompt.text} words={words} current={false} id={String(prompt.seq)} />
        ))}
      </div>
    )
  }
  return (
    <div data-transcript={conversation.sessionId} className="flex flex-col gap-3.5">
      {turns.map((turn) =>
        turn.kind === 'tools' ? (
          <div key={turn.key} data-transcript-tools={String(turn.names.length)} className="pl-3 font-mono text-[10.5px] text-fg-subtle select-text">
            {summariseTools(turn.names)}
          </div>
        ) : (
          <Turn
            key={turn.message.uuid}
            id={turn.message.uuid}
            mine={turn.message.role === 'user'}
            at={turn.message.at}
            text={turn.message.text}
            words={words}
            current={turn.message.uuid === current}
          />
        )
      )}
    </div>
  )
}

function Turn({
  id,
  mine,
  at,
  text,
  words,
  current
}: {
  id: string
  mine: boolean
  at: number
  text: string
  words: readonly string[]
  current: boolean
}): JSX.Element {
  return (
    <div
      data-message={id}
      data-transcript-message={mine ? 'user' : 'assistant'}
      data-current-match={current ? '' : undefined}
      className={cn(
        'relative rounded-raised px-3 py-2',
        mine && 'bg-accent-soft',
        current && 'ring-1 ring-accent ring-inset'
      )}
    >
      {mine && <span aria-hidden className="absolute top-2 bottom-2 left-0 w-[2px] rounded-full bg-accent" />}
      <div className="mb-0.5 text-[11px] text-fg-subtle" title={formatMoment(at)}>
        <span className={mine ? 'text-fg-muted' : 'text-fg-muted'}>{mine ? 'You' : 'Claude'}</span> · {clock(at)}
      </div>
      <div className="text-[12.5px] leading-relaxed break-words whitespace-pre-wrap text-fg select-text">
        {words.length === 0
          ? text
          : marks(text, words.join(' ')).map((part, index) =>
              part.hit ? (
                <mark key={index} className="bg-transparent font-medium text-accent-text">
                  {part.text}
                </mark>
              ) : (
                <span key={index}>{part.text}</span>
              )
            )}
      </div>
    </div>
  )
}
