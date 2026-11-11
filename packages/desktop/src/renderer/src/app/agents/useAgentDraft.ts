import { useCallback, useEffect, useRef, useState } from 'react'
import {
  blankAgentText,
  copyName,
  listedAgentNames,
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
  /**
   * Why the file could not be read: the first time, or again after that while
   * the host still lists it (the page keeps what it last read). Null once it was.
   */
  loadError: string | null
  loaded: boolean
}

/** What a page's action came to, for the page's owner to act on: a tab to rename or close, a toast. */
export type AgentOutcome =
  | { kind: 'created'; name: string }
  | { kind: 'saved'; name: string }
  | { kind: 'renamed'; from: string; name: string }
  /**
   * `from` is the name the page had when the save over the change also gave
   * it a new one, else null; `moved` says the host moved that file to the new
   * name, which it does not when the file had been removed and the save made it.
   */
  | { kind: 'savedOver'; name: string; at: string; from: string | null; moved: boolean }
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
/**
 * How many times each key was forgotten. A page keeps its state only while
 * its key has not been forgotten since it opened, so an answer or an Undo
 * that arrives after its tab closed does not put the closed tab's state back
 * for the next page opened under the same key.
 */
const forgotten = new Map<string, number>()

export function forgetAgentDraft(key: string): void {
  kept.delete(key)
  forgotten.set(key, (forgotten.get(key) ?? 0) + 1)
}

/**
 * Moves the page's state from tab `from` to tab `to`: a save that gave the
 * page a new name gives it a new tab, and what was typed while the save was
 * on its way goes with it. The page under `from` keeps nothing after.
 */
export function moveAgentDraft(from: string, to: string): void {
  if (from === to) return
  const state = kept.get(from)
  forgetAgentDraft(from)
  if (state !== undefined) kept.set(to, state)
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

/** What a write sent, so what was typed while it was on its way is kept. */
interface Sent {
  draft: string
  name: string
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
  /** How many times the key had been forgotten when this page opened. */
  const generation = useRef(forgotten.get(key) ?? 0)
  const outcome = useRef(onOutcome)
  useEffect(() => {
    outcome.current = onOutcome
  }, [onOutcome])
  const update = useCallback(
    (patch: Partial<AgentDraftState> | ((s: AgentDraftState) => Partial<AgentDraftState>)) => {
      const s = current.current
      const next = { ...s, ...(typeof patch === 'function' ? patch(s) : patch) }
      current.current = next
      if ((forgotten.get(key) ?? 0) === generation.current) kept.set(key, next)
      setState(next)
    },
    [key]
  )

  const dirtyOf = (s: AgentDraftState): boolean => s.base === null || s.draft !== s.base.text || s.name !== s.base.name

  /**
   * Whether the host no longer has `name`: its listing answered and names no
   * agent or bad file by it. A listing that could not be had (a dropped link)
   * says nothing, so the page stays.
   */
  const isGone = useCallback(
    async (name: string): Promise<boolean> => {
      try {
        return !listedAgentNames(await helm.invoke('agents:list')).has(name)
      } catch {
        return false
      }
    },
    [helm]
  )

  /** Reads `name` and takes it as the page's base, or holds it beside an unsaved edit. */
  const refresh = useCallback(
    async (name: string): Promise<void> => {
      let file: AgentFileText | null = null
      let failure: string | null = null
      try {
        file = await helm.invoke('agents:read', { name })
      } catch (err) {
        failure = readable(err)
      }
      if (failure !== null && !(await isGone(name))) {
        // The host still has it, or could not be asked: keep the page as last read and say why.
        if (current.current.base?.name === name) update({ loadError: failure })
        return
      }
      const s = current.current
      if (s.base === null || s.base.name !== name) return
      if (file !== null && file.hash === s.base.hash) {
        if (s.loadError !== null) update({ loadError: null })
        return
      }
      if (!dirtyOf(s)) {
        if (file === null) outcome.current({ kind: 'gone', name })
        else
          update({
            base: file,
            draft: file.text,
            name: file.name,
            problems: file.problems,
            refusal: null,
            external: null,
            loadError: null
          })
        return
      }
      if (s.external !== null && s.external.file?.hash === file?.hash) {
        if (s.loadError !== null) update({ loadError: null })
        return
      }
      update({ external: { file, at: clock() }, loadError: null })
    },
    [helm, isGone, update]
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
    // A copy is named after the names its own host's folder holds now, bad files included.
    const taken = isCopy
      ? helm.invoke('agents:list').then(listedAgentNames, () => new Set([from]))
      : Promise.resolve(null)
    Promise.all([helm.invoke('agents:read', { name: from }), taken])
      .then(([file, names]) => {
        if (!live) return
        if (names !== null) update({ draft: file.text, name: copyName(from, names), loaded: true })
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

  /**
   * Takes a change the host made: the file is now `file`. The draft and the
   * name become the file's unless they were changed while the write was on its
   * way, which leaves them as typed and the page with an unsaved edit.
   */
  const took = (file: AgentFileText, sent: Sent): void =>
    update((s) => ({
      base: file,
      draft: s.draft === sent.draft ? file.text : s.draft,
      name: s.name === sent.name ? file.name : s.name,
      problems: [],
      refusal: null,
      external: null,
      saving: false,
      savedAt: clock(),
      loadError: null
    }))

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

  const write = async (
    sent: Sent,
    expectedHash: string | null,
    renameFrom: string | null
  ): Promise<AgentChangeResult | null> => {
    update({ saving: true, refusal: null })
    try {
      return await helm.invoke('agents:write', {
        name: sent.name,
        text: withoutRetiredLines(sent.draft),
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
    const sent = { draft: s.draft, name: s.name }
    const from = s.base !== null && s.name !== s.base.name ? s.base.name : null
    const result = await write(sent, s.base?.hash ?? null, from)
    if (result === null) return null
    if (result.refusal !== null || result.conflict !== null || result.file === null) {
      refused(result)
      return null
    }
    took(result.file, sent)
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
    const sent = { draft: s.draft, name: s.name }
    // A rename moves the file as it is now; a file that was removed has nothing
    // to move, and the save makes it under its new name. The tab follows the
    // name either way.
    const from = s.name !== s.base.name ? s.base.name : null
    const renameFrom = s.external.file !== null ? from : null
    const result = await write(sent, s.external.file?.hash ?? null, renameFrom)
    if (result === null) return null
    if (result.refusal !== null || result.conflict !== null || result.file === null) {
      refused(result)
      return null
    }
    took(result.file, sent)
    const done: AgentOutcome = { kind: 'savedOver', name: result.file.name, at, from, moved: renameFrom !== null }
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

  /**
   * Drops the edit. With a change from outside held, the page has no edit
   * left to keep it for: it takes the file as it is now, or, when the file
   * was removed, is gone.
   */
  const revert = (): void => {
    const s = current.current
    if (s.base === null) return
    const external = s.external
    if (external === null) {
      update({ draft: s.base.text, name: s.base.name, problems: s.base.problems, refusal: null })
      return
    }
    if (external.file === null) {
      outcome.current({ kind: 'gone', name: s.base.name })
      return
    }
    const file = external.file
    update({ base: file, draft: file.text, name: file.name, problems: file.problems, refusal: null, external: null })
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
    revert,
    remove,
    reloadFromDisk,
    saveMine
  }
}
