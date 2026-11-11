import { useId, useState, type JSX } from 'react'
import type { RestorableSession, RestoreOffer } from '@helm/core/types'
import { cn } from '../lib/cn'
import { folderName } from '../lib/launcher'
import { formatMoment } from '../lib/time'
import { AgentTag } from './AgentTag'
import { Checkbox } from './Checkbox'
import { HistoryIcon } from './icons'

export interface RestorePaneProps {
  offer: RestoreOffer
  /** The \`restoreWithoutAsking\` setting, ticked here or in Settings. */
  withoutAsking: boolean
  onWithoutAskingChange: (next: boolean) => void
  /** True while the reopening is under way. */
  busy: boolean
  /** The ticked sessions, in the order they are listed. */
  onResume: (picks: RestorePick[]) => void
  onNotNow: () => void
  /** The clock, for the time column. */
  now: number
  /**
   * The sessions ticked when the pane opens, such as the one a lost tab's
   * Restore asked for; every one that can come back when not given.
   */
  preselected?: readonly number[]
  /**
   * The host the sessions went with, as the window names it ("this PC’s
   * host"), when they ran in one; absent when they went with Helm.
   */
  lostWith?: string
}

/** One session to reopen: its row id, and whether it comes back without the agent it was started as. */
export interface RestorePick {
  id: number
  /**
   * Its agent cannot be composed again (`agentProblem`) and the person ticked
   * it anyway: it comes back without its agent.
   */
  withoutAgent?: true
}

const plural = (count: number, one: string, many: string): string =>
  `${String(count)} ${count === 1 ? one : many}`

/**
 * When a session was last spoken to, in the width its column has: the time
 * for today, which is the case after a crash, and the date for anything older.
 */
function lastSaid(at: number, now: number): string {
  const when = new Date(at)
  return when.toDateString() === new Date(now).toDateString()
    ? when.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })
    : when.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}

/**
 * What a crash took, offered back: every session Helm, or the host they ran
 * in, was running when it stopped, ticked, and the one button that reopens
 * them where they were.
 *
 * A page in a pane rather than a dialog, because it is the first thing a
 * start after a crash has to say and somebody may want to look at the rest of
 * the window before answering. Closing its tab is "not now", the same as the
 * button.
 *
 * A session that cannot come back is listed anyway, unticked and greyed with
 * the reason - one quietly missing from the list would read as Helm having
 * forgotten it.
 */
export function RestorePane({
  offer,
  withoutAsking,
  onWithoutAskingChange,
  busy,
  onResume,
  onNotNow,
  now,
  preselected,
  lostWith
}: RestorePaneProps): JSX.Element {
  const headingId = useId()
  // One whose agent cannot be composed again starts unticked: it would come
  // back without the agent it was started as, which is the person's to choose.
  const [ticked, setTicked] = useState<ReadonlySet<number>>(
    () =>
      new Set(
        offer.sessions
          .filter(
            (session) =>
              session.blocked === null &&
              session.agentProblem === undefined &&
              (preselected === undefined || preselected.includes(session.id))
          )
          .map((session) => session.id)
      )
  )
  const chosen: RestorePick[] = offer.sessions
    .filter((session) => ticked.has(session.id))
    .map((session) =>
      session.agentProblem === undefined ? { id: session.id } : { id: session.id, withoutAgent: true }
    )

  const toggle = (id: number): void =>
    setTicked((current) => {
      const next = new Set(current)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })

  const count = offer.sessions.length
  // What stopped: Helm, which closes, or the host the sessions ran in, which stops.
  const stopped = lostWith === undefined ? 'Helm closed' : `${lostWith} stopped`
  const who = lostWith === undefined ? 'Helm' : lostWith.charAt(0).toUpperCase() + lostWith.slice(1)
  const elsewhere =
    offer.elsewhere === 0
      ? null
      : `${plural(offer.elsewhere, 'more is', 'more are')} still running somewhere else, and left alone.`

  return (
    <div
      data-restore-pane
      aria-labelledby={headingId}
      role="region"
      className="h-full overflow-auto px-6"
    >
      <div className="mx-auto w-full max-w-[560px] pt-24 pb-10">
        <div className="mb-1.5 flex items-center gap-2.5">
          <HistoryIcon width={20} height={20} className="shrink-0 text-accent-text" />
          <h1 id={headingId} className="m-0 text-[18px] font-medium tracking-[-0.01em] text-fg">
            {count === 1
              ? `1 session was running when ${stopped}`
              : `${String(count)} sessions were running when ${stopped}`}
          </h1>
        </div>
        <p className="mt-0 mb-[18px] ml-[30px] leading-normal text-fg-muted">
          {who} didn’t shut down cleanly. Pick up where they left off.
        </p>

        <ul
          aria-label="Sessions to reopen"
          className="m-0 list-none overflow-hidden rounded-raised border border-border bg-surface-raised p-0"
        >
          {offer.sessions.map((session) => (
            <Row
              key={session.id}
              session={session}
              on={ticked.has(session.id)}
              onToggle={() => toggle(session.id)}
              now={now}
            />
          ))}
        </ul>

        <p className="mt-3 mb-[18px] text-[11.5px] leading-normal text-fg-subtle">
          Each one runs <span className="font-mono text-fg-muted">claude --resume</span> in its own
          folder with its own profile and agent, in the tab it had. Not now keeps them at the top of History (Ctrl+H).
          {elsewhere !== null && <> {elsewhere}</>}
        </p>

        <div className="flex items-center gap-2">
          <label className="flex cursor-pointer items-center gap-2 text-[12px] text-fg-muted">
            <Checkbox
              checked={withoutAsking}
              onChange={() => onWithoutAskingChange(!withoutAsking)}
              label="Always resume without asking"
              mark="data-restore-always"
            />
            Always resume without asking
          </label>
          <span className="flex-1" />
          <button
            type="button"
            onClick={onNotNow}
            disabled={busy}
            className="rounded-well border border-border-strong px-3.5 py-1.5 text-[12.5px] text-fg transition-colors hover:bg-hover disabled:cursor-default disabled:opacity-50"
          >
            Not now
          </button>
          <button
            type="button"
            data-restore-resume
            onClick={() => onResume(chosen)}
            disabled={busy || chosen.length === 0}
            className={cn(
              'rounded-well border border-accent px-4 py-1.5 text-[12.5px] font-medium text-accent-text',
              'transition-colors hover:bg-accent-soft active:bg-active',
              'disabled:cursor-default disabled:border-border-strong disabled:text-fg-subtle disabled:hover:bg-transparent'
            )}
          >
            {busy ? 'Resuming…' : `Resume ${plural(chosen.length, 'session', 'sessions')}`}
          </button>
        </div>
      </div>
    </div>
  )
}

function Row({
  session,
  on,
  onToggle,
  now
}: {
  session: RestorableSession
  on: boolean
  onToggle: () => void
  now: number
}): JSX.Element {
  const blocked = session.blocked !== null
  const where = [folderName(session.cwd), session.branch].filter((part) => part !== null).join(' · ')
  const agent = session.agent === undefined || session.agent === '' ? undefined : session.agent
  return (
    <li
      data-restore-row={session.id}
      {...(blocked ? { 'data-restore-blocked': '' } : {})}
      {...(session.agentProblem === undefined ? {} : { 'data-restore-agent-problem': '' })}
      className="border-b border-border last:border-b-0"
    >
      <label
        title={session.cwd}
        className={cn(
          'flex h-[46px] items-center gap-3 px-3.5',
          blocked ? 'cursor-default' : 'cursor-pointer hover:bg-hover'
        )}
      >
        <Checkbox checked={on} onChange={onToggle} label={session.title ?? session.name} disabled={blocked} />
        <span className="flex min-w-0 flex-1 flex-col gap-0.5">
          <span className="flex min-w-0 items-center gap-2">
            <span className={cn('min-w-0 truncate', on ? 'text-fg' : 'text-fg-muted')}>{session.title ?? session.name}</span>
            {agent !== undefined && <AgentTag name={agent} gone={session.agentProblem !== undefined} />}
          </span>
          {blocked ? (
            <span className="truncate text-[11px] text-fg-subtle">{session.blocked}</span>
          ) : session.agentProblem !== undefined ? (
            // The host's sentence naming what is wrong with its agent, and what
            // ticking it does about that.
            <span title={session.agentProblem} className="truncate text-[11px] text-warn">
              {session.agentProblem} Restored, it comes back without its agent.
            </span>
          ) : (
            <span className="truncate font-mono text-[11px] text-fg-subtle">{where}</span>
          )}
        </span>
        {session.profileGone ? (
          <span className="shrink-0 text-[11.5px] text-warn">Profile deleted</span>
        ) : (
          session.profile !== null && (
            <span className="shrink-0 text-[11.5px] text-fg-subtle">{session.profile}</span>
          )
        )}
        <span
          className="w-16 shrink-0 text-right text-[11.5px] text-fg-subtle tabular-nums"
          title={session.lastAt === null ? undefined : formatMoment(session.lastAt)}
        >
          {session.lastAt === null ? '' : lastSaid(session.lastAt, now)}
        </span>
      </label>
    </li>
  )
}
