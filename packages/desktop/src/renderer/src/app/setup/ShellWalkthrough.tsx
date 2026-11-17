import type { JSX } from 'react'
import { useEffect, useState } from 'react'
import type { AppSettings, Walkthrough } from '@helm/core/types'
import { FirstSessionStep, TourStep } from '@helm/ui'
import type { HelmBridge } from '../../../../shared/ipc'
import { folderName, WALKTHROUGH_DONE } from './walkthrough'

const sentence = (err: unknown): string => (err instanceof Error ? err.message : String(err))

/** The session step 6 asks the shell to start. */
export interface FirstSessionRequest {
  cwd: string
  backend: string
  /** The first message, or empty for none. */
  openingPrompt: string
}

export interface ShellWalkthroughProps {
  /** At step 5 or 6. */
  walkthrough: Walkthrough
  bridge: Pick<HelmBridge, 'invoke'>
  /**
   * Whether this PC's host is ready. The agents are read again when it
   * becomes so: a read made while the shell was still connecting can answer
   * none.
   */
  hostReady: boolean
  /** Writes the shell's settings, as every other shell write goes. */
  write: (patch: Partial<AppSettings>) => void
  /** Starts the session and puts it in a tab; rejects with why it did not start. */
  start: (request: FirstSessionRequest) => Promise<void>
  /** The walkthrough is over: skipped, or its session started. */
  onDone: (workspace: string | null, started: boolean) => void
}

/**
 * Steps 5 and 6 of the guided first run, in the shell's main area beside the
 * rail and sidebar step 5 names. Starting the session, or skipping, marks the
 * walkthrough done, so it shows again only from Settings > General.
 */
export function ShellWalkthrough({
  walkthrough,
  bridge,
  hostReady,
  write,
  start,
  onDone
}: ShellWalkthroughProps): JSX.Element {
  const { workspace } = walkthrough
  const finish = (started: boolean): void => {
    write(WALKTHROUGH_DONE)
    onDone(workspace, started)
  }
  return (
    <div
      data-shell-walkthrough
      className="h-full overflow-y-auto rounded-island border border-border bg-surface px-6 pt-[26px] pb-[18px]"
    >
      <div className="h-full min-h-[398px]">
        {walkthrough.step === 5 ? (
          <TourStep
            onSkip={() => write({ walkthrough: { step: 6, workspace } })}
            // Step 4 is the setup window's: main opens it again there.
            onBack={() => void bridge.invoke('walkthrough:open', { step: 4 })}
            onContinue={() => write({ walkthrough: { step: 6, workspace } })}
          />
        ) : (
          <FirstSession
            bridge={bridge}
            hostReady={hostReady}
            workspace={workspace}
            start={start}
            onSkip={() => finish(false)}
            onBack={() => write({ walkthrough: { step: 5, workspace } })}
            onStarted={() => finish(true)}
          />
        )}
      </div>
    </div>
  )
}

function FirstSession({
  bridge,
  hostReady,
  workspace,
  start,
  onSkip,
  onBack,
  onStarted
}: {
  bridge: Pick<HelmBridge, 'invoke'>
  hostReady: boolean
  workspace: string | null
  start: ShellWalkthroughProps['start']
  onSkip: () => void
  onBack: () => void
  onStarted: () => void
}): JSX.Element {
  const [agents, setAgents] = useState<{ id: string; name: string }[]>([])
  const [agent, setAgent] = useState<string | null>(null)
  const [message, setMessage] = useState('')
  const [problems, setProblems] = useState<string[]>([])
  const [busy, setBusy] = useState(false)

  // The agents this PC's host runs, Claude Code first when it is one: read
  // on arrival and again once the host is ready.
  useEffect(() => {
    let cancelled = false
    void bridge.invoke('agents:list').then(
      (listing) => {
        if (cancelled) return
        setAgents(listing.backends)
        setAgent((chosen) => chosen ?? listing.backends.find((b) => b.id === 'claude-code')?.id ?? listing.backends[0]?.id ?? null)
      },
      (err: unknown) => {
        if (!cancelled) setProblems([sentence(err)])
      }
    )
    return () => {
      cancelled = true
    }
  }, [bridge, hostReady])

  const begin = async (): Promise<void> => {
    if (workspace === null || agent === null) return
    setBusy(true)
    setProblems([])
    try {
      // The agent picked is still installed: Claude Code by its own check.
      if (agent === 'claude-code') {
        const status = await bridge.invoke('setup:status')
        if (status.path === null || status.version === null) {
          setProblems([status.error ?? 'Claude Code is not installed on this PC.'])
          return
        }
      }
      await start({ cwd: workspace, backend: agent, openingPrompt: message.trim() })
      onStarted()
    } catch (err) {
      setProblems([sentence(err)])
    } finally {
      setBusy(false)
    }
  }

  return (
    <FirstSessionStep
      workspace={workspace === null ? null : folderName(workspace)}
      agents={agents}
      agent={agent}
      onAgent={setAgent}
      message={message}
      onMessage={setMessage}
      problems={problems}
      busy={busy}
      onSkip={onSkip}
      onBack={onBack}
      onStart={() => void begin()}
    />
  )
}
