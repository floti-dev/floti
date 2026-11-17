import type { JSX, ReactNode } from 'react'
import { cn } from '../lib/cn'
import { BUTTON_PRIMARY, BUTTON_SECONDARY } from '../lib/page'

/** The one button an empty state offers: what fills the surface, or Try again where it could not look. */
export interface EmptyStateAction {
  label: string
  onClick: () => void
  /** The shortcut that does the same, drawn after the label in mono ("Ctrl+N"). */
  keys?: string | undefined
  /** An icon before the label, 12px: Try again's refresh. */
  icon?: ReactNode | undefined
  /** Outlined in `border-strong` rather than the accent: Try again, which fills nothing. */
  secondary?: boolean | undefined
}

export interface EmptyStateProps {
  /** The destination's own icon, 18px. */
  icon: ReactNode
  /** What is empty, in a few words: "No sessions running". */
  title: string
  /** One sentence on what fills it, or why it is empty. */
  children?: ReactNode | undefined
  /** The way on: the one button that fills the surface. */
  action?: EmptyStateAction | undefined
  /** Anything else the way on needs, after `action`: a command to copy, a second link. */
  actions?: ReactNode | undefined
  /**
   * `page` fills a detail region and centres in it; `list` sits at the top of a
   * list column, where the eye already is.
   */
  size?: 'page' | 'list' | undefined
  /** Names it for a test or a driver. */
  name?: string | undefined
}

/**
 * What a page or a list says when it has nothing to show (DESIGN.md 5, "Empty
 * states"): the destination's icon in a small well, a title saying what is
 * empty, one sentence, and the way on as a button where there is one.
 *
 * Short on purpose. The empty states this replaced were two paragraphs each,
 * explaining the feature to somebody looking at nothing - and a paragraph in an
 * empty pane is read once and skipped every time after.
 */
export function EmptyState({ icon, title, children, action, actions, size = 'page', name }: EmptyStateProps): JSX.Element {
  return (
    <div
      data-empty-state={name ?? ''}
      className={cn('flex justify-center text-center', size === 'page' ? 'h-full items-center p-8' : 'px-4 py-8')}
    >
      <div className="flex max-w-[360px] flex-col items-center">
        <span
          aria-hidden
          className="grid size-9 place-items-center rounded-well border border-border bg-surface-raised text-fg-subtle"
        >
          {icon}
        </span>
        <p className="mt-3 text-[13px] font-medium text-fg">{title}</p>
        {children !== undefined && (
          <p className="mt-1 text-[12px] leading-relaxed text-fg-subtle">{children}</p>
        )}
        {(action !== undefined || actions !== undefined) && (
          <div className="mt-4 flex flex-wrap justify-center gap-2">
            {action !== undefined && (
              <button
                type="button"
                data-empty-action
                onClick={action.onClick}
                className={action.secondary === true ? BUTTON_SECONDARY : BUTTON_PRIMARY}
              >
                {action.icon}
                {action.label}
                {action.keys !== undefined && (
                  <span aria-hidden className="font-mono text-[11px] text-fg-subtle">
                    {action.keys}
                  </span>
                )}
              </button>
            )}
            {actions}
          </div>
        )}
      </div>
    </div>
  )
}
