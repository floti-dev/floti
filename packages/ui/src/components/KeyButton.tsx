import type { JSX, ReactNode } from 'react'
import { cn } from '../lib/cn'

/**
 * A hint in a palette's key strip that is also the control it names: the key
 * in mono, the word beside it. The strip's main one (`main`) is in accent
 * text. The launcher's strip and History's are made of these.
 */
export function KeyButton({
  keys,
  onClick,
  disabled = false,
  main = false,
  children
}: {
  keys: string
  onClick: () => void
  disabled?: boolean
  main?: boolean
  children: ReactNode
}): JSX.Element {
  return (
    <button
      type="button"
      onClick={onClick}
      onMouseDown={(event) => event.preventDefault()}
      disabled={disabled}
      data-key-button={keys}
      className={cn(
        'flex h-[24px] shrink-0 items-center gap-1.5 rounded-raised px-1.5 transition-colors',
        'hover:bg-hover hover:text-fg disabled:cursor-default disabled:opacity-50 disabled:hover:bg-transparent',
        main && 'text-accent-text'
      )}
    >
      <span className="font-mono text-fg-muted">{keys}</span>
      {children}
    </button>
  )
}
