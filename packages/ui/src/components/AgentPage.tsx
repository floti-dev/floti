import { useState, type JSX, type ReactNode } from 'react'
import {
  agentNameIssue,
  differingLines,
  readAgentDraft,
  withBody,
  type AgentProblem,
  type EditorHighlight,
  type InstalledBackend,
  type RenderedMarkdown
} from '@helm/core/types'
import { cn } from '../lib/cn'
import { SEGMENT_ON } from '../lib/segmented'
import { AgentForm, FORM_FIELDS } from './AgentForm'
import { CodeEditor } from './CodeEditor'
import { CopyIcon, SaveIcon, TrashIcon } from './icons'
import { MarkdownBody } from './MarkdownBody'

/** The file as it is on disk now, when it changed after the page read it. */
export interface AgentExternalChange {
  /** Its text now; null when the file was removed. */
  text: string | null
  /** When the page heard of it, as the window shows a time ("14:31"). */
  at: string
}

export interface AgentPageProps {
  /** The agent's name as saved; null for a new agent not yet written. */
  savedName: string | null
  /** The agent a new one copies, by name; null for a blank one. */
  copyOf: string | null
  /** The name field. */
  name: string
  onNameChange: (name: string) => void
  /** The names the folder has, this agent's own included. */
  takenNames: ReadonlySet<string>
  /** The host, as the window names it, and whether it is on another computer. */
  hostName: string
  remote: boolean
  /** The agents folder, as the host spells it. */
  folder: string | null
  draft: string
  onDraftChange: (text: string) => void
  dirty: boolean
  saving: boolean
  /** When it was last saved here, as a time ("13:12"); null when it was not. */
  savedAt: string | null
  /** The host's problems: with the last save it refused, or with the file as read. */
  problems: readonly AgentProblem[]
  /** Why the last save was refused, when it was refused for something other than its problems. */
  refusal: string | null
  /** Whether the file as read is not an agent (Not loaded). */
  bad: boolean
  /** Why the file could not be read again; the page shows it as it was last read. Null when it could. */
  readError: string | null
  external: AgentExternalChange | null
  /** Why the host changes nothing in its agents folder; null when it may. */
  readOnly: string | null
  backends: readonly InstalledBackend[]
  /** The host's profiles, by name; null when the window cannot list them. */
  profiles: readonly string[] | null
  onHighlight: ((path: string, source: string) => Promise<EditorHighlight>) | null
  /** The instructions as the host renders them, for the live preview; null until the first render. */
  preview: RenderedMarkdown | null
  /** Whether the preview is behind the instructions as they are now. */
  previewPending: boolean
  /** Opens a link in the preview outside Floti. */
  onOpenExternal: (url: string) => void
  /** Saves the draft: the Save button, and Ctrl+S anywhere in the page while Save is on. */
  onSave: () => void
  onRevert: () => void
  onDiscard: () => void
  onCopy: () => void
  onDelete: () => void
  /** Opens the New session dialog with this agent picked; null while it cannot be started. */
  onStartSession: (() => void) | null
  onReload: () => void
  onSaveMine: () => void
}

const BAR_BUTTON =
  'flex h-6 items-center gap-1.5 rounded-raised px-2 text-[11.5px] text-fg-muted transition-colors hover:bg-hover hover:text-fg disabled:opacity-50'
const STRIP_BUTTON = 'rounded-well border px-[9px] py-[3px] text-[11.5px] font-medium whitespace-nowrap transition-colors'
/** What Source draws after a retired line (the design's M3). */
const RETIRED_NOTE = 'no longer used · saving removes it'

/**
 * One agent, opened as a page (the design's A5-A16): a bar with Form and
 * Source, its file's path and what can be done with it; strips for a file
 * that changed outside, a file the host cannot load and lines Floti no
 * longer reads; the form over the instructions or the raw file; and Save and
 * Revert at the foot with the file's facts. Form and Source are two views of
 * one draft, which the page's owner holds.
 */
export function AgentPage(props: AgentPageProps): JSX.Element {
  const { draft, savedName, name, problems, external, readOnly } = props
  const fields = readAgentDraft(draft)
  const quiet = new Map(fields.retired.map((line) => [line.line, RETIRED_NOTE] as const))
  const unparsed = !fields.frontMatter || problems.some((p) => p.field === null && /front matter/.test(p.message))
  const [view, setView] = useState<'form' | 'source'>(unparsed ? 'source' : 'form')
  const [comparing, setComparing] = useState(false)
  const isNew = savedName === null
  const file = `${name === '' ? (savedName ?? 'new-agent') : name}.md`
  const path = props.folder === null ? file : `${props.folder}${props.folder.includes('\\') ? '\\' : '/'}${savedName ?? name}.md`

  const others = new Set([...props.takenNames].filter((each) => each !== savedName))
  const issue = agentNameIssue(name, others)
  const renaming = !isNew && issue === null && name !== savedName
  const nameHint: ReactNode =
    issue?.kind === 'invalid' ? (
      <>
        An agent's name is lowercase letters, digits and dashes.
        {issue.suggestion !== null && (
          <>
            {' '}
            <span className="font-mono">{issue.suggestion}</span> would do.
          </>
        )}
      </>
    ) : issue?.kind === 'taken' ? (
      <>
        There is already an agent called <span className="font-mono">{name}</span> in this folder.
      </>
    ) : renaming ? (
      <>
        Saving renames <span className="font-mono">{savedName}.md</span> to <span className="font-mono">{name}.md</span>.
      </>
    ) : (
      <>
        Lowercase letters, digits and dashes.{name !== '' && <> The file is <span className="font-mono">{file}</span>.</>}
      </>
    )

  const unplaced = problems.filter((p) => p.field === null || !(FORM_FIELDS as readonly string[]).includes(p.field))
  const canSave =
    readOnly === null &&
    !props.saving &&
    external === null &&
    issue === null &&
    (isNew || props.dirty || renaming || props.bad)
  // The host's problems with a save it refused, or with the file as read: "Not saved" or "Not loaded".
  const refused = !props.bad && problems.length > 0
  const bytes = new TextEncoder().encode(draft).length
  const facts = [file, `${(bytes / 1024).toFixed(1)} KB`, draft.includes('\r\n') ? 'CRLF' : 'LF']
  if (props.remote) facts.push(`on ${props.hostName}`)

  // The Files view's editor beside its live preview (the design's A7).
  const instructions = (
    <div data-agent-instructions className="flex h-[300px] min-h-0 overflow-hidden rounded-well border border-border">
      <div className="flex w-1/2 min-w-0 shrink-0 flex-col">
        <CodeEditor
          surface="content"
          path={file}
          ariaLabel="Instructions"
          value={fields.body}
          onChange={(body) => props.onDraftChange(withBody(draft, body))}
          onHighlight={props.onHighlight}
          wrap
          bare
          readOnly={readOnly !== null}
        />
      </div>
      <div data-agent-preview className="flex w-1/2 min-w-0 border-l border-border bg-surface">
        <MarkdownBody
          path={file}
          rendered={props.preview}
          stale={props.previewPending}
          compact
          toc={false}
          gutter="px-[18px]"
          onOpenExternal={props.onOpenExternal}
        />
      </div>
    </div>
  )

  return (
    <div
      data-agent-page={savedName ?? ''}
      className="flex h-full min-h-0 flex-col bg-surface"
      onKeyDown={(event) => {
        if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 's') {
          event.preventDefault()
          if (canSave) props.onSave()
        }
      }}
    >
      <header className="flex shrink-0 items-center gap-2 border-b border-border px-3 py-1.5">
        <span role="radiogroup" aria-label="View" className="flex shrink-0 gap-0.5 rounded-well border border-border bg-surface-sunken p-0.5">
          {(['form', 'source'] as const).map((each) => (
            <button
              key={each}
              type="button"
              role="radio"
              aria-checked={view === each}
              data-agent-view={each}
              onClick={() => {
                setView(each)
                setComparing(false)
              }}
              className={cn('rounded-raised px-2.5 py-0.5 text-[11.5px] transition-colors', view === each ? SEGMENT_ON : 'text-fg-muted hover:text-fg')}
            >
              {each === 'form' ? 'Form' : 'Source'}
            </button>
          ))}
        </span>
        <span data-agent-path className="min-w-0 truncate font-mono text-[11px] text-fg-subtle">
          {isNew ? (props.copyOf === null ? 'Blank agent · not saved' : `Copy of ${props.copyOf} · not saved`) : props.remote ? `${props.hostName} · ${path}` : path}
        </span>
        <span className="flex-1" />
        {!isNew && props.onStartSession !== null && (
          <button type="button" data-agent-start className={BAR_BUTTON} onClick={props.onStartSession}>
            Start a session…
          </button>
        )}
        {!isNew && (
          <button type="button" data-agent-copy className={BAR_BUTTON} onClick={props.onCopy}>
            <CopyIcon width={12} height={12} />
            Copy
          </button>
        )}
        {!isNew && (
          <button type="button" data-agent-delete className={BAR_BUTTON} disabled={readOnly !== null} onClick={props.onDelete}>
            <TrashIcon width={12} height={12} />
            Delete
          </button>
        )}
      </header>

      {external !== null && (
        <div data-agent-external className="flex shrink-0 flex-col gap-1.5 border-b border-warn/30 bg-warn/10 px-4 py-2.5">
          <span className="text-[12.5px] font-medium text-warn">
            {savedName}.md {external.text === null ? 'was removed from disk after you opened it' : 'changed on disk after you opened it'}
          </span>
          <span className="text-[12px] text-fg-muted">
            Your edit is kept here and not saved.{' '}
            {external.text === null
              ? 'Save yours to put the file back, or close the page.'
              : 'Compare the two, take the file as it is now, or save yours over it. Either can be undone.'}
          </span>
          <span className="flex gap-1.5">
            {external.text !== null && (
              <button type="button" data-agent-compare className={cn(STRIP_BUTTON, 'border-border-strong text-fg hover:bg-hover')} onClick={() => setComparing((on) => !on)}>
                {comparing ? 'Hide the comparison' : 'Compare'}
              </button>
            )}
            {external.text !== null && (
              <button type="button" data-agent-reload className={cn(STRIP_BUTTON, 'border-border-strong text-fg hover:bg-hover')} onClick={() => { setComparing(false); props.onReload() }}>
                Reload from disk
              </button>
            )}
            <button type="button" data-agent-save-mine className={cn(STRIP_BUTTON, 'border-accent text-accent-text hover:bg-accent-soft')} disabled={readOnly !== null} onClick={() => { setComparing(false); props.onSaveMine() }}>
              Save mine
            </button>
          </span>
        </div>
      )}

      <div className="min-h-0 flex-1 overflow-y-auto">
        {comparing && external?.text != null ? (
          <Compare onDisk={external.text} mine={draft} at={external.at} />
        ) : (
          // Source's editor fills the page: its height is the column's, so the column is the page's.
          <div className={cn('flex flex-col gap-3 px-6 py-4', view === 'form' ? 'mx-auto w-full max-w-[760px]' : 'h-full')}>
            {(props.bad || refused || props.refusal !== null) && (
              <div data-agent-problems role="alert" className="rounded-well border border-danger/30 bg-danger/10 px-3 py-2 text-[12px] text-fg-muted">
                <span className="block text-[12.5px] font-medium text-danger">
                  {props.bad ? `Floti cannot load ${savedName ?? name}.md` : 'Not saved'}
                </span>
                {props.refusal !== null && <span className="block">{props.refusal}</span>}
                {props.bad && (
                  <span className="block">
                    A start that names {savedName ?? name} is refused until it loads.
                    {view === 'form' && unplaced.length < problems.length && ' The problems are marked on their fields.'}
                  </span>
                )}
                {refused && (
                  <span className="block">
                    The file on disk is as it was, and your edit is kept.
                    {view === 'form' && unplaced.length < problems.length && ' The problems are marked on their fields.'}
                  </span>
                )}
                {(view === 'source' ? problems : unplaced).map((p, index) => (
                  <span key={index} data-agent-problem className="mt-1 block text-danger">
                    {p.line !== null && <span className="font-mono">line {p.line} · </span>}
                    {p.message}
                    {view === 'form' && '. Fix it in Source.'}
                  </span>
                ))}
              </div>
            )}
            {readOnly !== null && <p className="m-0 text-[12px] text-warn">{readOnly}</p>}
            {props.readError !== null && (
              <p data-agent-read-error className="m-0 text-[12px] text-warn">
                Floti could not read {savedName}.md again, so this is the file as last read: {props.readError}
              </p>
            )}
            {view === 'form' &&
              fields.retired.map((line) => (
                <p key={line.line} data-agent-retired={line.field} className="m-0 text-[12px] text-fg-muted">
                  <span className="font-mono">
                    {line.field}: {line.value}
                  </span>{' '}
                  is a line Floti no longer uses. It changes nothing, and saving writes the file without it.
                </p>
              ))}
            {view === 'form' && fields.sourceOnly.length > 0 && (
              <p data-agent-source-only className="m-0 text-[12px] text-fg-muted">
                This agent has <span className="font-mono">{fields.sourceOnly.join(', ')}</span>, which have no fields here:
                edit them in Source.
              </p>
            )}
            {view === 'form' ? (
              <AgentForm
                draft={draft}
                onDraftChange={props.onDraftChange}
                name={{ value: name, onChange: props.onNameChange, hint: nameHint, warn: issue !== null && issue.kind !== 'empty' }}
                problems={problems}
                backends={props.backends}
                hostName={props.hostName}
                profiles={props.profiles}
                readOnly={readOnly !== null}
                instructions={instructions}
                autoFocusName={isNew}
              />
            ) : (
              <div data-agent-source className="min-h-0 flex-1">
                <CodeEditor
                  surface="content"
                  path={file}
                  ariaLabel="Source"
                  value={draft}
                  onChange={props.onDraftChange}
                  onHighlight={props.onHighlight}
                  wrap
                  readOnly={readOnly !== null}
                  invalid={props.bad || refused}
                  quiet={quiet}
                />
              </div>
            )}
          </div>
        )}
      </div>

      <footer className="flex shrink-0 items-center gap-3 border-t border-border px-3 py-1.5">
        <span data-agent-facts className="min-w-0 truncate font-mono text-[10.5px] text-fg-subtle">
          {facts.join(' · ')}
        </span>
        <span className="flex-1" />
        <span
          data-agent-state
          className={cn('text-[11px]', isNew ? 'text-fg-subtle' : props.dirty || renaming ? 'text-warn' : props.bad ? 'text-danger' : 'text-fg-subtle')}
        >
          {props.saving
            ? 'Saving…'
            : isNew
              ? 'Not saved yet. Nothing is written until you save.'
              : props.dirty || renaming
                ? 'Unsaved changes'
                : props.bad
                  ? 'Not loaded'
                  : props.savedAt !== null
                    ? `Saved ${props.savedAt}`
                    : 'Saved'}
        </span>
        {isNew ? (
          <button type="button" data-agent-discard className={BAR_BUTTON} onClick={props.onDiscard}>
            Discard
          </button>
        ) : (
          (props.dirty || renaming) && (
            <button type="button" data-agent-revert className={BAR_BUTTON} onClick={props.onRevert}>
              Revert
            </button>
          )
        )}
        <button
          type="button"
          data-agent-save
          disabled={!canSave}
          onClick={props.onSave}
          title={external !== null ? 'The file changed on disk; decide above first' : 'Ctrl+S'}
          className={cn(
            'flex h-6 items-center gap-1.5 rounded-raised px-2 text-[11.5px] transition-colors',
            canSave ? 'text-accent-text hover:bg-accent-soft' : 'cursor-default text-fg-subtle opacity-60'
          )}
        >
          <SaveIcon width={11} height={11} />
          {isNew ? 'Save agent' : 'Save'}
          <kbd className="font-mono text-[10px] text-fg-subtle">Ctrl S</kbd>
        </button>
      </footer>
    </div>
  )
}

/** The file as it is now beside the edit, the lines that differ marked (the design's M8). */
function Compare({ onDisk, mine, at }: { onDisk: string; mine: string; at: string }): JSX.Element {
  const marks = differingLines(onDisk, mine)
  return (
    <div data-agent-comparison className="grid min-h-full grid-cols-2 divide-x divide-border">
      <Side title={`On disk now · changed ${at}`} text={onDisk} marked={marks.left} side="disk" />
      <Side title="Your edit" text={mine} marked={marks.right} side="mine" />
    </div>
  )
}

function Side({ title, text, marked, side }: { title: string; text: string; marked: Set<number>; side: string }): JSX.Element {
  return (
    <div data-compare-side={side} className="min-w-0">
      <div className="sticky top-0 border-b border-border bg-surface px-3 py-1.5 text-[11px] font-medium text-fg-muted">{title}</div>
      <pre className="m-0 font-mono text-[11.5px] leading-[1.6]">
        {text.split(/\r?\n/).map((line, index) => {
          const n = index + 1
          const on = marked.has(n)
          return (
            <div
              key={n}
              data-compare-line={on ? 'changed' : undefined}
              className={cn('flex border-l-2 border-transparent pr-3', on && 'border-accent bg-accent-soft')}
            >
              <span className={cn('w-10 shrink-0 pr-3 text-right text-fg-subtle select-none', on && 'text-accent-text')}>{n}</span>
              <span className="min-w-0 whitespace-pre-wrap text-fg">{line}</span>
            </div>
          )
        })}
      </pre>
    </div>
  )
}
