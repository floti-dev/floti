import { useEffect, useState, type JSX } from 'react'
import { cn } from '../lib/cn'
import { CheckIcon } from './icons'
import type { LocalStartupStep } from './SessionTree'

/**
 * Where main's link to this PC's host stands, as the start-up view reads it:
 * the desktop's `LocalHostStep` (`shared/ipc.ts`), narrowed to what is drawn.
 * `since` is when the step began, in milliseconds on the same clock as
 * `Date.now()`; `to` is the floti version an update restarts the host onto.
 */
export type StartupStep =
  | { step: 'preparing' | 'connecting' | 'starting' | 'ready' | 'refused' | 'unreachable'; since: number }
  | { step: 'updating'; since: number; to: string }

/** How long a step runs before the view says what it is waiting on. */
export const LONG_STEP_MS = 4000

/** What the main area shows in place of its panes, if anything (`startupView`). */
export type StartupView = 'steps' | 'waiting' | null

/**
 * What the main area shows in place of its panes:
 *
 * - `steps` (`StartupSteps`) from the first frame (before main has said a
 *   step, too) until the window's layout has arrived. `ready` keeps them up
 *   until the panes they hand over to can be drawn, so no empty pane flashes
 *   between them.
 * - `waiting` (`StartupWaiting`) while this PC's host cannot be used (it
 *   refused this Floti, or could not be reached; `hostUnusable` stays through
 *   main's retries, whose steps would otherwise bring the steps back) and
 *   there are no panes to draw: the layout has not arrived, or it holds no
 *   tab, where the welcome page would offer to add folders the host cannot
 *   read. Why it cannot be used is the host notice's to say, once.
 * - null: the panes, or the welcome page when there are none.
 */
export function startupView(
  step: StartupStep | null,
  { layoutArrived, layoutEmpty, hostUnusable }: { layoutArrived: boolean; layoutEmpty: boolean; hostUnusable: boolean }
): StartupView {
  if (hostUnusable || step?.step === 'refused' || step?.step === 'unreachable') {
    return !layoutArrived || layoutEmpty ? 'waiting' : null
  }
  return layoutArrived ? null : 'steps'
}

/**
 * The start-up step the Sessions tree's line for this PC says
 * (`SessionTreeProps.localHostStarting`): preparing, connecting or starting,
 * and preparing before main has said a step, since a connect begins with it.
 * Null for every other step, which the tree says in its own way.
 */
export function startupTreeStep(step: StartupStep | null): LocalStartupStep | null {
  if (step === null) return 'preparing'
  return step.step === 'preparing' || step.step === 'connecting' || step.step === 'starting' ? step.step : null
}

type RowKey = 'preparing' | 'connecting' | 'starting' | 'updating'

interface Row {
  key: RowKey
  label: string
  state: 'done' | 'current' | 'next'
}

/** What each step says it is waiting on once it has run past `LONG_STEP_MS`. */
const WAITING_ON: Record<RowKey, string> = {
  preparing: 'Copying floti into place',
  connecting: 'Waiting for this PC’s host to answer',
  starting: 'Waiting for floti host to start',
  updating: 'Waiting for the old host to hand over its sessions'
}

function labelOf(key: RowKey, to: string | null): string {
  switch (key) {
    case 'preparing':
      return 'Preparing floti'
    case 'connecting':
      return 'Connecting to this PC’s host'
    case 'starting':
      return 'Starting floti host'
    case 'updating':
      return to === null ? 'Updating this PC’s host' : `Updating this PC’s host to floti ${to}`
  }
}

/**
 * The rows for a step. Every connect prepares floti and then connects; after
 * that it either starts a host, because nothing answered, or updates the one
 * that did, because it was older. So preparing and connecting are always
 * listed, and starting or updating once it is the step, or once it was
 * (`after`, kept by the view so the row stays, checked, when `ready` comes).
 * Before main has said a step, preparing is the one under way: it is what a
 * connect begins with.
 */
function rowsFor(step: StartupStep | null, after: { key: 'starting' | 'updating'; to: string | null } | null): Row[] {
  const current: RowKey | null =
    step === null
      ? 'preparing'
      : step.step === 'preparing' || step.step === 'connecting' || step.step === 'starting' || step.step === 'updating'
        ? step.step
        : null
  const order: RowKey[] = ['preparing', 'connecting']
  if (after !== null) order.push(after.key)
  const at = current === null ? order.length : order.indexOf(current)
  return order.map((key, index) => ({
    key,
    label: labelOf(key, key === after?.key ? after.to : null),
    state: index < at ? 'done' : index === at ? 'current' : 'next'
  }))
}

/** A starting or updating step, which the view keeps listing once it has been the step. */
function laterStepOf(step: StartupStep | null): { key: 'starting' | 'updating'; to: string | null } | null {
  if (step?.step === 'starting') return { key: 'starting', to: null }
  if (step?.step === 'updating') return { key: 'updating', to: step.to }
  return null
}

/** Milliseconds the current step has run, ticking each second while one runs. */
function useElapsed(since: number | null): number {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (since === null) return undefined
    const timer = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(timer)
  }, [since])
  return since === null ? 0 : Math.max(0, now - since)
}

/** A quarter arc turning on a faint ring: the step under way (DESIGN.md, the page tab's spinner). */
function Spinner(): JSX.Element {
  return (
    <svg
      aria-hidden="true"
      width={13}
      height={13}
      viewBox="0 0 16 16"
      focusable="false"
      className="block shrink-0 animate-spin motion-reduce:animate-none"
    >
      <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" strokeOpacity="0.25" strokeWidth="1.5" />
      <path d="M8 2a6 6 0 0 1 6 6" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
    </svg>
  )
}

/** The faint ring alone: a step still to come. */
function Ring(): JSX.Element {
  return (
    <svg aria-hidden="true" width={13} height={13} viewBox="0 0 16 16" focusable="false" className="block shrink-0">
      <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" strokeOpacity="0.25" strokeWidth="1.5" />
    </svg>
  )
}

export interface StartupWaitingProps {
  /** The host it waits for, as the window names it ("this PC’s host"). */
  hostName: string
}

/**
 * The main area while this PC's host cannot be used and there are no panes
 * to draw: one quiet line saying it waits, in the island the steps were in.
 * The host notice above says why and offers the fix, so this repeats neither.
 */
export function StartupWaiting({ hostName }: StartupWaitingProps): JSX.Element {
  return (
    <div
      data-startup-waiting-pane
      className="grid h-full place-items-center rounded-island border border-border bg-surface px-8"
    >
      <div role="status" className="max-w-xs text-center">
        <p className="text-[13px] text-fg-muted">Waiting for {hostName}&hellip;</p>
        <p className="mt-1 text-[12px] text-fg-subtle">Your projects and sessions show here once it answers.</p>
      </div>
    </div>
  )
}

export interface StartupStepsProps {
  /** Main's step, or null before it has said one. */
  step: StartupStep | null
}

/**
 * What Floti is doing between the window's first frame and the host
 * answering, in place of the empty panes: the start-up steps, those done
 * checked and the one under way turning. A step that runs past
 * `LONG_STEP_MS` says what it is waiting on and for how many seconds, so a
 * slow start reads as slow, not hung. An island of its own, as the welcome
 * pane is: it stands in for the panes while there are none.
 */
export function StartupSteps({ step }: StartupStepsProps): JSX.Element {
  // A starting or updating step stays listed once it has been the step, so
  // its row is checked rather than gone when `ready` comes.
  const later = laterStepOf(step)
  const [after, setAfter] = useState(later)
  if (later !== null && (after === null || later.key !== after.key || later.to !== after.to)) setAfter(later)
  const rows = rowsFor(step, later ?? after)
  const running = rows.find((row) => row.state === 'current')
  const elapsed = useElapsed(running !== undefined && step !== null ? step.since : null)
  const long = running !== undefined && step !== null && elapsed >= LONG_STEP_MS

  return (
    <div
      data-startup-steps={step?.step ?? 'none'}
      className="grid h-full place-items-center rounded-island border border-border bg-surface px-8"
    >
      <div role="status" aria-label="Starting Floti" className="w-full max-w-xs">
        <p className="mb-3 text-[10px] font-semibold tracking-[.07em] text-fg-subtle uppercase">Starting Floti</p>
        <ol className="flex flex-col gap-2.5">
          {rows.map((row) => (
            <li
              key={row.key}
              data-startup-step={row.key}
              data-state={row.state}
              aria-current={row.state === 'current' ? 'step' : undefined}
              className="flex items-start gap-2.5"
            >
              <span
                className={cn(
                  'mt-[3px]',
                  row.state === 'done' ? 'text-success' : row.state === 'current' ? 'text-fg-muted' : 'text-fg-subtle'
                )}
              >
                {row.state === 'done' ? (
                  <CheckIcon width={13} height={13} className="block shrink-0" />
                ) : row.state === 'current' ? (
                  <Spinner />
                ) : (
                  <Ring />
                )}
              </span>
              <span className="min-w-0 flex-1">
                <span
                  className={cn(
                    'block text-[13px] leading-[1.5]',
                    row.state === 'current' ? 'text-fg' : row.state === 'done' ? 'text-fg-muted' : 'text-fg-subtle'
                  )}
                >
                  {row.label}
                </span>
                {row.state === 'current' && long && (
                  <span data-startup-waiting className="block text-[12px] text-fg-subtle">
                    {WAITING_ON[row.key]}
                    {/* The seconds tick; a screen reader hears what it waits on, not every second. */}
                    <span aria-hidden="true">
                      {' · '}
                      <span className="font-mono text-[11px] tabular-nums">{Math.floor(elapsed / 1000)} s</span>
                    </span>
                  </span>
                )}
              </span>
            </li>
          ))}
        </ol>
      </div>
    </div>
  )
}
