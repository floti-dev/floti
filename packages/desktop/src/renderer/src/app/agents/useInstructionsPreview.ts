import { useEffect, useRef, useState } from 'react'
import type { RenderedMarkdown } from '@helm/core/types'
import { bridgeFor } from '../bridge'

/** How long typing pauses before the preview asks again: the Files view's split preview's pause. */
export const INSTRUCTIONS_PREVIEW_PAUSE_MS = 160

export interface InstructionsPreview {
  /** The instructions as the host last rendered them; null until the first render. */
  rendered: RenderedMarkdown | null
  /** Whether `rendered` is of other text than the instructions are now. */
  pending: boolean
}

/**
 * An agent's instructions as its host renders them (`agents:render`), for the
 * live preview beside the editor: asked once typing pauses, and only the
 * newest answer is kept, so a slow render never paints over a newer one. A
 * render that fails keeps the last preview, marked as behind.
 */
export function useInstructionsPreview(host: string, text: string): InstructionsPreview {
  const [asked, setAsked] = useState(text)
  const [shown, setShown] = useState<{ text: string; rendered: RenderedMarkdown } | null>(null)
  const ticket = useRef(0)

  useEffect(() => {
    const timer = setTimeout(() => setAsked(text), INSTRUCTIONS_PREVIEW_PAUSE_MS)
    return () => clearTimeout(timer)
  }, [text])

  useEffect(() => {
    const mine = ++ticket.current
    bridgeFor(host)
      .invoke('agents:render', { text: asked })
      .then((rendered) => {
        if (mine === ticket.current) setShown({ text: asked, rendered })
      })
      .catch(() => undefined)
  }, [host, asked])

  return { rendered: shown?.rendered ?? null, pending: shown === null || shown.text !== text }
}
