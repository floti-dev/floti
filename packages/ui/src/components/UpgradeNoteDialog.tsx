import type { JSX, ReactNode } from 'react'
import { useEffect, useRef } from 'react'
import { cn } from '../lib/cn'
import { primaryButton } from './DialogParts'
import { CheckIcon, FolderIcon, MonitorIcon } from './icons'
import { Overlay } from './Overlay'

export interface UpgradeNoteDialogProps {
  /** Got it, Escape or the scrim: the note is closed for good. */
  onClose: () => void
  /** Opens the release notes; the note is closed for good with it. */
  onWhatsNew: () => void
}

/** The note's title, which is also its accessible name. */
export const UPGRADE_NOTE_TITLE = 'Helm is now Floti'

interface NoteItem {
  key: string
  icon: ReactNode
  lead: string
  rest: ReactNode
}

const ICON = { width: 14, height: 14, className: 'mt-[2px] shrink-0 text-fg-subtle' }

const ITEMS: readonly NoteItem[] = [
  {
    key: 'workspaces',
    icon: <FolderIcon {...ICON} />,
    lead: 'Harnesses are now called workspaces.',
    rest: (
      <>
        Nothing in your folders changed: a folder with <code className="font-mono text-[11.5px]">harness.yaml</code>{' '}
        works as before, and new ones get <code className="font-mono text-[11.5px]">workspace.yaml</code>.
      </>
    )
  },
  {
    key: 'kept',
    icon: <CheckIcon {...ICON} />,
    lead: 'Everything came with you:',
    rest: 'sessions, history, profiles, templates, plugins and settings.'
  },
  {
    key: 'host',
    icon: <MonitorIcon {...ICON} />,
    lead: 'Sessions run in the host now.',
    rest: 'Quitting still ends the ones you started here; Settings > Sessions can keep them running instead.'
  }
]

/**
 * What someone coming from an earlier Helm is told once, on the first start
 * after the update, over their own sessions (Floti Onboarding, "Upgrade
 * note"). It says what was renamed and that nothing in their folders was:
 * Floti never renames or rewrites a file of theirs for the new names.
 *
 * Closing it in any way closes it for good, What's new included: the release
 * notes open in the Browser tab, whose page a dialog left over it would hide.
 * Got it has the focus, so Enter closes it.
 */
export function UpgradeNoteDialog({ onClose, onWhatsNew }: UpgradeNoteDialogProps): JSX.Element {
  const gotItRef = useRef<HTMLButtonElement>(null)
  useEffect(() => {
    gotItRef.current?.focus()
  }, [])

  return (
    <Overlay aria-label={UPGRADE_NOTE_TITLE} data-upgrade-note className="max-w-[420px]" onDismiss={onClose}>
      <header className="px-4 pt-[14px] pb-[10px]">
        <h2 className="text-[15px] font-medium tracking-tight text-fg">{UPGRADE_NOTE_TITLE}</h2>
      </header>
      <ul className="grid gap-2.5 px-4 pt-1 pb-[14px]">
        {ITEMS.map((item) => (
          <li
            key={item.key}
            data-upgrade-note-item={item.key}
            className="flex gap-2.5 text-[12.5px] leading-[1.5] text-fg-muted"
          >
            {item.icon}
            <span className="min-w-0">
              <span className="font-medium text-fg">{item.lead}</span> {item.rest}
            </span>
          </li>
        ))}
      </ul>
      <footer className="flex items-center justify-end gap-2 px-4 pt-3 pb-[14px]">
        <button
          type="button"
          data-upgrade-note-whats-new
          onClick={onWhatsNew}
          className="rounded-well px-2 py-1.5 text-[12px] text-fg-muted transition-colors hover:bg-hover hover:text-fg"
        >
          What’s new
        </button>
        <button
          ref={gotItRef}
          type="button"
          data-upgrade-note-close
          onClick={onClose}
          className={cn(
            primaryButton(true),
            // `:focus`, as ConfirmSessionDialog's Cancel: focus placed by
            // script is not visible focus to Chromium, and the button Enter
            // presses has to look like it.
            'focus:outline-2 focus:outline-offset-2 focus:outline-accent'
          )}
        >
          Got it
        </button>
      </footer>
    </Overlay>
  )
}
