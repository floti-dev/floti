import { closeTab, LOCAL_HOST_KEY, paneId, replaceTab, type PaneLayout, type PaneRef } from '@helm/core/types'
import { forgetAgentDraft, moveAgentDraft, type AgentOutcome } from './useAgentDraft'
import { restoreAgent } from './useAgentToast'

/** An agent's page on `host`, as a tab's ref. */
export const agentRefOn = (name: string, host: string): PaneRef =>
  host === LOCAL_HOST_KEY ? { kind: 'agent', name } : { kind: 'agent', name, host }

/** What an agent page's outcome acts on in the window. */
export interface AgentOutcomeWindow {
  commit: (op: (current: PaneLayout) => PaneLayout) => void
  markDocDirty: (key: string, dirty: boolean) => void
  raise: (title: string, detail: string | null, undo: () => Promise<string | null>) => void
}

/**
 * What the page in tab `id`, on `host`, coming to `outcome` does to its tab,
 * and the toast it leaves. A save that gives the page a new name (a new
 * agent's first, a rename, a Save mine that renames) moves its tab to that
 * name and carries the page's state with it, so what was typed while the
 * save was on its way stays.
 */
export const agentOutcomeIn =
  (window: AgentOutcomeWindow, id: string, host: string) =>
  (outcome: AgentOutcome): void => {
    const { commit, markDocDirty, raise } = window
    const leave = (key: string): void => {
      forgetAgentDraft(key)
      markDocDirty(key, false)
    }
    /** The tab, and its page's state, under `name` now. */
    const moveTo = (name: string): PaneRef => {
      const ref = agentRefOn(name, host)
      moveAgentDraft(id, paneId(ref))
      markDocDirty(id, false)
      commit((current) => replaceTab(current, id, ref))
      return ref
    }
    /** An Undo that puts `from` back where the page now under `ref` is. */
    const undoRename = (ref: PaneRef, from: string): (() => Promise<string | null>) =>
      restoreAgent(host, from, () => {
        leave(paneId(ref))
        commit((current) => replaceTab(current, paneId(ref), agentRefOn(from, host)))
      })

    switch (outcome.kind) {
      case 'saved':
        return
      case 'created':
        moveTo(outcome.name)
        return
      case 'renamed': {
        const renamed = moveTo(outcome.name)
        raise(`Renamed ${outcome.from} to ${outcome.name}`, 'Undo gives it its old name back.', undoRename(renamed, outcome.from))
        return
      }
      case 'savedOver': {
        // Undo of a save that moved the file puts it back under its old name;
        // of one that made it anew, removes it, and the page then closes as gone.
        const moved = outcome.from === null ? null : moveTo(outcome.name)
        const undo =
          moved !== null && outcome.moved && outcome.from !== null
            ? undoRename(moved, outcome.from)
            : restoreAgent(host, outcome.name)
        raise(`Saved over the change made at ${outcome.at}`, 'Undo puts that change back.', undo)
        return
      }
      case 'deleted':
        leave(id)
        commit((current) => closeTab(current, id))
        raise(
          `Deleted ${outcome.name}`,
          "Its file is kept in Floti's data folder, so Undo puts it back as it was.",
          restoreAgent(host, outcome.name)
        )
        return
      case 'reloaded':
        raise(`Reloaded ${outcome.name}.md`, 'Undo brings your edit back.', () => {
          outcome.undo()
          return Promise.resolve(null)
        })
        return
      case 'gone':
        leave(id)
        commit((current) => closeTab(current, id))
        return
    }
  }
