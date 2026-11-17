import type { JSX } from 'react'
import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from 'react'
import {
  LOCAL_HOST_KEY,
  quitChoice,
  WALKTHROUGH_START,
  walkthroughInSetupWindow,
  type AppSettings,
  type Walkthrough,
  type WalkthroughStep
} from '@floti/core/types'
import {
  AddHostDialog,
  AgentsStep,
  HostStep,
  TitleBar,
  WelcomeStep,
  WorkspaceStep,
  type HostStepProps,
  type WalkthroughAgent,
  type WalkthroughQuit,
  type WorkspaceChoice
} from '@floti/ui'
import type { ClaudeStatus, DesktopBridge } from '../../../../shared/ipc'
import { addHostActions } from '../hostActions'
import { atStep, claudeAgent, stepAfter, stepBefore, WALKTHROUGH_DONE } from './walkthrough'
import type { LocalHostStepStore } from '../host/localHostStep'

const sentence = (err: unknown): string => (err instanceof Error ? err.message : String(err))

/** What the setup window reaches: the routed bridge, and main's steps for this PC's host. */
export interface SetupWindowProps {
  bridge: Pick<DesktopBridge, 'invoke' | 'on'>
  hostStep: LocalHostStepStore
}

/**
 * The guided first run's setup window: steps 1 to 4, before the shell exists
 * (Floti Onboarding, "Guided first run").
 *
 * The step is the `walkthrough` setting, written on every move, so closing
 * the window resumes here. Once step 4 is done or skipped, or setup is
 * skipped on step 1, the host is told setup is done and main opens the shell
 * and closes this window (`walkthrough:openShell`). It reads the agents
 * through the backend's own check, starts nothing but what the user asks, and
 * reaches another computer only through the add-a-host dialog.
 */
export function SetupWindow({ bridge, hostStep }: SetupWindowProps): JSX.Element {
  const [settings, setSettings] = useState<AppSettings | null>(null)
  const [step, setStep] = useState<WalkthroughStep | null>(null)
  const [readError, setReadError] = useState<string | null>(null)
  const handedOver = useRef(false)

  const write = useCallback(
    async (patch: Partial<AppSettings>): Promise<void> => {
      const next = await bridge.invoke('settings:write', patch)
      setSettings(next)
    },
    [bridge]
  )

  /** Tells the host setup is done and hands the window over to the shell, once. */
  const handOver = useCallback(
    async (walkthrough: Walkthrough | null): Promise<void> => {
      if (handedOver.current) return
      handedOver.current = true
      try {
        await write({ walkthrough })
        // Stamped as the old setup pane stamped it: the host's record that this
        // computer has been set up. A host that cannot be reached is no reason to
        // keep someone here; the shell says what the host is doing.
        await bridge.invoke('setup:complete').catch(() => undefined)
      } finally {
        await bridge.invoke('walkthrough:openShell')
      }
    },
    [bridge, write]
  )

  useEffect(() => {
    let cancelled = false
    void bridge.invoke('settings:read').then(
      (read) => {
        if (cancelled) return
        setSettings(read)
        // Opened with nothing for this window to show: the shell's turn.
        if (!walkthroughInSetupWindow(read.walkthrough)) {
          void bridge.invoke('walkthrough:openShell')
          return
        }
        setStep(read.walkthrough?.step ?? WALKTHROUGH_START.step)
      },
      (err: unknown) => {
        if (!cancelled) setReadError(sentence(err))
      }
    )
    return () => {
      cancelled = true
    }
  }, [bridge])

  const goTo = useCallback(
    (next: WalkthroughStep) => {
      setStep(next)
      void write({ walkthrough: atStep(settings?.walkthrough ?? null, next) }).catch(() => undefined)
    },
    [settings, write]
  )

  const body =
    step === null ? (
      <p data-walkthrough-loading className="text-[12px] text-fg-subtle">
        {readError === null ? 'Looking…' : `Unknown: ${readError}`}
      </p>
    ) : step === 1 ? (
      <WelcomeStep onSkipSetup={() => void handOver(WALKTHROUGH_DONE.walkthrough)} onGetStarted={() => goTo(2)} />
    ) : step === 2 ? (
      <AgentsScreen bridge={bridge} onSkip={() => goTo(3)} onBack={() => goTo(1)} onContinue={() => goTo(3)} />
    ) : step === 3 ? (
      <HostScreen
        bridge={bridge}
        hostStep={hostStep}
        settings={settings}
        write={write}
        onRefresh={() => void bridge.invoke('settings:read').then(setSettings, () => undefined)}
        onSkip={() => goTo(4)}
        onBack={() => goTo(stepBefore(3))}
        onContinue={() => goTo(stepAfter(3))}
      />
    ) : (
      <WorkspaceScreen
        bridge={bridge}
        settings={settings}
        // Skipped, it keeps a workspace an earlier pass chose (Back from step 5).
        onSkip={() => void handOver({ step: 5, workspace: settings?.walkthrough?.workspace ?? null })}
        onBack={() => goTo(3)}
        onDone={(workspace) => void handOver({ step: 5, workspace })}
      />
    )

  return (
    <div className="flex h-full w-full flex-col bg-bg text-fg" data-setup-window>
      <TitleBar />
      <div className="min-h-0 flex-1 px-1.5 pb-1.5">
        <div className="h-full rounded-island border border-border bg-surface px-6 pt-[26px] pb-[18px]">{body}</div>
      </div>
    </div>
  )
}

// ---------------------------------------------------------------------------

/** Whether the host this PC runs is Windows, for the install line; Unknown reads as not. */
function useHostIsWindows(bridge: SetupWindowProps['bridge']): boolean {
  const [windows, setWindows] = useState(false)
  useEffect(() => {
    let cancelled = false
    void bridge.invoke('app:info').then(
      (info) => {
        if (!cancelled) setWindows(info.windowsBuild !== null)
      },
      () => undefined
    )
    return () => {
      cancelled = true
    }
  }, [bridge])
  return windows
}

function AgentsScreen({
  bridge,
  onSkip,
  onBack,
  onContinue
}: {
  bridge: SetupWindowProps['bridge']
  onSkip: () => void
  onBack: () => void
  onContinue: () => void
}): JSX.Element {
  const windows = useHostIsWindows(bridge)
  const [agents, setAgents] = useState<WalkthroughAgent[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [checking, setChecking] = useState(false)
  const [locateProblem, setLocateProblem] = useState<string | null>(null)
  /** The error of the status the row was last drawn from: how a cancelled pick is told from a failed one. */
  const shownError = useRef<string | null>(null)
  const locating = useRef(false)

  /** Draws the row from a status; one that turned installed has nothing left to say about a pick. */
  const show = useCallback(
    (status: ClaudeStatus) => {
      const agent = claudeAgent(status, windows)
      setAgents([agent])
      setError(null)
      shownError.current = status.error
      if (agent.installed === true) setLocateProblem(null)
    },
    [windows]
  )

  /** One read of the backend's own check. Could not look: said as Unknown, and what was known stays. */
  const read = useCallback(
    () => bridge.invoke('setup:status').then(show, (err: unknown) => setError(sentence(err))),
    [bridge, show]
  )

  // Read on arrival, with no spinner: nobody asked yet.
  useEffect(() => {
    void read()
  }, [read])

  /** Check again: the user asked and is waiting, so it spins. */
  const check = useCallback(() => {
    setChecking(true)
    void read().finally(() => setChecking(false))
  }, [read])

  /**
   * Locate…: the picker and the host's check of the picked file, which keeps
   * it as `claudePath` only when it passes (`setup:locateClaude`, as Settings
   * does). A cancel answers the current status and a failed pick that status
   * with the check's sentence in place of its error, so a pick that did not
   * pass is one whose error is not the one already drawn.
   */
  const locate = useCallback(() => {
    if (locating.current) return
    locating.current = true
    void bridge
      .invoke('setup:locateClaude')
      .then(
        (status) => {
          const kept = claudeAgent(status, windows).installed === true
          if (!kept && status.error !== null && status.error !== shownError.current) setLocateProblem(status.error)
          else show(status)
        },
        (err: unknown) => setLocateProblem(sentence(err))
      )
      .finally(() => {
        locating.current = false
      })
  }, [bridge, show, windows])

  return (
    <AgentsStep
      agents={agents}
      error={error}
      checking={checking}
      onCheckAgain={check}
      onCopy={(text) => void bridge.invoke('clipboard:write', text).catch(() => undefined)}
      onLocate={locate}
      locateProblem={locateProblem}
      onSkip={onSkip}
      onBack={onBack}
      onContinue={onContinue}
    />
  )
}

function HostScreen({
  bridge,
  hostStep,
  settings,
  write,
  onRefresh,
  onSkip,
  onBack,
  onContinue
}: {
  bridge: SetupWindowProps['bridge']
  hostStep: LocalHostStepStore
  settings: AppSettings | null
  write: (patch: Partial<AppSettings>) => Promise<void>
  onRefresh: () => void
  onSkip: () => void
  onBack: () => void
  onContinue: () => void
}): JSX.Element {
  const step = useSyncExternalStore(hostStep.subscribe, hostStep.get)
  const [version, setVersion] = useState<string | null>(null)
  const [adding, setAdding] = useState(false)
  const ready = step?.step === 'ready'

  // The host's own version, once it answers: Floti desktop started it when none did.
  useEffect(() => {
    if (!ready) return
    let cancelled = false
    void bridge.invoke('app:info').then(
      (info) => {
        if (!cancelled) setVersion(info.version)
      },
      () => undefined
    )
    return () => {
      cancelled = true
    }
  }, [bridge, ready])

  const host: HostStepProps['host'] =
    step?.step === 'unreachable'
      ? { state: 'unknown', reason: step.reason }
      : step?.step === 'refused'
        ? { state: 'unknown', reason: step.sentence }
        : ready && version !== null
          ? { state: 'running', version }
          : { state: 'starting' }

  const quit: WalkthroughQuit = settings === null ? 'end' : quitChoice(settings)
  const setQuit = (choice: WalkthroughQuit): void => {
    void write({ quitSessions: { ...(settings?.quitSessions ?? {}), [LOCAL_HOST_KEY]: choice } }).catch(
      () => undefined
    )
  }

  return (
    <>
      <HostStep
        host={host}
        quit={quit}
        onQuit={setQuit}
        others={(settings?.hosts ?? []).map((added) => ({
          id: added.id,
          name: added.name,
          quit: settings === null ? 'keep' : quitChoice(settings, added.id)
        }))}
        onAddHost={() => setAdding(true)}
        onSkip={onSkip}
        onBack={onBack}
        onContinue={onContinue}
      />
      {adding && settings !== null && (
        <AddHostDialog
          actions={addHostActions}
          hosts={settings.hosts}
          onQuitChange={(id, choice) =>
            void write({ quitSessions: { ...settings.quitSessions, [id]: choice } }).catch(() => undefined)
          }
          onClose={() => {
            setAdding(false)
            // A host added here was written by main, which told the window in front.
            onRefresh()
          }}
        />
      )}
    </>
  )
}

/** The name step 4 suggests. */
const SUGGESTED_NAME = 'my-work'

function WorkspaceScreen({
  bridge,
  settings,
  onSkip,
  onBack,
  onDone
}: {
  bridge: SetupWindowProps['bridge']
  settings: AppSettings | null
  onSkip: () => void
  onBack: () => void
  /** The workspace made or chosen, or null when folders to scan were added. */
  onDone: (workspace: string | null) => void
}): JSX.Element {
  const [choice, setChoice] = useState<WorkspaceChoice>('new')
  const [name, setName] = useState(SUGGESTED_NAME)
  // A folder already scanned is the first choice; otherwise the effect below finds one.
  const [parent, setParent] = useState(() => settings?.scanRoots[0] ?? '')
  const [folder, setFolder] = useState('')
  const [folders, setFolders] = useState<string[]>([])
  const [problems, setProblems] = useState<string[]>([])
  const [busy, setBusy] = useState(false)

  // Somewhere plausible to create it inside: a folder already scanned, a
  // folder discovery can justify, or the home folder.
  const scanned = settings?.scanRoots[0]
  useEffect(() => {
    if (scanned !== undefined) return undefined
    let cancelled = false
    void Promise.allSettled([bridge.invoke('roots:suggest'), bridge.invoke('app:info')]).then(([suggested, info]) => {
      if (cancelled) return
      const found =
        (suggested.status === 'fulfilled' ? suggested.value[0] : undefined) ??
        (info.status === 'fulfilled' ? info.value.home : undefined)
      if (found !== undefined) setParent((typed) => (typed === '' ? found : typed))
    })
    return () => {
      cancelled = true
    }
  }, [bridge, scanned])

  const choose = async (title: string): Promise<string | null> =>
    (await bridge.invoke('path:chooseDirectory', { title })).path

  const submit = async (): Promise<void> => {
    setBusy(true)
    setProblems([])
    try {
      if (choice === 'find') {
        for (const path of folders) await bridge.invoke('roots:accept', { path })
        onDone(null)
        return
      }
      const result = await bridge.invoke(
        'workspace:create',
        choice === 'new'
          ? { mode: 'new', dir: parent, name: name.trim(), template: 'starter' }
          : { mode: 'convert', dir: folder }
      )
      if (result.path === null) {
        setProblems(result.problems)
        return
      }
      onDone(result.path)
    } catch (err) {
      setProblems([sentence(err)])
    } finally {
      setBusy(false)
    }
  }

  return (
    <WorkspaceStep
      choice={choice}
      onChoice={(next) => {
        setChoice(next)
        setProblems([])
      }}
      name={name}
      onName={setName}
      parent={parent}
      onChooseParent={() =>
        void choose('Create the workspace inside').then((path) => {
          if (path !== null) setParent(path)
        })
      }
      folder={folder}
      onChooseFolder={() =>
        void choose('Choose the folder to use').then((path) => {
          if (path !== null) setFolder(path)
        })
      }
      folders={folders}
      onAddFolders={() =>
        void choose('Choose a folder your workspaces are in').then((path) => {
          if (path === null) return
          setFolders((current) =>
            current.some((known) => known.toLowerCase() === path.toLowerCase()) ? current : [...current, path]
          )
        })
      }
      problems={problems}
      busy={busy}
      onSkip={onSkip}
      onBack={onBack}
      onSubmit={() => void submit()}
    />
  )
}
