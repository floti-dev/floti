import { useEffect, type JSX } from 'react'
import { readAgentDraft, type EditorHighlight } from '@helm/core/types'
import { AgentPage } from '@helm/ui'
import { bridgeFor } from '../bridge'
import { useAgentDraft, type AgentDraftTarget, type AgentOutcome } from './useAgentDraft'
import { useAgentList } from './useAgentList'
import { useInstructionsPreview } from './useInstructionsPreview'

export interface AgentTabProps {
  host: string
  /** The host as the window names it, and whether it is on another computer. */
  hostName: string
  remote: boolean
  /** The tab's id: what the page's draft is kept under while it is a tab. */
  tabKey: string
  target: AgentDraftTarget
  /** The host's profiles, by name; null when the window cannot list them. */
  profiles: readonly string[] | null
  onDirtyChange: (dirty: boolean) => void
  /** Whether the file as read is not an agent: the tab marks it. */
  onBadChange: (bad: boolean) => void
  /** A save, a rename, a delete or a reload that the window acts on: the tab, a toast. */
  onOutcome: (outcome: AgentOutcome) => void
  onCopy: (name: string) => void
  onDiscard: () => void
  /** Opens a link the instructions' preview holds. */
  onOpenExternal: (url: string) => void
  /** Opens the New session dialog with the agent picked; null where the window cannot. */
  onStartSession: ((name: string) => void) | null
}

/** One agent page: its draft on its host, drawn by `AgentPage`, with Ctrl+S to save. */
export function AgentTab(props: AgentTabProps): JSX.Element {
  const { host, onDirtyChange, onBadChange } = props
  // The host's agents as listed now: the names a new name must not take, its
  // backends and its folder, for this page's own host whichever the view shows.
  const { listing } = useAgentList(host, true)
  const agent = useAgentDraft(host, props.tabKey, props.target, props.onOutcome)
  const { state, dirty } = agent
  const helm = bridgeFor(host)
  const highlight = (path: string, source: string): Promise<EditorHighlight> =>
    helm.invoke('editor:highlight', { path, source })

  useEffect(() => onDirtyChange(dirty), [dirty, onDirtyChange])
  const bad = (state.base?.problems.length ?? 0) > 0
  useEffect(() => onBadChange(bad), [bad, onBadChange])
  const preview = useInstructionsPreview(host, readAgentDraft(state.draft).body)

  const taken = new Set([...(listing?.agents ?? []).map((a) => a.name), ...(listing?.bad ?? []).map((b) => b.name)])
  const savedName = state.base?.name ?? null
  const copyOf = 'copyOf' in props.target ? props.target.copyOf : null

  if (state.loadError !== null && state.base === null && !('copyOf' in props.target && props.target.copyOf === null)) {
    return (
      <p data-agent-load-error className="m-0 px-6 py-4 text-[12.5px] text-danger">
        {state.loadError}
      </p>
    )
  }
  if (!state.loaded) {
    return <p className="m-0 px-6 py-4 text-[12.5px] text-fg-subtle">Reading the agent…</p>
  }

  return (
    <div
      className="h-full min-h-0"
      onKeyDown={(event) => {
        if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 's') {
          event.preventDefault()
          void agent.save()
        }
      }}
    >
      <AgentPage
        savedName={savedName}
        copyOf={copyOf}
        name={state.name}
        onNameChange={agent.setName}
        takenNames={taken}
        hostName={props.hostName}
        remote={props.remote}
        folder={listing?.folder ?? null}
        draft={state.draft}
        onDraftChange={agent.setDraft}
        dirty={dirty && savedName !== null}
        saving={state.saving}
        savedAt={state.savedAt}
        problems={state.problems}
        refusal={state.refusal}
        bad={(state.base?.problems.length ?? 0) > 0}
        external={state.external === null ? null : { text: state.external.file?.text ?? null, at: state.external.at }}
        readOnly={listing?.readOnly ?? null}
        backends={listing?.backends ?? []}
        profiles={props.profiles}
        onHighlight={highlight}
        preview={preview.rendered}
        previewPending={preview.pending}
        onOpenExternal={props.onOpenExternal}
        onSave={() => void agent.save()}
        onRevert={agent.revert}
        onDiscard={props.onDiscard}
        onCopy={() => props.onCopy(savedName ?? copyOf ?? '')}
        onDelete={() => void agent.remove()}
        onStartSession={
          props.onStartSession === null || savedName === null || (state.base?.problems.length ?? 0) > 0
            ? null
            : () => props.onStartSession?.(savedName)
        }
        onReload={() => void agent.reloadFromDisk()}
        onSaveMine={() => void agent.saveMine()}
      />
    </div>
  )
}
