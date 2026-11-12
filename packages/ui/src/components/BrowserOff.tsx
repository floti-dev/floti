import { useState, type JSX } from 'react'
import { EmptyState } from './EmptyState'
import { CopyIcon, GlobeIcon } from './icons'

export interface BrowserOffProps {
  /**
   * The one command that gives this Helm Chromium's sandbox, or null when
   * Helm could not write one for this computer.
   */
  command: string | null
  /** Puts the command on the clipboard; true once it is there. */
  onCopy: (command: string) => Promise<boolean>
  /** Opens the packaging notes on the sandbox in the user's own browser. */
  onReadMore: () => void
}

const BUTTON =
  'flex h-7 items-center gap-1.5 rounded-well border border-border-strong px-2.5 text-[12px] text-fg transition-colors hover:bg-hover'

/**
 * What the Browser tab shows instead of a page when Helm runs without
 * Chromium's sandbox (DESIGN.md 5, "Empty states"): the pane's icon, what is
 * off, one sentence on why, and the way on - the command that turns the
 * sandbox on, in mono because it is machine text, with a button to copy it.
 */
export function BrowserOff({ command, onCopy, onReadMore }: BrowserOffProps): JSX.Element {
  const [copied, setCopied] = useState(false)
  return (
    <EmptyState
      name="browser-off"
      icon={<GlobeIcon width={18} height={18} />}
      title="The browser is off"
      actions={
        <>
          {command !== null && (
            <code className="block w-full basis-full select-text break-all rounded-well border border-border bg-surface-raised px-2.5 py-2 text-left font-mono text-[11px] leading-relaxed text-fg">
              {command}
            </code>
          )}
          {command !== null && (
            <button
              type="button"
              className={BUTTON}
              onClick={() => void onCopy(command).then(setCopied, () => setCopied(false))}
            >
              <CopyIcon width={12} height={12} />
              {copied ? 'Copied' : 'Copy command'}
            </button>
          )}
          <button type="button" className={BUTTON} onClick={onReadMore}>
            How this works
          </button>
        </>
      }
    >
      {command === null
        ? "Helm is running without Chromium's sandbox, so it opens no web page and gives sessions no browser tools. Start it with the sandbox to turn the browser on."
        : "Helm is running without Chromium's sandbox, so it opens no web page and gives sessions no browser tools. Run this once in a terminal, then start Helm again:"}
    </EmptyState>
  )
}
