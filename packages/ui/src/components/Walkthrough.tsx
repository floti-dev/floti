import type { JSX, ReactNode } from 'react'
import { useEffect, useRef } from 'react'
import { cn } from '../lib/cn'
import { Chip, type ChipTone } from './Chip'
import { dialogInput, dialogLabel, primaryButton, secondaryButton } from './DialogParts'
import {
  AgentIcon,
  CopyIcon,
  DocIcon,
  FlotiMarkIcon,
  FolderIcon,
  GearIcon,
  GlobeIcon,
  HistoryIcon,
  LayersIcon,
  MonitorIcon,
  PlugIcon,
  PlusIcon,
  RefreshIcon,
  ServerIcon,
  TerminalIcon
} from './icons'

/**
 * The guided first run (Floti Onboarding, "Guided first run"): five steps from
 * the install to the shell's home screen. Steps 1 to 4 are drawn in the setup
 * window, before the shell exists, and 5 in the shell's main area. It starts
 * no session: the first one starts the way every later one does.
 *
 * Every screen is a title, at most one short line and its choices. What a
 * step checks, writes and never touches is the desktop's business and not
 * text anybody reads here. These are presentational: the desktop reads the
 * agents and the host, writes the step and makes the workspace.
 */

/** How many steps there are. */
export const WALKTHROUGH_STEP_COUNT = 5

const ghostButton = 'rounded-well px-2 py-1.5 text-[12px] text-fg-muted transition-colors hover:text-fg'

/** The column every step is drawn in, with its step label, progress and foot. */
function StepFrame({
  step,
  label,
  title,
  lead,
  hero,
  children,
  foot
}: {
  step: number
  /** The label over the title: `Step 3 of 5`, unless the step says more. */
  label?: string
  title: string
  lead?: ReactNode
  /** Drawn over the title: step 1's mark. */
  hero?: ReactNode
  children?: ReactNode
  foot: ReactNode
}): JSX.Element {
  return (
    <section
      data-walkthrough-step={String(step)}
      aria-labelledby={`walkthrough-step-${String(step)}`}
      className="mx-auto flex h-full min-h-0 w-full max-w-[444px] flex-col"
    >
      <div className="mb-[18px] flex items-center justify-between">
        <span className={dialogLabel}>{label ?? `Step ${String(step)} of ${String(WALKTHROUGH_STEP_COUNT)}`}</span>
        <span aria-hidden className="flex gap-1">
          {Array.from({ length: WALKTHROUGH_STEP_COUNT }, (_, index) => (
            <i
              key={index}
              data-walkthrough-progress={index < step ? 'done' : 'todo'}
              className={cn('h-[2px] w-[22px] rounded-[1px]', index < step ? 'bg-accent' : 'bg-border-strong')}
            />
          ))}
        </span>
      </div>
      {hero}
      <h1
        id={`walkthrough-step-${String(step)}`}
        className="mb-1.5 text-[20px] leading-[1.3] font-medium tracking-[-0.01em] text-fg"
      >
        {title}
      </h1>
      {lead !== undefined && <p className="mb-4 text-[13px] text-fg-muted">{lead}</p>}
      <div className="min-h-0 flex-1 overflow-y-auto">{children}</div>
      <div className="mt-auto flex items-center gap-2 pt-4">{foot}</div>
    </section>
  )
}

/** The foot's buttons: Skip on the left, Back and the step's own action on the right. */
function Foot({
  skip,
  onSkip,
  onBack,
  primary,
  primaryReady = true,
  onPrimary,
  extra
}: {
  skip?: string
  onSkip?: () => void
  onBack?: () => void
  primary: ReactNode
  primaryReady?: boolean
  onPrimary: () => void
  /** Beside the primary, before it: Check again. */
  extra?: ReactNode
}): JSX.Element {
  const primaryRef = useRef<HTMLButtonElement>(null)
  useEffect(() => {
    primaryRef.current?.focus()
  }, [])
  return (
    <>
      {onSkip !== undefined && (
        <button type="button" data-walkthrough-skip onClick={onSkip} className={ghostButton}>
          {skip ?? 'Skip'}
        </button>
      )}
      <span className="flex-1" />
      {extra}
      {onBack !== undefined && (
        <button type="button" data-walkthrough-back onClick={onBack} className={secondaryButton}>
          Back
        </button>
      )}
      <button
        ref={primaryRef}
        type="button"
        data-walkthrough-continue
        disabled={!primaryReady}
        onClick={onPrimary}
        className={cn(primaryButton(primaryReady), 'flex items-center gap-1.5')}
      >
        {primary}
      </button>
    </>
  )
}

/** A row of the raised list: an icon, a name over a mono line, and what is at its end. */
function Row({
  icon,
  name,
  detail,
  end,
  children,
  ...rest
}: {
  icon: ReactNode
  name: string
  detail: string | null
  end: ReactNode
  children?: ReactNode
} & Record<`data-${string}`, unknown>): JSX.Element {
  return (
    <div {...rest} className="border-b border-border px-3 py-[9px] last:border-b-0">
      <div className="flex items-center gap-2.5">
        <span className="shrink-0 text-fg-subtle">{icon}</span>
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="truncate text-[12.5px] font-medium text-fg">{name}</span>
          {detail !== null && <span className="truncate font-mono text-[11px] text-fg-subtle">{detail}</span>}
        </span>
        <span className="flex shrink-0 items-center gap-1.5">{end}</span>
      </div>
      {children}
    </div>
  )
}

function Rows({ children }: { children: ReactNode }): JSX.Element {
  return <div className="overflow-hidden rounded-raised border border-border bg-surface-raised">{children}</div>
}

/** One choice of a set, drawn as the design's option card with its radio. */
function Option({
  on,
  title,
  detail,
  marker,
  onPick,
  ...rest
}: {
  on: boolean
  title: string
  detail?: string
  marker?: string
  onPick: () => void
} & Record<`data-${string}`, unknown>): JSX.Element {
  return (
    <button
      {...rest}
      type="button"
      role="radio"
      aria-checked={on}
      onClick={onPick}
      className={cn(
        'flex w-full items-start gap-2.5 rounded-raised border px-3 py-[9px] text-left transition-colors',
        on ? 'border-accent bg-accent-soft' : 'border-border bg-surface-raised hover:bg-hover'
      )}
    >
      <span
        aria-hidden
        className={cn(
          'mt-[2px] grid size-3.5 shrink-0 place-items-center rounded-full border-[1.5px]',
          on ? 'border-accent' : 'border-fg-subtle'
        )}
      >
        {on && <span className="size-[7px] rounded-full bg-accent" />}
      </span>
      <span className="flex min-w-0 flex-col">
        <span className="text-[12.5px] font-medium text-fg">
          {title}
          {marker !== undefined && <span className="ml-1.5 text-[10.5px] font-normal text-fg-subtle">{marker}</span>}
        </span>
        {detail !== undefined && <span className="text-[12px] leading-[1.45] text-fg-muted">{detail}</span>}
      </span>
    </button>
  )
}

function Problems({ problems }: { problems: readonly string[] }): JSX.Element | null {
  if (problems.length === 0) return null
  return (
    <ul
      role="alert"
      data-walkthrough-problems
      className="rounded-raised border border-danger/30 bg-danger/10 px-3 py-2 text-[12px] text-danger"
    >
      {problems.map((problem) => (
        <li key={problem}>{problem}</li>
      ))}
    </ul>
  )
}

// ---------------------------------------------------------------------------
// Step 1
// ---------------------------------------------------------------------------

export interface WelcomeStepProps {
  /** Opens the shell with nothing set up, and marks the walkthrough done. */
  onSkipSetup: () => void
  onGetStarted: () => void
}

export function WelcomeStep({ onSkipSetup, onGetStarted }: WelcomeStepProps): JSX.Element {
  return (
    <StepFrame
      step={1}
      title="Welcome to Floti"
      hero={
        <FlotiMarkIcon aria-hidden width={64} height={64} className="mt-[18px] mb-[22px] shrink-0 text-fg-subtle opacity-45" />
      }
      foot={<Foot skip="Skip setup" onSkip={onSkipSetup} primary="Get started" onPrimary={onGetStarted} />}
    >
      <p className="mb-2 text-[15px] text-fg">Be the captain of your fleet.</p>
      <p className="text-[13px] text-fg-muted">
        Run your coding agents side by side, on this PC or your other computers. A few steps and you’re ready.
      </p>
    </StepFrame>
  )
}

// ---------------------------------------------------------------------------
// Step 2
// ---------------------------------------------------------------------------

/**
 * One agent as its backend's own check found it on this PC. `unknown` is a
 * check that could not look, or a sign-in no check can tell, and is never
 * drawn as missing.
 */
export interface WalkthroughAgent {
  id: string
  name: string
  /** `claude 2.1.319`, or null when it is not installed or did not say. */
  version: string | null
  installed: boolean | 'unknown'
  signedIn: boolean | 'unknown'
  /** The line that installs it on this PC, for a missing one. */
  install: string
  /** The command that signs it in, for a signed-out one. */
  login: string
}

export interface AgentsStepProps {
  /** Null until the first check has answered. */
  agents: readonly WalkthroughAgent[] | null
  /** Why the check could not look, when it could not. */
  error: string | null
  checking: boolean
  onCheckAgain: () => void
  /** Puts a line on the clipboard: the install line or the login command. */
  onCopy: (text: string) => void
  onSkip: () => void
  onBack: () => void
  onContinue: () => void
}

function CommandLine({ text, onCopy }: { text: string; onCopy: (text: string) => void }): JSX.Element {
  return (
    <div className="mt-2 ml-[24px] flex items-center gap-1.5">
      <code
        data-walkthrough-command
        className="min-w-0 flex-1 truncate rounded-well border border-border bg-surface-sunken px-2.5 py-1.5 font-mono text-[11.5px] text-fg select-text"
        title={text}
      >
        {text}
      </code>
      <button
        type="button"
        aria-label={`Copy ${text}`}
        data-walkthrough-copy
        onClick={() => onCopy(text)}
        className="grid size-[28px] shrink-0 place-items-center rounded-well border border-border-strong text-fg-muted transition-colors hover:bg-hover hover:text-fg"
      >
        <CopyIcon width={13} height={13} />
      </button>
    </div>
  )
}

function agentChips(agent: WalkthroughAgent): { tone: ChipTone; label: string }[] {
  if (agent.installed === 'unknown') return [{ tone: 'neutral', label: 'Unknown' }]
  if (!agent.installed) return [{ tone: 'warn', label: 'Not installed' }]
  const signIn: { tone: ChipTone; label: string } =
    agent.signedIn === 'unknown'
      ? { tone: 'neutral', label: 'Sign-in unknown' }
      : agent.signedIn
        ? { tone: 'success', label: 'Signed in' }
        : { tone: 'warn', label: 'Signed out' }
  return [{ tone: 'success', label: 'Installed' }, signIn]
}

/** Whether an agent is missing something the user can put right: not installed, or signed out. */
function needsAFix(agent: WalkthroughAgent): boolean {
  return agent.installed === false || (agent.installed === true && agent.signedIn === false)
}

export function AgentsStep({
  agents,
  error,
  checking,
  onCheckAgain,
  onCopy,
  onSkip,
  onBack,
  onContinue
}: AgentsStepProps): JSX.Element {
  const missing = agents !== null && agents.some(needsAFix)
  const checkAgain =
    missing || (agents === null && error !== null) ? (
      <button
        type="button"
        data-walkthrough-check-again
        disabled={checking}
        onClick={onCheckAgain}
        className={cn(secondaryButton, 'flex items-center gap-1.5 disabled:opacity-50')}
      >
        <RefreshIcon width={13} height={13} className={cn(checking && 'animate-spin')} />
        Check again
      </button>
    ) : undefined
  return (
    <StepFrame
      step={2}
      title="Your agents"
      lead="Floti runs the agents you already have."
      foot={<Foot onSkip={onSkip} onBack={onBack} primary="Continue" onPrimary={onContinue} extra={checkAgain} />}
    >
      {agents === null ? (
        <p data-walkthrough-agents-pending className="text-[12px] text-fg-subtle">
          {error === null ? 'Looking…' : `Unknown: ${error}`}
        </p>
      ) : (
        <Rows>
          {agents.map((agent) => (
            <Row
              key={agent.id}
              data-walkthrough-agent={agent.id}
              icon={<TerminalIcon width={14} height={14} />}
              name={agent.name}
              detail={agent.version}
              end={agentChips(agent).map((chip) => (
                <Chip key={chip.label} tone={chip.tone}>
                  {chip.label}
                </Chip>
              ))}
            >
              {agent.installed === false && <CommandLine text={agent.install} onCopy={onCopy} />}
              {agent.installed === true && agent.signedIn === false && (
                <CommandLine text={agent.login} onCopy={onCopy} />
              )}
            </Row>
          ))}
        </Rows>
      )}
    </StepFrame>
  )
}

// ---------------------------------------------------------------------------
// Step 3
// ---------------------------------------------------------------------------

/** What quitting Floti does to this computer's sessions. */
export type WalkthroughQuit = 'end' | 'keep'

export interface HostStepProps {
  /** This computer's host: running with its version, starting, or not answering. */
  host: { state: 'running'; version: string } | { state: 'starting' } | { state: 'unknown'; reason: string }
  quit: WalkthroughQuit
  onQuit: (quit: WalkthroughQuit) => void
  /** The hosts on other computers added so far, with what quitting does there. */
  others: readonly { id: string; name: string; quit: WalkthroughQuit }[]
  onAddHost: () => void
  onSkip: () => void
  onBack: () => void
  onContinue: () => void
}

const HOST_QUIT: readonly { value: WalkthroughQuit; title: string; detail: string }[] = [
  { value: 'end', title: 'End my sessions', detail: 'Like closing a terminal.' },
  { value: 'keep', title: 'Keep them running', detail: 'Pick them up when you open Floti again.' }
]

export function HostStep({
  host,
  quit,
  onQuit,
  others,
  onAddHost,
  onSkip,
  onBack,
  onContinue
}: HostStepProps): JSX.Element {
  const chip: { tone: ChipTone; label: string } =
    host.state === 'running'
      ? { tone: 'success', label: 'Running' }
      : host.state === 'starting'
        ? { tone: 'neutral', label: 'Starting…' }
        : { tone: 'neutral', label: 'Unknown' }
  return (
    <StepFrame
      step={3}
      title="The host on this computer"
      lead="Your sessions run in the background on this PC."
      foot={<Foot onSkip={onSkip} onBack={onBack} primary="Continue" onPrimary={onContinue} />}
    >
      <div className="grid gap-3">
        <Rows>
          <Row
            data-walkthrough-host="local"
            data-walkthrough-host-state={host.state}
            icon={<MonitorIcon width={14} height={14} />}
            name="This PC"
            detail={host.state === 'running' ? `floti ${host.version}` : host.state === 'unknown' ? host.reason : null}
            end={<Chip tone={chip.tone}>{chip.label}</Chip>}
          />
        </Rows>
        <span className={dialogLabel}>When you quit Floti</span>
        <div role="radiogroup" aria-label="When you quit Floti" className="grid gap-1.5">
          {HOST_QUIT.map((option) => (
            <Option
              key={option.value}
              data-walkthrough-quit={option.value}
              on={quit === option.value}
              title={option.title}
              detail={option.detail}
              {...(option.value === 'end' ? { marker: 'Default' } : {})}
              onPick={() => onQuit(option.value)}
            />
          ))}
        </div>
        <p className="text-[11.5px] text-fg-subtle">You can change this in Settings.</p>
        <span className={dialogLabel}>Other computers</span>
        {others.length > 0 && (
          <Rows>
            {others.map((other) => (
              <Row
                key={other.id}
                data-walkthrough-other-host={other.id}
                icon={<ServerIcon width={14} height={14} />}
                name={other.name}
                detail={null}
                end={
                  <Chip tone="neutral">{other.quit === 'keep' ? 'Keeps sessions running' : 'Ends sessions on quit'}</Chip>
                }
              />
            ))}
          </Rows>
        )}
        <div className="flex items-center gap-1.5">
          <span className="flex-1 text-[11.5px] text-fg-subtle">Optional: run sessions on a server or another PC.</span>
          <button
            type="button"
            data-walkthrough-add-host
            onClick={onAddHost}
            className={cn(secondaryButton, 'flex items-center gap-1.5')}
          >
            <PlusIcon width={13} height={13} />
            Add a host…
          </button>
        </div>
      </div>
    </StepFrame>
  )
}

// ---------------------------------------------------------------------------
// Step 4
// ---------------------------------------------------------------------------

/** How step 4 gets a workspace: made from Starter, a folder used as it is, or the folders to scan. */
export type WorkspaceChoice = 'new' | 'convert' | 'find'

export interface WorkspaceStepProps {
  choice: WorkspaceChoice
  onChoice: (choice: WorkspaceChoice) => void
  /** Create a new one: its name and the folder it goes inside. */
  name: string
  onName: (name: string) => void
  parent: string
  onChooseParent: () => void
  /** Use a folder I have: the folder, or empty until one is chosen. */
  folder: string
  onChooseFolder: () => void
  /** Find the ones I have: the folders chosen to scan. */
  folders: readonly string[]
  onAddFolders: () => void
  problems: readonly string[]
  busy: boolean
  onSkip: () => void
  onBack: () => void
  onSubmit: () => void
}

const WORKSPACE_CHOICES: readonly { value: WorkspaceChoice; title: string; detail: string }[] = [
  { value: 'new', title: 'Create a new one', detail: 'A fresh folder, ready for your repositories.' },
  { value: 'convert', title: 'Use a folder I have', detail: 'Nothing in it moves or changes.' },
  { value: 'find', title: 'Find the ones I have', detail: 'Point Floti at where they are.' }
]

/** What step 4's action says for each choice. */
const SUBMIT: Record<WorkspaceChoice, string> = {
  new: 'Create workspace',
  convert: 'Use this folder',
  find: 'Continue'
}

/** Whether step 4 has what its choice needs. */
export function workspaceStepReady(
  props: Pick<WorkspaceStepProps, 'choice' | 'name' | 'parent' | 'folder' | 'folders'>
): boolean {
  if (props.choice === 'new') return props.name.trim() !== '' && props.parent.trim() !== ''
  if (props.choice === 'convert') return props.folder.trim() !== ''
  return props.folders.length > 0
}

function PathField({
  label,
  path,
  onChoose,
  ...rest
}: {
  label: string
  path: string
  onChoose: () => void
} & Record<`data-${string}`, unknown>): JSX.Element {
  return (
    <label className="flex min-w-0 flex-1 flex-col gap-[5px]">
      <span className={dialogLabel}>{label}</span>
      <span className="flex items-center gap-1.5">
        <input
          {...rest}
          readOnly
          value={path}
          title={path}
          aria-label={label}
          className={cn(dialogInput, 'min-w-0 flex-1 font-mono text-[11px] text-fg-muted')}
        />
        <button type="button" onClick={onChoose} className={cn(secondaryButton, 'shrink-0')}>
          Choose…
        </button>
      </span>
    </label>
  )
}

export function WorkspaceStep(props: WorkspaceStepProps): JSX.Element {
  const { choice, onChoice, name, onName, parent, onChooseParent, folder, onChooseFolder, folders, onAddFolders } =
    props
  const ready = workspaceStepReady(props) && !props.busy
  return (
    <StepFrame
      step={4}
      title="A workspace"
      lead="A folder for your repositories and the instructions agents follow there."
      foot={
        <Foot
          onSkip={props.onSkip}
          onBack={props.onBack}
          primary={SUBMIT[choice]}
          primaryReady={ready}
          onPrimary={props.onSubmit}
        />
      }
    >
      <div className="grid gap-3">
        <div role="radiogroup" aria-label="A workspace" className="grid gap-1.5">
          {WORKSPACE_CHOICES.map((option) => (
            <Option
              key={option.value}
              data-walkthrough-workspace-choice={option.value}
              on={choice === option.value}
              title={option.title}
              detail={option.detail}
              onPick={() => onChoice(option.value)}
            />
          ))}
        </div>
        {choice === 'new' && (
          <div className="flex items-end gap-1.5">
            <label className="flex w-[132px] shrink-0 flex-col gap-[5px]">
              <span className={dialogLabel}>Name</span>
              <input
                data-walkthrough-workspace-name
                value={name}
                onChange={(event) => onName(event.target.value)}
                spellCheck={false}
                className={dialogInput}
              />
            </label>
            <PathField
              label="Create it inside"
              path={parent}
              onChoose={onChooseParent}
              data-walkthrough-workspace-parent
            />
          </div>
        )}
        {choice === 'convert' && (
          <PathField label="Folder" path={folder} onChoose={onChooseFolder} data-walkthrough-workspace-folder />
        )}
        {choice === 'find' && (
          <div className="flex flex-col gap-[5px]">
            <span className={dialogLabel}>Folders</span>
            {folders.length > 0 && (
              <Rows>
                {folders.map((path) => (
                  <div
                    key={path}
                    data-walkthrough-scan-folder={path}
                    title={path}
                    className="flex items-center gap-2 border-b border-border px-3 py-1.5 last:border-b-0"
                  >
                    <FolderIcon width={13} height={13} className="shrink-0 text-fg-subtle" />
                    <span className="min-w-0 truncate font-mono text-[11px] text-fg-muted">{path}</span>
                  </div>
                ))}
              </Rows>
            )}
            <span>
              <button
                type="button"
                data-walkthrough-add-folders
                onClick={onAddFolders}
                className={cn(secondaryButton, 'flex items-center gap-1.5')}
              >
                <PlusIcon width={13} height={13} />
                Choose…
              </button>
            </span>
          </div>
        )}
        <Problems problems={props.problems} />
      </div>
    </StepFrame>
  )
}

// ---------------------------------------------------------------------------
// Step 5
// ---------------------------------------------------------------------------

export interface TourStepProps {
  onSkip: () => void
  onBack: () => void
  onContinue: () => void
}

const TOUR: readonly { key: string; icon: ReactNode; name: string; what: string; keys?: string }[] = [
  { key: 'sessions', icon: <TerminalIcon width={15} height={15} />, name: 'Sessions', what: 'Workspaces and running agents' },
  { key: 'profiles', icon: <LayersIcon width={15} height={15} />, name: 'Profiles', what: 'Saved launches' },
  { key: 'agents', icon: <AgentIcon width={15} height={15} />, name: 'Agents', what: 'Roles with their own instructions' },
  { key: 'files', icon: <DocIcon width={15} height={15} />, name: 'Files', what: 'Notes and docs, rendered' },
  { key: 'browser', icon: <GlobeIcon width={15} height={15} />, name: 'Browser', what: 'Pages for you and your agents' },
  { key: 'plugins', icon: <PlugIcon width={15} height={15} />, name: 'Plugins', what: 'Panels and tools you add' },
  { key: 'settings', icon: <GearIcon width={15} height={15} />, name: 'Settings', what: 'Hosts, themes and this tour' },
  {
    key: 'history',
    icon: <HistoryIcon width={15} height={15} />,
    name: 'History',
    what: 'Find and resume past sessions',
    keys: 'Ctrl+H'
  }
]

export function TourStep({ onSkip, onBack, onContinue }: TourStepProps): JSX.Element {
  return (
    <StepFrame
      step={5}
      title="Around the window"
      lead="A quick look around."
      foot={<Foot onSkip={onSkip} onBack={onBack} primary="Continue" onPrimary={onContinue} />}
    >
      <ul className="grid grid-cols-2 gap-x-3.5 gap-y-0.5">
        {TOUR.map((place) => (
          <li key={place.key} data-walkthrough-tour={place.key} className="flex items-start gap-2.5 py-[7px]">
            <span className="mt-[2px] shrink-0 text-fg-subtle">{place.icon}</span>
            <span className="flex min-w-0 flex-col">
              <span className="text-[12.5px] font-medium text-fg">
                {place.name}
                {place.keys !== undefined && (
                  <span className="ml-1.5 font-mono text-[11px] font-normal text-fg-subtle">{place.keys}</span>
                )}
              </span>
              <span className="text-[11.5px] leading-[1.45] text-fg-muted">{place.what}</span>
            </span>
          </li>
        ))}
      </ul>
    </StepFrame>
  )
}
