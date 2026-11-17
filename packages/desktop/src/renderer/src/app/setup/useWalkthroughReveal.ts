import { useEffect, useRef } from 'react'

/**
 * Brings the workspace the walkthrough chose into view in the sidebar's tree
 * as the shell opens on it, and again when a different one is chosen. The tree
 * selects it from the walkthrough already; without this it can sit below the
 * fold of a long list. `reveal` is the app's own way of pointing the tree at a
 * project, and runs once per workspace, not on every render.
 */
export function useWalkthroughReveal(workspace: string | null, shown: boolean, reveal: (path: string) => void): void {
  const revealed = useRef<string | null>(null)
  useEffect(() => {
    if (!shown || workspace === null) {
      revealed.current = null
      return
    }
    if (revealed.current === workspace) return
    revealed.current = workspace
    reveal(workspace)
  }, [workspace, shown, reveal])
}
