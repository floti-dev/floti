import { useCallback, useState } from 'react'
import { bridgeFor } from '../bridge'
import { readable } from '../errors'

/** A change to an agent made without asking, with its Undo: null once Undo put it back, else why not. */
export interface AgentToast {
  key: number
  title: string
  detail: string | null
  undo: () => Promise<string | null>
  undoing: boolean
  error: string | null
}

export interface AgentToastState {
  /** The one toast, until it is closed or the next replaces it; nothing closes it on a timer. */
  toast: AgentToast | null
  raise: (title: string, detail: string | null, undo: () => Promise<string | null>) => void
  /** Runs the toast's Undo: it goes once that worked, and says why when it did not. */
  runUndo: () => Promise<void>
  close: () => void
}

export function useAgentToast(): AgentToastState {
  const [toast, setToast] = useState<AgentToast | null>(null)

  const raise = useCallback(
    (title: string, detail: string | null, undo: () => Promise<string | null>): void =>
      setToast((current) => ({ key: (current?.key ?? 0) + 1, title, detail, undo, undoing: false, error: null })),
    []
  )

  const runUndo = async (): Promise<void> => {
    if (toast === null || toast.undoing) return
    setToast({ ...toast, undoing: true, error: null })
    const problem = await toast.undo()
    setToast((current) =>
      current?.key !== toast.key ? current : problem === null ? null : { ...current, undoing: false, error: problem }
    )
  }

  const close = useCallback(() => setToast(null), [])
  return { toast, raise, runUndo, close }
}

/** Undo by the host's `agents:restore`: null once it put the agent back, else why not. */
export const restoreAgent =
  (host: string, name: string, after?: () => void) =>
  async (): Promise<string | null> => {
    try {
      const result = await bridgeFor(host).invoke('agents:restore', { name })
      if (result.refusal !== null) return result.refusal
      after?.()
      return null
    } catch (err) {
      return readable(err)
    }
  }
