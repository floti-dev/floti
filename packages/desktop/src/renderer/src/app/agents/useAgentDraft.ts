import { useCallback, useEffect, useRef, useState } from 'react'
import {
  blankAgentText,
  withoutRetiredLines,
  type AgentChangeResult,
  type AgentFileText,
  type AgentProblem
} from '@helm/core/types'
import { bridgeFor } from '../bridge'
import { readable } from '../errors'

/** What an agent page edits: an agent the folder has, or a new one, blank or a copy. */
export type AgentDraftTarget = { name: string } | { copyOf: string | null; name: string }

/** The file as it is on disk now, when it changed after the page read it; `file` is null when it was removed. */
export interface AgentExternal {
  file: AgentFileText | null
  at: string
}

export interface AgentDraftState {
  /** The file as last read or saved; null for a new agent, and while the first read is on its way. */
  base: AgentFileText | null
  name: string
  draft: string
  /** The host's problems with the last save it refused, else with the file as read. */
  problems: AgentProblem[]
  refusal: string | null
  external: AgentExternal | null
  saving: boolean
  savedAt: string | null
  /** Why the file could not be read; null once it was. */
  loadError: string | null
  loaded: boolean
}

/** What a page's action came to, for the page's owner to act on: a tab to rename or close, a toast. */
export type AgentOutcome =
  | { kind: 'created'; name: string }
  | { kind: 'saved'; name: string }
  | { kind: 'renamed'; from: string; name: string }
  | { kind: 'savedOver'; name: string; at: string }
  | { kind: 'deleted'; name: string }
  | { kind: 'reloaded'; name: string; undo: () => void }
  | { kind: 'gone'; name: string }

/**
 * Each open agent page's state, by its tab, so a page that leaves the front
 * of its pane and comes back has its edit as it left it. Only what is on
 * screen, or was and is still a tab, is kept: `forgetAgentDraft` drops a
 * closed tab's.
 */
const kept = new Map<string, AgentDraftState>()

export function forgetAgentDraft(key: string): void {
  kept.delete(key)
}

const clock = (): string => new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })

function initial(target: AgentDraftTarget): AgentDraftState {
  return {
    base: null,
    name: target.name,
    draft: 'copyOf' in target && target.copyOf === null ? blankAgentText() : '',
    problems: [],
    refusal: null,
    external: null,
    saving: false,
    savedAt: null,
    loadError: null,
    loaded: 'copyOf' in target && target.copyOf === null
  }
}

export interface AgentDraft {
  state: AgentDraftState
  dirty: boolean
  setDraft: (text: string) => void
  setName: (name: string) => void
  save: () => Promise<AgentOutcome | null>
  revert: () => void
  remove: () => Promise<AgentOutcome | null>
  reloadFromDisk: () => AgentOutcome | null
  saveMine: () => Promise<AgentOutcome | null>
}

/**
 * One agent page's draft, on `host`. It reads the file when it opens and
 * again whenever `agents:changed` names it: with no unsaved edit the page
 * takes the change quietly; with one, it keeps the edit and holds the file
 * as it is now for the person to choose (the design's A16). A save sends the
 * whole draft without its retired lines and with the hash it was read at, so
 * the host refuses one over a change this page did not see.
 */
export function useAgentDraft(
  host: string,
  key: string,
  target: AgentDraftTarget,
  onOutcome: (outcome: AgentOutcome) => void
): AgentDraft {
  const helm = bridgeFor(host)
  const [state, setState] = useState<AgentDraftState>(() => kept.get(key) ?? initial(target))
  /** The state as last set, for what runs after an answer arrives. */
  const current = useRef(state)
  const outcome = useRef(onOutcome)
  useEffect(() => {
    outcome.current = onOutcome
  }, [onOutcome])
  const update = useCallback(
    (patch: Partial<AgentDraftState> | ((s: AgentDraftState) => Partial<AgentDraftState>)) => {
      const s = current.current
      const next = { ...s, ...(typeof patch === 'function' ? patch(s) : patch) }
      current.current = next
      kept.set(key, next)
      setState(next)
    },
    [key]
  )

  const dirtyOf = (s: AgentDraftState): boolean => s.base === null || s.draft !== s.base.text || s.name !== s.base.name

  /** Reads `name` and takes it as the page's base, or holds it beside an unsaved edit. */
  const refresh = useCallback(
    async (name: string): Promise<void> => {
      let file: AgentFileText | null
      try {
        file = await helm.invoke('agents:read', { name })
      } catch {
        file = null
      }
      const s = current.current
      if (s.base === null || s.base.name !== name) return
      if (file !== null && file.hash === s.base.hash) return
      if (!dirtyOf(s)) {
        if (file === null) outcome.current({ kind: 'gone', name })
        else update({ base: file, draft: file.text, name: file.name, problems: file.problems, refusal: null, external: null })
        return
      }
      if (s.external?.file?.hash === file?.hash && s.external !== null) return
      update({ external: { file, at: clock() } })
    },
    [helm, update]
  )

  const isCopy = 'copyOf' in target
  const from = isCopy ? target.copyOf : target.name
  // The first read: the agent itself, or the one a copy copies. A page that
  // comes back to the front reads its file again, as a change it missed.
  useEffect(() => {
    const s = current.current
    if (s.loaded) {
      if (s.base !== null) void refresh(s.base.name)
      return
    }
    if (from === null) return
    let live = true
    helm
      .invoke('agents:read', { name: from })
      .then((file) => {
        if (!live) return
        if (isCopy) update({ draft: file.text, loaded: true })
        else update({ base: file, draft: file.text, name: file.name, problems: file.problems, loaded: true })
      })
      .catch((err: unknown) => live && update({ loadError: readable(err), loaded: true }))
    return () => {
      live = false
    }
  }, [helm, key, from, isCopy, refresh, update])

  useEffect(
    () =>
      helm.on('agents:changed', ({ names }) => {
        const base = current.current.base
        if (base !== null && names.includes(base.name)) void refresh(base.name)
      }),
    [helm, refresh]
  )

  /** Takes a change the host made: the file is now `file`. */
  const took = (file: AgentFileText): void =>
    update({
      base: file,
      draft: file.text,
      name: file.name,
      problems: [],
      refusal: null,
      external: null,
      saving: false,
      savedAt: clock()
    })

  /** Holds a refusal: each problem on its field, or the host's sentence, and the edit kept. */
  const refused = (result: AgentChangeResult): void => {
    if (result.conflict !== null && current.current.base === null) {
      update({ saving: false, refusal: `There is already an agent called ${current.current.name}.` })
      return
    }
    if (result.conflict !== null) {
      const on = result.file
      update({ saving: false, external: { file: on, at: clock() } })
      return
    }
    update({
      saving: false,
      problems: result.problems,
      refusal: result.problems.length > 0 ? null : (result.refusal ?? 'The host did not save it.')
    })
  }

  const write = async (expectedHash: string | null, renameFrom: string | null): Promise<AgentChangeResult | null> => {
    const s = current.current
    update({ saving: true, refusal: null })
    try {
      return await helm.invoke('agents:write', {
        name: s.name,
        text: withoutRetiredLines(s.draft),
        expectedHash,
        renameFrom
      })
    } catch (err) {
      update({ saving: false, refusal: readable(err) })
      return null
    }
  }

  const save = async (): Promise<AgentOutcome | null> => {
    const s = current.current
    if (s.saving || s.external !== null) return null
    const from = s.base !== null && s.name !== s.base.name ? s.base.name : null
    const result = await write(s.base?.hash ?? null, from)
    if (result === null) return null
    if (result.refusal !== null || result.conflict !== null || result.file === null) {
      refused(result)
      return null
    }
    took(result.file)
    const done: AgentOutcome =
      s.base === null
        ? { kind: 'created', name: result.file.name }
        : from !== null
          ? { kind: 'renamed', from, name: result.file.name }
          : { kind: 'saved', name: result.file.name }
    onOutcome(done)
    return done
  }

  const saveMine = async (): Promise<AgentOutcome | null> => {
    const s = current.current
    if (s.external === null || s.base === null) return null
    const at = s.external.at
    const result = await write(s.external.file?.hash ?? null, null)
    if (result === null) return null
    if (result.refusal !== null || result.conflict !== null || result.file === null) {
      refused(result)
      return null
    }
    took(result.file)
    const done: AgentOutcome = { kind: 'savedOver', name: result.file.name, at }
    onOutcome(done)
    return done
  }

  const reloadFromDisk = (): AgentOutcome | null => {
    const s = current.current
    const file = s.external?.file
    if (file == null) return null
    const mine = { draft: s.draft, name: s.name }
    update({ base: file, draft: file.text, name: file.name, problems: file.problems, refusal: null, external: null })
    const done: AgentOutcome = { kind: 'reloaded', name: file.name, undo: () => update(mine) }
    onOutcome(done)
    return done
  }

  const remove = async (): Promise<AgentOutcome | null> => {
    const s = current.current
    if (s.base === null) return null
    let result: AgentChangeResult
    try {
      result = await helm.invoke('agents:delete', { name: s.base.name, expectedHash: s.base.hash })
    } catch (err) {
      update({ refusal: readable(err) })
      return null
    }
    if (result.refusal !== null || result.conflict !== null) {
      refused(result)
      return null
    }
    const done: AgentOutcome = { kind: 'deleted', name: s.base.name }
    onOutcome(done)
    return done
  }

  return {
    state,
    dirty: dirtyOf(state),
    setDraft: (draft) => update({ draft }),
    setName: (name) => update({ name }),
    save,
    revert: () =>
      update((s) =>
        s.base === null ? {} : { draft: s.base.text, name: s.base.name, problems: s.base.problems, refusal: null }
      ),
    remove,
    reloadFromDisk,
    saveMine
  }
}
