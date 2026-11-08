import type { JSX } from 'react'
import { useEffect, useRef } from 'react'
import { cn } from '../lib/cn'
import { Overlay } from './Overlay'

export interface DeleteProfileDialogProps {
  /** The profile's name, as the question names it. */
  name: string
  onConfirm: () => void
  onCancel: () => void
}

/** What the question says under the name, wherever a profile is deleted from. */
const DELETE_PROFILE_DETAIL =
  'This cannot be undone. Sessions already launched from it keep running, and ' +
  'exporting it first writes a YAML file you can import back.'

/**
 * The question before a profile is deleted.
 *
 * A profile is a small thing, but it is one the user assembled by hand in a
 * form, there is no undo, and the button that asks for this sits on a row
 * that shows its actions on hover, beside the one that edits. So it asks, and
 * the sentence offers the export, because that is the answer to "I wanted to
 * keep that".
 *
 * Cancel holds the focus and is what Enter presses; Escape and the scrim mean
 * no as well. Deleting is its own button, chosen on purpose.
 */
export function DeleteProfileDialog({ name, onConfirm, onCancel }: DeleteProfileDialogProps): JSX.Element {
  const cancelRef = useRef<HTMLButtonElement>(null)
  const message = `Delete “${name}”?`

  useEffect(() => {
    cancelRef.current?.focus()
  }, [])

  return (
    <Overlay
      role="alertdialog"
      aria-label={message}
      data-delete-profile
      className="max-w-[440px]"
      onDismiss={onCancel}
    >
      <div className="flex flex-col gap-[10px] px-[18px] pt-4 pb-[14px]">
        <h2 className="text-[14px] leading-[1.4] font-medium text-fg">{message}</h2>
        <p className="text-[12.5px] leading-[1.5] text-fg-muted">{DELETE_PROFILE_DETAIL}</p>
        <footer className="mt-1 flex shrink-0 items-center justify-end gap-[6px]">
          <button
            ref={cancelRef}
            type="button"
            data-delete-profile-cancel
            onClick={onCancel}
            className={cn(
              'rounded-well border border-border-strong px-3 py-1 text-[12px] font-medium text-fg',
              'transition-colors hover:bg-hover',
              // `:focus`, as ConfirmSessionDialog's Cancel: focus placed by
              // script is not visible focus to Chromium, and the button Enter
              // would press has to look like it.
              'focus:outline-2 focus:outline-offset-2 focus:outline-accent'
            )}
          >
            Cancel
          </button>
          <button
            type="button"
            data-delete-profile-confirm
            onClick={onConfirm}
            // Danger in the text and the hover wash, never a solid fill (DESIGN.md).
            className="rounded-well border border-danger/50 px-3 py-1 text-[12px] font-medium text-danger transition-colors hover:bg-danger/10"
          >
            Delete profile
          </button>
        </footer>
      </div>
    </Overlay>
  )
}
