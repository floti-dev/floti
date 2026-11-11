import type { JSX } from 'react'
import { cn } from '../lib/cn'
import { CloseIcon } from './icons'
import { NOTICE_BUTTON, NOTICE_PLAIN } from './NeedsYouToast'

export interface UndoToastProps {
  /** What was done, such as "Deleted triage". */
  title: string
  /** What Undo does, in a sentence. */
  detail: string | null
  /** Whether Undo is on its way: the button waits. */
  undoing: boolean
  /** Why Undo did not work, as a sentence. */
  error: string | null
  onUndo: () => void
  onClose: () => void
}

/**
 * A change that was made without asking, with Undo (the design's A14). It
 * stays until it is closed or the next one replaces it: no timer takes the
 * Undo away from somebody who looked away. Drawn in the window's one stack of
 * notices at its left edge, where no browser view reaches.
 */
export function UndoToast({ title, detail, undoing, error, onUndo, onClose }: UndoToastProps): JSX.Element {
  return (
    <div
      role="status"
      data-undo-toast={title}
      className="pointer-events-auto flex w-[330px] max-w-full flex-col gap-1.5 rounded-well border border-border-strong bg-surface-raised px-3 py-[11px]"
    >
      <div className="flex items-center gap-2 text-[12.5px] font-medium text-fg">
        <span className="min-w-0 truncate">{title}</span>
        <button
          type="button"
          aria-label="Close notification"
          onClick={onClose}
          className="ml-auto grid size-[22px] shrink-0 place-items-center rounded-well text-fg-subtle transition-colors hover:bg-hover hover:text-fg"
        >
          <CloseIcon width={11} height={11} />
        </button>
      </div>
      {detail !== null && <p className="m-0 text-[12px] leading-[1.45] text-fg-muted">{detail}</p>}
      {error !== null && <p className="m-0 text-[12px] leading-[1.45] text-danger">{error}</p>}
      <div className="flex gap-1.5">
        <button
          type="button"
          data-toast-undo
          disabled={undoing}
          onClick={onUndo}
          className={cn(NOTICE_BUTTON, NOTICE_PLAIN, undoing && 'opacity-60')}
        >
          {undoing ? 'Undoing…' : 'Undo'}
        </button>
      </div>
    </div>
  )
}
