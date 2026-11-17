import type { JSX } from 'react'
import { cn } from '../lib/cn'
import { shortcutLabel } from '../lib/platform'
import { FlotiMarkIcon, TerminalIcon } from './icons'

export interface WelcomePaneProps {
  roots: string[]
  projectCount: number
  onAddRoot: () => void
  /** Scaffold a workspace. The same action first run offers, still reachable. */
  onCreateWorkspace?: (() => void) | undefined
  /** Open the new-session launcher - what an empty workspace is usually for. */
  onNewSession?: (() => void) | undefined
}

/** Shown when no project is selected. Names the roots being scanned so an empty
 * sidebar is diagnosable without opening settings. */
export function WelcomePane({
  roots,
  projectCount,
  onAddRoot,
  onCreateWorkspace,
  onNewSession
}: WelcomePaneProps): JSX.Element {
  return (
    <div
      data-welcome-pane
      // An island of its own, unlike every other page: it stands in for the
      // pane itself when there is none, rather than sitting inside one.
      className="grid h-full place-items-center rounded-island border border-border bg-surface px-8"
    >
      <div className="max-w-md text-center">
        {/* The mark at watermark scale, standing in for the wordmark that used
            to head this pane - the same argument the title bar's comment makes,
            with more force here: "Floti" set above the Floti mark says the name
            twice to someone already looking at the app.

            A faint silhouette - the solid ship at 35% of `fg-subtle`, about the
            hairline's value - because this is the empty state of the workspace
            column and the one thing on it that must read is the sentence saying
            what to do next. A watermark that wins that contest has become a
            splash screen.

            The alpha is on the **element**, not in the colour. CSS `opacity`
            composites the mark once, so it stays flat at any size whatever it
            is drawn from; a semi-transparent `currentColor` would be painted
            per shape, and a mark of overlapping shapes would darken where they
            cross. The mark is one path today, and this keeps it from mattering.

            Which leaves the colour opaque, and `fg-subtle` is the one token
            carrying the same value in both modes (#75798C, theme.css) - so a
            single opacity lands on the same composite either side: at 35%, the
            hairline the rest of the app draws its edges with.

            Vector, so at 192px it is drawn at 192px rather than resampled from
            the icon file's 256px image, which would be the softest thing on
            the pane. */}
        <FlotiMarkIcon
          width={192}
          height={192}
          role="img"
          aria-label="Floti"
          aria-hidden={false}
          className="mx-auto text-fg-subtle opacity-35"
        />
        <p className="mt-8 text-[13px] text-fg-muted">
          {projectCount === 0
            ? 'Point Floti at a folder to get started.'
            : onNewSession
              ? 'Start a session, or pick a project on the left.'
              : 'Pick a project on the left.'}
        </p>
        {projectCount > 0 && onNewSession && (
          <button
            type="button"
            onClick={onNewSession}
            className={cn(
              'mt-4 inline-flex items-center gap-2 rounded-well border border-accent px-3.5 py-1.5',
              'text-[12px] font-medium text-accent-text transition-colors hover:bg-accent-soft active:bg-active'
            )}
          >
            <TerminalIcon width={14} height={14} />
            New session
            <span className="font-mono text-[11px] font-normal text-fg-subtle">{shortcutLabel('N')}</span>
          </button>
        )}

        {roots.length > 0 && (
          <div className="mt-6 text-left">
            <p className="mb-1.5 text-[10px] font-semibold tracking-[.07em] text-fg-subtle uppercase">
              Scanning
            </p>
            <ul className="rounded-raised border border-border bg-surface-raised">
              {roots.map((root) => (
                <li
                  key={root}
                  className={cn(
                    'truncate border-b border-border px-3 py-2 font-mono text-[11px] text-fg-muted',
                    'last:border-b-0'
                  )}
                  title={root}
                >
                  {root}
                </li>
              ))}
            </ul>
          </div>
        )}

        <div className="mt-5 flex flex-wrap justify-center gap-2">
          <button
            type="button"
            onClick={onAddRoot}
            className="rounded-well border border-border-strong px-3 py-1.5 text-[12px] text-fg transition-colors hover:bg-hover"
          >
            Add a folder
          </button>
          {onCreateWorkspace && (
            <button
              type="button"
              data-welcome-create-workspace
              onClick={onCreateWorkspace}
              className="rounded-well border border-border-strong px-3 py-1.5 text-[12px] text-fg transition-colors hover:bg-hover"
            >
              Create a workspace
            </button>
          )}
        </div>
      </div>
    </div>
  )
}
