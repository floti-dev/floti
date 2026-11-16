import type { JSX } from 'react'
import { cn } from '../lib/cn'
import { WarnIcon } from './icons'

/**
 * What the window says about this PC's host, when there is something to say:
 *
 * - `updating`: Floti is restarting it onto its own floti (`to`); its
 *   sessions carry over. Nothing to press.
 * - `refused`: it refused this Floti, saying why (`sentence`). Updating Floti
 *   fixes it where the host is the newer side (`update-floti`); anything
 *   else - a floti that cannot serve, an update that did not take - is tried
 *   again (`try-again`).
 * - `unreachable`: no host could be reached or started (`reason`).
 * - `lost`: it stopped without being asked to; Floti starts a new one
 *   (`restarts`) or waits for it to answer.
 *
 * `retrying` says Floti is trying it again by itself right now.
 */
export type HostNoticeState =
  | { kind: 'updating'; to: string }
  | { kind: 'refused'; sentence: string; fix: 'try-again' | 'update-floti'; retrying: boolean }
  | { kind: 'unreachable'; reason: string; retrying: boolean }
  | { kind: 'lost'; restarts: boolean }

export interface HostNoticeProps {
  /** The host, as the window names it: "this PC’s host". */
  hostName: string
  notice: HostNoticeState
  /** Try again: Floti connects to the host now rather than after its wait. */
  onTryAgain: () => void
  /** Where Floti is updated from: what a host newer than this Floti needs. */
  onUpdateFloti: () => void
}

/** The sentence a notice says, as its strip shows it. */
function noticeText(notice: HostNoticeState, hostName: string, host: string): string {
  switch (notice.kind) {
    case 'updating':
      return `Updating ${hostName} to floti ${notice.to}… Its sessions carry over.`
    case 'refused':
      return notice.sentence
    case 'unreachable':
      return `Floti could not reach ${hostName}. ${notice.reason}`
    case 'lost':
      return notice.restarts ? `${host} stopped. Starting it again…` : `Reconnecting to ${hostName}…`
  }
}

/**
 * The one place the window says this PC's host cannot be used, or is being
 * updated, or stopped: a strip above the tabs like the version guard, with
 * the action that fixes it as its button. Everything that waits on the host
 * says only that it waits. It goes by itself once the host answers, so it has
 * no close.
 */
export function HostNotice({ hostName, notice, onTryAgain, onUpdateFloti }: HostNoticeProps): JSX.Element {
  const host = hostName.charAt(0).toUpperCase() + hostName.slice(1)
  const action =
    notice.kind === 'refused' && notice.fix === 'update-floti'
      ? { label: 'Update Floti…', run: onUpdateFloti, busy: false, title: 'Opens Settings > Hosts' }
      : notice.kind === 'refused' || notice.kind === 'unreachable'
        ? {
            label: notice.retrying ? 'Trying again…' : 'Try again',
            run: onTryAgain,
            busy: notice.retrying,
            title: `Connect to ${hostName} now, rather than after the wait`
          }
        : null
  return (
    <div
      role="status"
      data-host-notice={notice.kind}
      className={cn(
        'flex shrink-0 items-center gap-2.5 rounded-raised border border-warn/30 bg-warn/10 px-3 py-1.5',
        'text-[12px] text-warn'
      )}
    >
      <WarnIcon width={13} height={13} className="shrink-0" />
      {/* It wraps rather than truncates: a refusal's sentence is the fix. */}
      <span data-host-notice-text className="min-w-0 flex-1 leading-[1.45] break-words">
        {noticeText(notice, hostName, host)}
      </span>
      {action !== null && (
        <button
          type="button"
          data-host-notice-action={notice.kind === 'refused' ? notice.fix : 'try-again'}
          onClick={action.run}
          disabled={action.busy}
          title={action.title}
          className={cn(
            'shrink-0 rounded-well border border-warn/40 px-2 py-0.5 text-[11.5px] text-warn',
            'transition-colors hover:bg-warn/15 disabled:opacity-60 disabled:hover:bg-transparent'
          )}
        >
          {action.label}
        </button>
      )}
    </div>
  )
}
