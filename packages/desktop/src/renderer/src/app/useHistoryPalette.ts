import { useCallback, useEffect, useMemo, useState } from 'react'
import type {
  HistoryProject,
  HistoryQuery,
  HistorySession,
  HistorySummary,
  LaunchAgents
} from '@helm/core'
import type { HistoryAnswer } from '@helm/ui'
import { bridgeFor } from './bridge'
import { readable } from './errors'

/**
 * What History's palette reads, on one host: the sessions worked in last, the
 * two lists a query gives, a parent's children and the host's agents. Each is
 * a read of the host's index (`history:*`) and nothing else; nothing new is
 * collected. Every list is the answer to the query that is current: a stale
 * answer is dropped on arrival, and a later read of the same list keeps the
 * last one on screen while it is asked.
 */

/** The sessions worked in last, as the palette lists them with nothing typed. */
export const RECENT_LIMIT = 10

/** How many sessions a typed list reads: a palette is not scrolled through for more. */
export const SEARCH_LIMIT = 50

/** Long enough that a typed word is one query, short enough that the list keeps up with the keys. */
const SEARCH_DEBOUNCE_MS = 90

export interface HistoryPaletteState {
  query: string
  setQuery: (query: string) => void
  summary: HistorySummary | null
  recent: HistoryAnswer
  titles: HistoryAnswer
  conversations: HistoryAnswer
  children: ReadonlyMap<string, HistoryAnswer>
  unfold: (parent: HistorySession) => void
  agents: Pick<LaunchAgents, 'agents' | 'bad'> | null
  /** Resolves to the sentence a refusal gave, or null. */
  rename: (session: HistorySession, name: string | null) => Promise<string | null>
  retry: () => void
}

/**
 * The query as the host is asked it: a word that is a recorded folder's name
 * narrows to that folder (the most recent one of that name) and is not
 * searched for; the rest is searched.
 */
export function splitQuery(query: string, projects: readonly HistoryProject[]): { search: string; project: string | null } {
  const words = query.trim().split(/\s+/).filter((word) => word !== '')
  let project: HistoryProject | null = null
  const rest: string[] = []
  for (const word of words) {
    const named: HistoryProject | null = project === null ? newestNamed(projects, word) : null
    if (named !== null) project = named
    else rest.push(word)
  }
  return { search: rest.join(' '), project: project?.project ?? null }
}

function newestNamed(projects: readonly HistoryProject[], word: string): HistoryProject | null {
  const lower = word.toLowerCase()
  let best: HistoryProject | null = null
  for (const each of projects) {
    if (each.name.toLowerCase() !== lower) continue
    if (best === null || each.lastAt > best.lastAt) best = each
  }
  return best
}

const answerOf = (page: { sessions: HistorySession[]; total: number }): HistoryAnswer => ({
  sessions: page.sessions,
  total: page.total
})

const failed = (err: unknown): HistoryAnswer => ({ error: readable(err) })

/** The archive's answer to a query of only a folder's name: nothing was said to search it for. */
const NOTHING_SAID: HistoryAnswer = { sessions: [], total: 0 }

/** `answer` with `updated` in place of the session it names. */
function patched(answer: HistoryAnswer, updated: HistorySession): HistoryAnswer {
  if (answer === null || 'error' in answer) return answer
  return {
    ...answer,
    sessions: answer.sessions.map((each) => (each.sessionId === updated.sessionId ? { ...each, ...updated } : each))
  }
}

export function useHistoryPalette(host: string, open: boolean): HistoryPaletteState {
  const helm = bridgeFor(host)
  const [query, setQuery] = useState('')
  const [debounced, setDebounced] = useState('')
  const [summary, setSummary] = useState<HistorySummary | null>(null)
  const [recent, setRecent] = useState<HistoryAnswer>(null)
  const [titles, setTitles] = useState<HistoryAnswer>(null)
  const [conversations, setConversations] = useState<HistoryAnswer>(null)
  const [children, setChildren] = useState<ReadonlyMap<string, HistoryAnswer>>(new Map())
  const [projects, setProjects] = useState<readonly HistoryProject[]>([])
  const [agents, setAgents] = useState<LaunchAgents | null>(null)
  /** Moved by a retry, an index change and a rename, so every list is read again. */
  const [version, setVersion] = useState(0)

  // Another host, or the palette closed: nothing read stays, and it opens empty.
  const shown = `${host}:${String(open)}`
  const [was, setWas] = useState(shown)
  if (was !== shown) {
    setWas(shown)
    setQuery('')
    setDebounced('')
    setSummary(null)
    setRecent(null)
    setTitles(null)
    setConversations(null)
    setChildren(new Map())
    setProjects([])
    setAgents(null)
  }

  useEffect(() => {
    const timer = setTimeout(() => setDebounced(query), SEARCH_DEBOUNCE_MS)
    return () => clearTimeout(timer)
  }, [query])

  // The summary, the folders and the agents, read once each time it opens;
  // the summary again whenever the index moves, which reads the lists again.
  useEffect(() => {
    if (!open) return
    let live = true
    const off = helm.on('history:changed', (next) => {
      if (!live) return
      setSummary(next)
      setVersion((count) => count + 1)
    })
    helm.invoke('history:summary').then(
      (next) => live && setSummary(next),
      () => undefined
    )
    helm.invoke('history:projects').then(
      (next) => live && setProjects(next),
      () => undefined
    )
    // A listing that could not be read is "not known", never "every agent gone".
    helm.invoke('agents:list').then(
      (next) => live && setAgents(next),
      () => undefined
    )
    return () => {
      live = false
      off()
    }
  }, [helm, open])

  const read = useCallback(
    (request: HistoryQuery) => helm.invoke('history:sessions', request).then(answerOf, failed),
    [helm]
  )

  useEffect(() => {
    if (!open) return
    let live = true
    void read({ topLevel: true, limit: RECENT_LIMIT }).then((answer) => {
      if (live) setRecent(answer)
    })
    return () => {
      live = false
    }
  }, [open, read, version])

  const split = useMemo(() => splitQuery(debounced, projects), [debounced, projects])
  useEffect(() => {
    if (!open || debounced.trim() === '') return
    let live = true
    const narrowed = split.project === null ? {} : { project: split.project }
    void read({ search: split.search, scope: 'prompts', limit: SEARCH_LIMIT, ...narrowed }).then((answer) => {
      if (live) setTitles(answer)
    })
    // Only a folder's name says nothing to search the archive for.
    if (split.search !== '') {
      void read({ search: split.search, scope: 'messages', limit: SEARCH_LIMIT, ...narrowed }).then((answer) => {
        if (live) setConversations(answer)
      })
    }
    return () => {
      live = false
    }
  }, [open, debounced, split, read, version])

  // A new query is read afresh: the last query's lists are not this one's.
  const [asked, setAsked] = useState(debounced)
  if (asked !== debounced) {
    setAsked(debounced)
    setTitles(null)
    setConversations(null)
  }

  const unfold = useCallback(
    (parent: HistorySession) => {
      setChildren((current) => new Map(current).set(parent.sessionId, null))
      void read({ parent: parent.sessionId, limit: SEARCH_LIMIT }).then((answer) =>
        setChildren((current) => (current.has(parent.sessionId) ? new Map(current).set(parent.sessionId, answer) : current))
      )
    },
    [read]
  )

  const rename = useCallback(
    async (session: HistorySession, name: string | null) => {
      try {
        const updated = await helm.invoke('history:rename', { sessionId: session.sessionId, name })
        if (updated === null) return null
        setRecent((answer) => patched(answer, updated))
        setTitles((answer) => patched(answer, updated))
        setConversations((answer) => patched(answer, updated))
        setChildren((current) => new Map([...current].map(([key, answer]) => [key, patched(answer, updated)])))
        // A listing that was in flight read the old name: every list is read again.
        setVersion((count) => count + 1)
        return null
      } catch (err: unknown) {
        return readable(err)
      }
    },
    [helm]
  )

  const retry = useCallback(() => {
    void helm
      .invoke('history:refresh')
      .then(setSummary, () => undefined)
      .finally(() => setVersion((count) => count + 1))
  }, [helm])

  return {
    query,
    setQuery,
    summary,
    recent,
    titles: debounced.trim() === '' ? null : titles,
    conversations: debounced.trim() === '' ? null : split.search === '' ? NOTHING_SAID : conversations,
    children,
    unfold,
    agents,
    rename,
    retry
  }
}
