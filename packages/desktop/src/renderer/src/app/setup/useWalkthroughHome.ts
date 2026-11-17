import { useCallback, useState } from 'react'

export interface WalkthroughHome {
  /** The workspace the sidebar's tree selects on the home screen, or null. */
  selected: string | null
  /** The walkthrough is over, with the workspace step 4 chose, if any. */
  finish: (workspace: string | null) => void
}

/**
 * The workspace a finished walkthrough leaves selected in the sidebar's tree
 * while the shell is on its home screen (`empty`: no tab open). The
 * walkthrough opens no tab, so without this the tree would select nothing
 * the moment it ends. Run again from Settings with tabs open, it selects
 * nothing: the tree follows the tab in front. The selection goes once a tab
 * opens, so closing every tab later does not bring it back.
 */
export function useWalkthroughHome(empty: boolean): WalkthroughHome {
  const [workspace, setWorkspace] = useState<string | null>(null)
  // A tab opening drops the selection, adjusted as the render sees it
  // rather than in an effect, so no frame paints the stale one.
  const [wasEmpty, setWasEmpty] = useState(empty)
  if (wasEmpty !== empty) {
    setWasEmpty(empty)
    if (!empty) setWorkspace(null)
  }
  const finish = useCallback((chosen: string | null) => setWorkspace(empty ? chosen : null), [empty])
  return { selected: empty ? workspace : null, finish }
}
