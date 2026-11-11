import { useEffect, useState, type JSX } from 'react'
import type { ArchivedConversation, ConversationRef, HistoryPrompt, HistorySession } from '@helm/core'
import { hostOf } from '@helm/core/types'
import { ConversationTab, historySpan, type ConversationFamily } from '@helm/ui'
import { bridgeFor } from './bridge'
import { readable } from './errors'

/**
 * A conversation tab's reads, on the tab's own host: the archived
 * conversation, what was typed where the archive has none, and the children
 * and commands its sessions started. Read once when the tab opens; an
 * archived conversation does not change. Keyed by its tab, so another
 * conversation is another mount.
 */
export interface ConversationPaneProps {
  tab: ConversationRef
  /** Resumes it on its host; resolves to the sentence a refusal gave, or null. */
  onResume: (tab: ConversationRef) => Promise<string | null>
  /** Names it by hand, or clears the name; the tab takes the new title. */
  onRename: (tab: ConversationRef, name: string | null) => void
  /** Absent where its folder cannot be shown from here. */
  onReveal?: ((path: string) => void) | undefined
  /** Opens a child's conversation in a tab of its own, on this tab's host. */
  onReadChild: (tab: ConversationRef, child: HistorySession) => void
  /** Whether a conversation runs now on this tab's host. */
  runsNow: (tab: ConversationRef, conversationId: string) => boolean
}

/** How many children and commands a parent's tab lists, as History lists a parent's children. */
const FAMILY_LIMIT = 50

export function ConversationPane({ tab, onResume, onRename, onReveal, onReadChild, runsNow }: ConversationPaneProps): JSX.Element {
  const host = hostOf(tab)
  const [conversation, setConversation] = useState<ArchivedConversation | null | undefined>(undefined)
  const [prompts, setPrompts] = useState<HistoryPrompt[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [resumeError, setResumeError] = useState<string | null>(null)
  const [family, setFamily] = useState<ConversationFamily | null>(null)

  useEffect(() => {
    let live = true
    const helm = bridgeFor(host)
    helm.invoke('archive:conversation', { sessionId: tab.sessionId }).then(
      (answer) => {
        if (!live) return
        setConversation(answer)
        if (answer === null || answer.state !== 'archived') {
          helm.invoke('history:prompts', { sessionId: tab.sessionId }).then(
            (typed) => live && setPrompts(typed),
            (err: unknown) => live && setError(readable(err))
          )
        }
      },
      (err: unknown) => live && setError(readable(err))
    )
    helm.invoke('history:sessions', { parent: tab.sessionId, limit: FAMILY_LIMIT }).then(
      (page) => live && setFamily({ children: page.sessions, commands: page.commands ?? [] }),
      (err: unknown) => live && setFamily({ error: readable(err) })
    )
    return () => {
      live = false
    }
  }, [host, tab.sessionId])

  const facts =
    conversation === null || conversation === undefined || conversation.firstAt === null || conversation.lastAt === null
      ? []
      : [historySpan(conversation.firstAt, conversation.lastAt)]

  return (
    <ConversationTab
      title={tab.title}
      agent={tab.agent}
      project={tab.project}
      facts={facts}
      conversation={conversation}
      error={error}
      prompts={prompts}
      at={tab.at ?? null}
      find={tab.find}
      onResume={() => {
        setResumeError(null)
        void onResume(tab).then(setResumeError)
      }}
      onRename={(name) => onRename(tab, name)}
      onReveal={onReveal === undefined ? undefined : () => onReveal(tab.project)}
      resumeError={resumeError}
      family={family}
      onReadChild={(child) => onReadChild(tab, child)}
      childRuns={(child) => runsNow(tab, child.sessionId)}
    />
  )
}
