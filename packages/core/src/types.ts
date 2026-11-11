/**
 * The vocabulary every surface shares. Nothing here may reference Electron,
 * the DOM, or a database driver - these are the shapes that cross the IPC wire
 * and get persisted, and both sides have to agree on them.
 *
 * It is also the one module the renderer may import *values* from, so the two
 * config parsers the editor needs before it is allowed to save are re-exported
 * through here. Both files they come from are pure by construction; adding a
 * `node:` import to either would break the renderer bundle at rollup rather
 * than at typecheck (CLAUDE.md, hard rules).
 */

// Imported as well as re-exported: `AppSettings` below names it, and a
// re-export alone does not bring a name into this file's scope.
import type { UsageDisplayMode } from './usage/shape'
// The same, for `CreateConfigRequest` below.
// The same again: `AppSettings.browserReach` names it.
import type { BrowserReach, BrowserSearch } from './browser/reach'
import type { SavedPaneLayout } from './layout/panes'
// A plugin's tab is a `WorkspaceTab`, and its rail id may be in `railHidden`.
import type { PluginPage, PluginParams, PluginRailId } from './plugins/tabs'
// `AppSettings.hosts` names it, and `quitEndsSessions` reads the key.
import { LOCAL_HOST_KEY, type RemoteHost } from './hosts'

/**
 * The browser pane's URL rules, re-exported here rather than from the package
 * root for the reason `USAGE_DISPLAY_MODES` is: the
 * address bar and the settings pane are *renderer* code, and a value import
 * from `@helm/core` reaches the filesystem through `launch/` and `store/`,
 * which fails at rollup rather than at typecheck. These are pure by
 * construction - they decide about a string and touch nothing.
 */
export {
  agentReach,
  BROWSER_REACH_MODES,
  BROWSER_SEARCH_ENGINES,
  browserReachAllows,
  isLoopbackUrl,
  resolveBrowserAddress,
  browserSearchName,
  type BrowserReach,
  type BrowserSearch,
  type ReachDecision
} from './browser/reach'

/**
 * An agent file as the Agents page edits it, re-exported for the same
 * reason: the page's form reads and writes its draft with it on every edit.
 * `agents/agentFile.ts` imports nothing.
 */
export * from './agents/agentFile'

/**
 * Themes, re-exported for the same reason: the renderer paints the tokens and
 * the Appearance pane draws every theme's swatch, both in the browser bundle.
 * `theme/themes.ts` and `theme/color.ts` import nothing but types and each
 * other. Reading theme files off disk is `theme/load.ts`, which only the
 * package root reaches.
 */
export * from './theme/color'
export * from './theme/themes'

/**
 * The pane layout, re-exported for the same reason: the renderer arranges tabs
 * into groups with it on every render. `layout/panes.ts` imports nothing but a
 * type from this file, the host key from `hosts.ts`, which is as pure, and the
 * family modules beside it, which import nothing at all. The family view's
 * room rule, state and layout operations are re-exported with it: the window
 * draws and arranges the family view from them.
 */
export * from './layout/panes'
export * from './layout/pluginPages'
export * from './layout/family'
export * from './layout/familyRoom'
export * from './layout/familyState'
export * from './layout/familyReport'
export * from './layout/listed'
export * from './layout/familyView'
export * from './layout/childPlace'

/**
 * A plugin's tab and rail id, re-exported for the same reason: the renderer
 * opens plugin tabs and names their rail buttons. `plugins/tabs.ts` imports
 * only the SDK's patterns, which are pure.
 */
export * from './plugins/tabs'
export * from './plugins/theme'
export type * from './plugins/info'

/**
 * The Files view's ranking and its sentences about git, re-exported for the
 * same reason: Ctrl+P ranks on every keystroke in the window, and the tree
 * paints the letters. Both modules import nothing but types from this file.
 */
export * from './files/fuzzy'
export * from './files/shape'

/**
 * The hosts this device reaches on other computers, re-exported for the same
 * reason: Settings > Hosts checks its add form in the window. `hosts.ts`
 * imports nothing.
 */
export * from './hosts'

/** Installing floti on a host's computer: what the check found and what is offered, said in the window. */
export * from './hostInstall'

/** Adding a host by connecting to it, what went wrong said one way everywhere, its agents, and the ssh config's Host names. */
export * from './hostAdd'
export * from './hostFailure'
export * from './hostAgents'
export * from './sshConfig'

/**
 * The usage reader's pure half, re-exported for the same reason: the status bar
 * re-derives what it may paint on a timer, from the same functions the main
 * process parsed with. Two implementations of "is this reading still good" is
 * exactly the bug this file exists to prevent.
 */
export {
  describeAge,
  nextUsageMode,
  offerableUsageModes,
  parseUsage,
  usageProblem,
  usageView,
  COST_MODE_UNAVAILABLE,
  USAGE_DISPLAY_MODES,
  USAGE_STALE_AFTER_MS,
  type UsageBucket,
  type UsageDisplayMode,
  type UsageGroup,
  type UsageLimit,
  type UsageProblem,
  type UsageProblemKind,
  type UsageSeverity,
  type UsageSnapshot,
  type UsageSpend,
  type UsageTokens,
  type UsageView,
  type UsageWindowCost
} from './usage/shape'
export {
  costOfTokens,
  priceFor,
  priceTableAgeDays,
  PRICES,
  PRICE_TABLE_DATE,
  PRICE_TABLE_FRESH_FOR_DAYS,
  type ModelPrice,
  type TokenPrice
} from './usage/prices'

/**
 * What the editors do when you press a key.
 *
 * Re-exported here rather than reached for through the package root, because
 * the component that calls them runs in the browser bundle and this file is the
 * one entry point with no `node:` behind it. `content/editing.ts` imports
 * nothing at all, which is what makes that safe - and is why the editing rules
 * live away from both the DOM and shiki.
 */
export {
  backspaceAction,
  caretAt,
  editorExtension,
  editorKeyAction,
  enterAction,
  findMatchesIn,
  indentAction,
  lineStarts,
  pairAction,
  syntaxFor,
  wrapsByDefault,
  type EditAction,
  type EditorSyntax
} from './content/editing'

/** What a discovered directory turned out to be. */
export type ProjectKind =
  /** Has a `harness.yaml`. Its `repos/*` children are projects in their own right. */
  | 'harness'
  /** A repo inside a harness's `repos/` directory. */
  | 'repo'
  /** A directory that is neither, scanned because a root path pointed at it. */
  | 'folder'

/**
 * Counts of what a project's `.claude/` directory actually contains.
 *
 * Counted the way Claude Code resolves them, not the way a file listing would:
 * a skill is a directory holding a `SKILL.md`, and commands and agents are
 * markdown files at any depth (`commands/spec/plan.md` is the `/spec:plan`
 * command, so a top-level count of that tree would report 1 instead of 20).
 */
export interface ClaudeInventory {
  skills: number
  commands: number
  agents: number
  hooks: boolean
  settings: boolean
  claudeMd: boolean
  mcp: boolean
}

export const EMPTY_INVENTORY: ClaudeInventory = {
  skills: 0,
  commands: 0,
  agents: 0,
  hooks: false,
  settings: false,
  claudeMd: false,
  mcp: false
}

/** Working-tree summary for the launcher's per-project chips. */
export interface GitState {
  branch: string | null
  /** Detached HEAD, mid-rebase, or otherwise not on a named branch. */
  detached: boolean
  /** Files with any staged, unstaged, or untracked change. */
  dirty: number
  ahead: number
  behind: number
  /** Set when the directory is a repo but git could not answer. */
  error?: string
}

export interface Project {
  /** Absolute, normalised path. Stable across scans; the identity of a project. */
  path: string
  name: string
  kind: ProjectKind
  /** Path of the harness this project belongs to, if any. */
  harnessPath: string | null
  hasClaudeDir: boolean
  inventory: ClaudeInventory
  git: GitState | null
}

export interface Harness {
  path: string
  name: string
  /** Parsed from `harness.yaml` when present. */
  template: string | null
  version: string | null
  /** Absolute paths of the projects found under `repos/`. */
  repoPaths: string[]
}

export interface DiscoveryResult {
  /** The root paths that were scanned. */
  roots: string[]
  harnesses: Harness[]
  projects: Project[]
  /** Roots that could not be read, with the reason. */
  errors: Array<{ path: string; message: string }>
  scannedAt: string
  durationMs: number
}

/**
 * How a hosted session ended, or that it has not.
 *
 * `lost` is the honest answer for a row that was still `running` when the
 * process that owned it went away - a crash, a kill from Task Manager, a power
 * cut. The alternative, stamping an end time at the next launch, would invent a
 * duration nobody measured.
 */
export type SessionStatus = 'running' | 'exited' | 'lost'

/** One hosted `claude` process, from spawn to exit. */
export interface SessionRecord {
  id: number
  /** The `-n` name handed to the CLI, so the session is identifiable in
   * `/resume` later (SPEC 4.1, and the session index reads these rows). Never
   * rewritten - a rename writes `label`. See the column comment in `schema.ts`. */
  name: string
  /** What Helm calls it on screen, or null for "use `name`". `sessionLabel`. */
  label: string | null
  /**
   * What the session is doing, in its agent's words (`session_title`): one
   * line of at most 60 characters, drawn beside its name. Absent until the
   * agent sets one, and from a host before protocol 2.2.
   */
  title?: string
  cwd: string
  /** The branch `cwd` was on when this was spawned. Null for a non-repo cwd, a
   * detached HEAD, or a read that failed. Captured, never followed. */
  branch: string | null
  /** The discovered project it was launched against, if it was one. */
  projectPath: string | null
  /** The profile it was launched from, if it was one. Not a foreign key: the
   * session is a record of what happened and outlives a deleted profile. */
  profileId: number | null
  /** Argv after the executable, as spawned. */
  argv: string[]
  /**
   * The Claude Code conversation id this session is having, or null.
   *
   * **Assigned, not discovered.** Helm mints a uuid and hands it over in argv
   * (`--session-id`), so the row and the conversation agree from the first
   * instant rather than from whenever something managed to read one out of the
   * other. A resumed session carries the id it resumed, because that is the
   * conversation it is.
   *
   * Null for a session launched before this column existed, and for one
   * launched against a CLI with no `--session-id` flag. Both are read as "no
   * durable join", never as an error.
   *
   * Three measured facts shape what it may be used for, all against 2.1.238:
   *
   *   - **A `/clear` re-registers under a new id, same process.** So this is
   *     the id the conversation *started* as, and a live join may not depend on
   *     it staying current - see `joinSessionRegistry`, which uses it once to
   *     learn the process and then follows the process.
   *   - **`/compact` keeps it.** A compaction is the same conversation.
   *   - **A uuid that already exists is refused**: `Error: Session ID <uuid> is
   *     already in use.` and exit 1. So an id is minted per launch and never
   *     reused, and this column is a record rather than something to launch
   *     from.
   */
  claudeSessionId: string | null
  status: SessionStatus
  startedAt: string
  endedAt: string | null
  durationMs: number | null
  /** Null while running, and for a session whose exit code was never observed. */
  exitCode: number | null
  /**
   * The session that started this one with the host's agent tools, by its row
   * id. Absent for a session a person started. It names the parent after the
   * parent has ended, which is what marks a child that outlived it.
   */
  parentId?: number | null
  /** What the parent is called on screen, or null. */
  parentName?: string | null
  /** The agent the session was started as (`reviewer`, `worker`), or null for one started as none. */
  agent?: string | null
  /**
   * The command line a command child runs, as its parent gave it after `--`.
   * Absent for every other session. Its exit code is `exitCode`, once it has
   * exited.
   */
  command?: string | null
  /**
   * Whether the host ended it - its parent stopped it, a person ended it, or
   * the host went down with it - as against its agent exiting by itself. Only
   * a session that has ended carries it, and absent reads as false: the end of
   * a child reads "Stopped" when it is set and "Done" when it is not
   * (`endedHeaderText`).
   */
  stopped?: boolean
  /**
   * How it ended, once it has exited: its program exited by itself, or who
   * stopped it (`stoppedText` words it). Absent while it runs, for a lost
   * session, and from a host before protocol 2.2, where `stopped` alone says
   * the host ended it.
   */
  endedBy?: EndedBy
  /**
   * Whether this child, stopped, stays under its parent in the Sessions tree
   * and the family view: its parent stopped it with keep, or a person stopped
   * it. Any other ended child leaves both (`inFamilyTree`). Cleared when it is
   * resumed and when a person removes it (`session:remove`). Absent reads
   * false, as it does from a host before protocol 2.2.
   */
  kept?: boolean
  /**
   * What this child last told the session that started it with
   * `agents_report`, until its next turn starts. Absent for a session that has
   * told nothing since.
   */
  lastReport?: ChildReport
  /** Whether it was started to keep running when its parent ends. Absent reads as false. */
  outlive?: boolean
  /** The permission mode it was launched in. Absent for one launched with none. */
  permissionMode?: PermissionMode
  /**
   * What it was launched with beyond its agent, profile and permission mode:
   * the sessions pane's Launched as. Absent for a plain command line, a row
   * from before it, and from a host before protocol 2.3.
   */
  launchedAs?: LaunchedAs
}

/** What a session was launched with, as its launch composed it. */
export interface LaunchedAs {
  /** The agent file it was composed from, as the host spells its path; null for none and the host's built-in default. */
  agentFile: string | null
  /** Null for the backend's own. */
  model: string | null
  /** Null for the backend's own. */
  effort: EffortLevel | null
  allowedTools: string[]
  disallowedTools: string[]
  /** The agent tool families its launch was given, such as `floti-agents`. */
  tools: string[]
  /** Whether it works in an agent worktree Floti made for it. */
  worktree: boolean
}

/**
 * How a session that has exited came to end: `exited` by itself, or stopped by
 * the session that started it (`parent`), by a person on any device
 * (`person`), or by Floti when its parent ended or the host was stopped
 * (`floti`).
 */
export type EndedBy = 'exited' | 'parent' | 'person' | 'floti'

/** A child's report to its parent: its state, its words and when it was made. */
export interface ChildReport {
  /** `done`, `stuck` or `note`. */
  state: string
  text: string
  /** ISO 8601. */
  at: string
}

/**
 * What a live Claude Code session says it is doing.
 *
 * These four are the CLI's own vocabulary, not Helm's: they are the enum its
 * registry validator accepts, and Helm neither adds to it nor collapses it.
 * Confirmed against **2.1.238** by driving a real session through every one of
 * them and keeping the records - see `packages/core/src/registry/registry.ts`
 * for what each one was provoked with.
 *
 * A const array because a component maps it to a tone, which makes it a
 * *value* in the browser bundle - so it lives in `types.ts`, the one entry
 * point safe to import values from (CLAUDE.md "Boundaries").
 */
export const SESSION_ACTIVITIES = ['busy', 'shell', 'idle', 'waiting', 'error'] as const
export type SessionActivity = (typeof SESSION_ACTIVITIES)[number]

/**
 * One record out of `~/.claude/sessions/<pid>.json`, parsed tolerantly.
 *
 * Every field but `pid` and `file` is optional, because every field but those
 * two has been observed absent: a record is written the instant a session
 * registers and gains its `status` only when the interactive loop publishes
 * one, so the very first record of every session has no status at all.
 *
 * This is undocumented CLI internals. The parse is deliberately shaped so that
 * a field going missing, changing type or gaining a sibling costs a value and
 * never an exception.
 */
export interface SessionRegistryEntry {
  /** The file this came out of, for diagnostics. `<pid>.json`. */
  file: string
  /**
   * The pid Claude Code registered under - **its own**, which is not always the
   * one Helm holds. Through a `.cmd` shim the pty is `cmd.exe` and `claude.exe`
   * is its child, so the two differ; measured, pty 23496 against registry 4068.
   */
  pid: number
  /**
   * Windows FILETIME of the process's creation, as a decimal string, or null.
   *
   * The one thing that makes a pid safe to join on. Claude Code's own registry
   * sweep does not compare it and is therefore blind to pid reuse; a reader
   * joining on pid should not be.
   */
  procStart: string | null
  /** The conversation id. Changes under a `/clear` without the pid changing. */
  sessionId: string | null
  cwd: string | null
  /** The `-n` name. */
  name: string | null
  /** The CLI version that wrote the record. */
  version: string | null
  /** `cli` for an interactive run, `sdk-cli` for a `-p` one. */
  entrypoint: string | null
  /** Epoch ms the process registered at. */
  startedAt: number | null
  /**
   * The published status, or null where the record carried none **or carried
   * one this build does not recognise**.
   *
   * Those two are deliberately the same value here and are told apart by
   * `rawStatus`: both mean "Helm has nothing to say", which is the only honest
   * answer to a status it cannot interpret. A CLI that renames one of the four
   * therefore degrades to painting what the tab painted before this existed,
   * never to a guess.
   */
  activity: SessionActivity | null
  /** Exactly what the `status` field held, whatever it was. Null when absent. */
  rawStatus: string | null
  /**
   * Why the session is blocked, when it is - the CLI's own sentence.
   *
   * Two were measured on 2.1.238: `"dialog open"` for a slash command that
   * renders a UI, and `"permission prompt"` for a tool call awaiting approval.
   * Carried verbatim and never matched against, because it comes from whatever
   * dialog is on top rather than from a fixed list.
   */
  waitingFor: string | null
  /**
   * When the status last changed, epoch ms, or null.
   *
   * **Its age says nothing about staleness.** The file is written on
   * transition and never on a timer - a `busy` record carrying an 18-minute-old
   * stamp is an ordinary long tool call. Liveness is the only test, and it is
   * `probeProcess`. Present for diagnostics; nothing in Helm may infer from it.
   */
  statusUpdatedAt: number | null
}

/**
 * What one of Helm's own sessions is doing, ready for the tab.
 *
 * Null activity is the ordinary state, not an error: a session whose registry
 * record has not appeared yet, whose process cannot be proved alive, or that is
 * publishing a status this build does not know. All three paint what the tab
 * painted before any of this existed.
 */
export interface SessionActivityState {
  /** Helm's own session row id. */
  id: number
  /**
   * From the backend's own records where it keeps them, or else from the
   * OSC 7501 reports the session's program writes. `error` only ever comes
   * from a report.
   */
  activity: SessionActivity | null
  /**
   * What a `waiting` session waits on: the CLI's own sentence, or for a
   * reported block its kind in words (`permission prompt`, `question`,
   * `sign-in`). Null otherwise.
   */
  waitingFor: string | null
  /** The program's own message for an `error`, at most 200 characters. Null otherwise. */
  message: string | null
  /**
   * The conversation id the registry is currently reporting under.
   *
   * Not necessarily `SessionRecord.claudeSessionId`: a `/clear` gives the same
   * process a new one, and this is the live value.
   */
  claudeSessionId: string | null
}

/**
 * A session the last run was hosting when it stopped without shutting down,
 * as the restore offer lists it.
 */
export interface RestorableSession {
  /** Its row, now `lost`. */
  id: number
  /** What its tab said: `sessionLabel` of the row. */
  name: string
  cwd: string
  /** The branch it was started on, as its tab's crumb had it. */
  branch: string | null
  /** The profile it was launched from, by name, or null for none. */
  profile: string | null
  /** True when it had a profile that has since been deleted. It reopens without. */
  profileGone: boolean
  /** When it was last spoken to, ms since epoch. Null when that is not known. */
  lastAt: number | null
  /** Why it cannot be reopened, as a sentence, or null when it can. */
  blocked: string | null
  /** The agent it was started as, or absent for none. */
  agent?: string
  /**
   * Why its agent cannot be composed again (it is gone, or its file cannot be
   * loaded), as the host's sentence naming it; absent when it can. Such a
   * session is offered unticked, and restoring it restores it without its
   * agent.
   */
  agentProblem?: string
}

/** What a crash took, offered back once at the next start. */
export interface RestoreOffer {
  /** In the order their tabs were in. */
  sessions: RestorableSession[]
  /**
   * How many more the record calls lost whose conversation is still running -
   * in another Helm, or a terminal. Left alone, because reopening one would be
   * a second process on one conversation.
   */
  elsewhere: number
  /** The panes as the stopped run last wrote them. `placeRestored` reads it. */
  layout: SavedPaneLayout | null
}

/**
 * One live Claude Code session on this machine, whoever started it.
 *
 * **Listing is machine-wide; detail is Helm's own.** The registry Claude Code
 * keeps is not Helm's - a `claude` in Windows Terminal writes a record beside
 * anything Helm hosts - and a listing that only showed Helm's own would answer
 * "is anybody working in this tree" wrongly in exactly the case somebody gets
 * hurt by it. So every record is listed, and the fields below are the whole of
 * what a record carries. Everything richer - branch, profile, overlays, argv,
 * the process tree, the ports - hangs off `helmSessionId`, because Helm has
 * none of it for a session it did not spawn. That degradation needs no special
 * case: it is simply what is knowable.
 *
 * This is the same rule `browser_tabs` established, arrived at from the other
 * side: listing is not driving.
 */
export interface LiveSession {
  /**
   * Helm's own session row id, or null for a session Helm does not host.
   *
   * The whole of the hosted/not distinction, and it is a join key rather than a
   * flag: everything Helm knows beyond the record is reached through it.
   */
  helmSessionId: number | null
  /**
   * The pid the session is registered under, or the pty's pid for a hosted
   * session that has no record yet.
   *
   * Not necessarily the same process in the two cases: through a `.cmd` shim
   * the pty is `cmd.exe` and `claude.exe` registers under its own pid. Which
   * one this is, is what `registered` says.
   */
  pid: number
  /**
   * Whether a registry record was found for it.
   *
   * False is an ordinary state rather than an error, and there are two measured
   * ways into it: the second between spawn and registration, and a session
   * sitting on the workspace-trust prompt, which registers only after the
   * startup gates. A Helm session in that state is listed anyway - Helm knows
   * its own processes are running - carrying what its row says and nothing the
   * record would have added.
   */
  registered: boolean
  /** The working directory. From the record, or from Helm's row. */
  cwd: string | null
  /** What it is called: the `-n` name, or Helm's own label for a hosted one. */
  name: string | null
  activity: SessionActivity | null
  /** The CLI's own sentence for a `waiting` session, verbatim, or a reported block's kind in words. */
  waitingFor: string | null
  /** The program's own message for an `error`, at most 200 characters. Null otherwise. */
  message: string | null
  /**
   * When the status above was last published, epoch ms, or null.
   *
   * Carried so a surface can say **how long** a session has been busy, or
   * waiting, which is the difference between "it is working" and "it has been
   * working for forty minutes". That is a different claim from the one
   * `SessionRegistryEntry.statusUpdatedAt` forbids, and the two are worth
   * keeping apart: the record is written on transition and never on a timer, so
   * its age says exactly how long this status has held, and says **nothing**
   * about whether the session is still alive. Liveness is `probeProcess` and
   * only `probeProcess` - by the time a value reaches this field the record has
   * already been through that filter.
   */
  statusSinceMs: number | null
  /** The CLI version that wrote the record. Null for an unregistered one. */
  version: string | null
  /** `cli` for an interactive run, `sdk-cli` for a `-p` one. */
  entrypoint: string | null
  /** Epoch ms the session registered at, or Helm's own start time. */
  startedAtMs: number | null
  /** The conversation id the registry is reporting under. */
  claudeSessionId: string | null
}

/** Every live session on the machine, as one pass saw them. */
export interface SessionsOverview {
  /** Hosted first, then the rest; each group by start time, newest last. */
  sessions: LiveSession[]
  /** When the registry was read, epoch ms. */
  readAtMs: number
}

/**
 * One process in a session's tree.
 *
 * **Helm spawned the pty, so the tree under it is Helm's own process tree**,
 * not a reading of anybody's conversation. That is the whole argument for this
 * being in scope: nothing here parses session output or infers anything from
 * what a model said - it asks the operating system what the children of a
 * process Helm started are, which is a question Helm is entitled to ask about
 * its own children and about nothing else.
 */
export interface SessionProcess {
  pid: number
  parentPid: number
  /** The image name, e.g. `docker.exe`. Machine data, so it renders in mono. */
  name: string
  /**
   * The full command line, or null where the host would not give it up.
   *
   * Null is routine rather than exceptional and it is **not** an error: on this
   * machine 159 of 277 processes refused one to an unelevated query. Helm makes
   * no elevation assumption anywhere, so a process it may not open is a process
   * reported as unknown - never one that fails the pass.
   */
  commandLine: string | null
  /** How far below the session's own process this sits. The root is 0. */
  depth: number
  /**
   * The ports this process is listening on, ascending - or null where the
   * socket query could not run at all.
   *
   * Nullable on its own rather than folded into the tree's own nullability,
   * because the two queries fail independently: a tree read with no socket
   * answer must say "not known" here and not the empty list, which would be a
   * claim that this process is listening on nothing.
   */
  ports: number[] | null
}

/** A listening socket held somewhere in a session's process tree. */
export interface SessionPort {
  port: number
  pid: number
  /** The image name of the process holding it. */
  process: string
  /**
   * Every local address it is bound to, sorted - `127.0.0.1`, `0.0.0.0`, `::`.
   *
   * A list rather than one string because a server routinely binds the same
   * port twice, once per address family, and "which interfaces is this reachable
   * on" is a different question from "which port". Collapsing them would lose
   * the difference between a loopback-only dev server and one on every
   * interface, which is the difference that matters when two machines share a
   * network.
   */
  addresses: string[]
}

/**
 * What one session Helm hosts is currently holding.
 *
 * Every field that can be unknown is nullable, and the rule the whole shape is
 * built on is that **"could not look" and "nothing there" are different
 * claims**. A pass that could not enumerate processes leaves `processes` null,
 * and the pane says so; a pass that enumerated and found the session childless
 * leaves it `[]`. A surface that merged them would tell somebody a session is
 * holding nothing at the exact moment Helm had failed to ask.
 */
export interface SessionResources {
  /** Helm's own session row id. */
  id: number
  /**
   * The pty's pid: the root of the tree, and provably Helm's own child.
   *
   * Deliberately the pty's rather than the registry record's. Through a `.cmd`
   * shim those differ - the pty is `cmd.exe` and `claude.exe` is beneath it -
   * and the pty's is the one that roots *everything the session started*,
   * including the shim itself. It is also the one Helm holds without asking
   * anybody.
   */
  rootPid: number
  /**
   * The tree under `rootPid`, root first and then breadth-first, or null where
   * the host could not be asked at all.
   */
  processes: SessionProcess[] | null
  /**
   * Whether `rootPid` was in the enumeration.
   *
   * Distinguishes the third state from the other two: the pass ran, and the
   * session's own process was not in it. That is a session that exited between
   * the pass being scheduled and it running, which is ordinary - and it is not
   * the same claim as "it has no children".
   */
  rootSeen: boolean
  /** Listening sockets anywhere in the tree, or null where none was asked for. */
  ports: SessionPort[] | null
  /**
   * How many processes in the tree would not give up a command line.
   *
   * Reported rather than hidden: it is the size of what this pass could not
   * see, and a pane that showed twelve rows without saying four of them are
   * opaque would be implying a completeness it does not have.
   */
  opaque: number
  /** When the pass that produced this ran, epoch ms. */
  atMs: number
}

/**
 * What to call a session on screen: its label if it has one, its `-n` name if
 * not.
 *
 * One function rather than `label ?? name` at each call site, and that is the
 * whole reason it exists. A session is named in four places - the tab, the tab's
 * hover hint, the sidebar's live-session tooltip, and the confirmation before it
 * is ended - and the failure a shared helper prevents is the one where the tab
 * says "PR review", the dialog asking to end it says "dev 2", and the user has
 * to work out that those are the same session before answering a question whose
 * cost is whatever the session had not finished saying.
 *
 * In `types.ts` because the renderer imports it, and a value import into the
 * browser bundle comes from `@helm/core/types` and not the package root
 * (CLAUDE.md "Boundaries").
 */
export function sessionLabel(session: Pick<SessionRecord, 'name' | 'label'>): string {
  return session.label ?? session.name
}

/**
 * One prompt a person submitted, as `~/.claude/history.jsonl` recorded it.
 *
 * This file is Claude Code's own, shared by every session on the machine and
 * appended to whether or not Helm is running. Helm reads it and never writes
 * to it.
 */
export interface HistoryPrompt {
  sessionId: string
  /** Submission order across the whole file. Monotonic, not necessarily dense. */
  seq: number
  /** The prompt as typed, verbatim. */
  text: string
  /** Epoch milliseconds, as recorded. */
  at: number
}

/**
 * A session `history.jsonl` knows about, and whether it can still be resumed.
 *
 * The two facts come from different places and only one of them is durable.
 * The prompts persist indefinitely; the transcript that `--resume` actually
 * needs is reaped on Claude Code's own schedule - 105 of 799 survive on the
 * machine this was built against - so resumability is a property of the disk
 * right now, re-read on every index pass rather than remembered.
 */
export interface HistorySession {
  sessionId: string
  /** Working directory the session ran in, exactly as history recorded it. */
  project: string
  /** Last path segment of `project`, for a list that has no room for the rest. */
  projectName: string
  promptCount: number
  firstAt: number
  lastAt: number
  /** The opening prompt, verbatim. What was said first, not what this is called. */
  firstPrompt: string
  /**
   * What to call this session, derived from its prompts.
   *
   * Never empty, and not the same thing as `firstPrompt`: the opening prompt is
   * a slash command in 291 of this machine's 1,011 sessions and an image
   * placeholder in others, which is a row that says nothing. See
   * `discovery/title.ts` for what is read instead, and `historyTitle` for the
   * name a surface should actually paint.
   */
  title: string
  /**
   * True when no prompt survived cleaning and `title` is Helm's own words for
   * an empty record - "Image only", "No prompt recorded". A stand-in is not
   * something to draw in the same weight as a sentence somebody wrote.
   */
  titleFallback: boolean
  /**
   * The name somebody gave this session by hand, or null.
   *
   * Kept apart from `title` for the reason `SessionRecord` keeps `label` apart
   * from `name`: clearing it has to return the row to its derived title, and a
   * single field that had been overwritten could not.
   */
  label: string | null
  /**
   * The title the conversation's agent last gave its session
   * (`session_title`), or absent when no agent titled it. A name given by
   * hand wins over it, and it wins over `title`, which is only derived.
   */
  agentTitle?: string
  /**
   * The agent its newest session in Floti was started as, which Resume
   * composes again from its file as it is then; absent for none.
   */
  agent?: string
  /**
   * The permission mode its newest session ran with, which a resume as its
   * agent keeps; absent for none.
   */
  permissionMode?: PermissionMode
  /** The transcript on disk, or null once it has been reaped. */
  transcriptFile: string | null
  transcriptBytes: number | null
  /** False when the recorded working directory is no longer there. */
  projectExists: boolean
  /**
   * What Helm's own archive holds for this session, or null when it has never
   * held anything.
   *
   * The third fact about a session, alongside the transcript and the folder,
   * and it does not follow from either: a conversation Claude Code reaped can
   * still be readable here, and one that is still on disk may never have been
   * captured. `'evicted'` is deliberately not folded into null - see
   * `ArchiveSessionState`.
   */
  archive: ArchiveSessionState | null
  /** Messages in the archive for it. Zero once the ceiling has dropped them. */
  archivedMessages: number | null
  /**
   * The first prompt that matched the search, when the query had one. Absent
   * for an unfiltered listing rather than set to the opening prompt, so the UI
   * can tell "matched here" from "this is just the start of it".
   */
  match?: string | undefined
}

/**
 * The cap on a hand-given session name.
 *
 * Long enough for a sentence about what a session was, short enough that the
 * list stays a list. `sanitizeSessionName`'s 60 is a tab's width; this is a
 * row's, and the row is wider. Here rather than beside the write because the
 * field enforcing it is in the renderer and the write enforcing it is in main -
 * one number, imported by both, or the field lets you type what the write will
 * silently cut.
 */
export const HISTORY_NAME_MAX = 120

/**
 * What to call a history session on screen: the name it was given by hand,
 * else the title its agent gave it, else the one derived from its prompts.
 *
 * `sessionLabel`'s twin, and it exists for the same reason - the list row, the
 * detail heading and the tab a resume opens all name the same session, and a
 * `label ?? agentTitle ?? title` written out three times is three places for
 * them to disagree. In `types.ts` because the renderer imports it (CLAUDE.md
 * "Boundaries").
 */
export function historyTitle(session: Pick<HistorySession, 'title' | 'label' | 'agentTitle'>): string {
  return session.label ?? session.agentTitle ?? session.title
}

// ---------------------------------------------------------------------------
// Transcript archive
// ---------------------------------------------------------------------------

/**
 * What Helm's archive holds for a session.
 *
 * Two values and no third for "never captured", which is the absence of a row.
 * The distinction that has to survive every refactor is `'evicted'` against
 * null: "we had this conversation and dropped it to stay under your limit" and
 * "this was reaped before Helm ever saw it" are different facts about the same
 * missing text, and only one of them is something the user chose.
 */
export type ArchiveSessionState = 'archived' | 'evicted'

/** One message of an archived conversation, as the viewer renders it. */
export interface ArchiveMessage {
  uuid: string
  role: 'user' | 'assistant'
  /** Epoch ms. */
  at: number
  text: string
}

/**
 * One archived conversation.
 *
 * `messages` is empty for an evicted one, and the row is still returned: the
 * pane has something to say about a conversation Helm dropped, and nothing to
 * say about one it never had.
 */
export interface ArchivedConversation {
  sessionId: string
  /** The transcript it was read from. Usually gone by the time this is read. */
  sourceFile: string
  state: ArchiveSessionState
  firstAt: number | null
  lastAt: number | null
  messageCount: number
  /** Message text as read, before compression. */
  rawBytes: number
  /** What it costs in the database now. Zero once evicted. */
  storedBytes: number
  capturedAt: string
  evictedAt: string | null
  messages: ArchiveMessage[]
}

/** What the archive holds, for the settings pane to state rather than imply. */
export interface ArchiveStats {
  sessions: number
  /** Sessions the ceiling dropped. Counted separately; they are not gone-gone. */
  evictedSessions: number
  messages: number
  rawBytes: number
  /** Compressed message bodies. The figure the ceiling is enforced against. */
  storedBytes: number
  /** The ceiling in force, from `transcriptArchiveMaxBytes`. */
  maxBytes: number
  /** Last-message time of the oldest and newest archived conversation. */
  oldestAt: number | null
  newestAt: number | null
}

/**
 * Resuming needs both halves: the conversation to restore and the directory to
 * restore it in. `--resume <id>` is resolved against the working directory - a
 * session resumed from anywhere else reports "No conversation found", measured
 * on 2.1.225 - so a project that has been deleted is as fatal as a reaped
 * transcript, and the launcher says which one it is.
 */
export function canResume(session: HistorySession): boolean {
  return session.transcriptFile !== null && session.projectExists
}

/** One recorded working directory, with how much history it holds. */
export interface HistoryProject {
  project: string
  name: string
  sessions: number
  prompts: number
  lastAt: number
  resumable: number
  exists: boolean
}

/** What the index currently holds. Cheap enough to recompute on every change. */
export interface HistorySummary {
  sessions: number
  prompts: number
  projects: number
  /** Sessions that could be resumed right now. */
  resumable: number
  /** Newest prompt in the index, or null when it is empty. */
  latestAt: number | null
  /** The file being indexed, so the UI can say where this came from. */
  historyFile: string
  /** Bytes of it consumed so far; the cursor an incremental pass resumes at. */
  indexedBytes: number
  /** Set when the file could not be read at all. */
  error?: string | undefined
}

/**
 * What a search is over.
 *
 * `prompts` is the historic behaviour and the default: a substring of a prompt
 * or a project path, matched with `LIKE`. `messages` is the archive - every
 * word of every conversation Helm captured, through FTS5.
 *
 * Two scopes rather than one box that searches both, and that is a decision.
 * They answer different questions and return wildly different counts, and the
 * counts are the point: "sessions where I typed this" and "conversations where
 * this was said" are not the same list, and a box that quietly returned the
 * union would make the smaller of the two unreachable.
 */
export const HISTORY_SEARCH_SCOPES = ['prompts', 'messages'] as const
export type HistorySearchScope = (typeof HISTORY_SEARCH_SCOPES)[number]

export interface HistoryQuery {
  /** Case-insensitive substring of a prompt. Empty means no filter. */
  search?: string | undefined
  /** What `search` is matched against. Defaults to `prompts`. */
  scope?: HistorySearchScope | undefined
  /** One recorded working directory, compared case-insensitively. */
  project?: string | undefined
  /** Drop sessions that could not be resumed. */
  resumableOnly?: boolean | undefined
  limit?: number | undefined
}

export interface HistoryPage {
  /** Most recently active first. */
  sessions: HistorySession[]
  /** Sessions the query matched, before `limit` was applied. */
  total: number
  /** How long the query itself took, in milliseconds. */
  tookMs: number
}

/**
 * The launch knobs a profile carries, named as the CLI names them.
 *
 * Both lists are the CLI's own, copied rather than derived: they are the
 * choices `claude --help` prints for `--effort` and `--permission-mode` on the
 * pinned version, and a value outside them is rejected by the CLI after the
 * session has already been spawned. Keeping them here lets a profile be
 * validated at the point it is saved instead.
 */
export const EFFORT_LEVELS = ['low', 'medium', 'high', 'xhigh', 'max'] as const
export type EffortLevel = (typeof EFFORT_LEVELS)[number]

export const PERMISSION_MODES = [
  'acceptEdits',
  'auto',
  'bypassPermissions',
  'manual',
  'dontAsk',
  'plan'
] as const
export type PermissionMode = (typeof PERMISSION_MODES)[number]

/**
 * One of a host's agents, as a launcher offers it: a person picks it to start
 * a session as that agent. Its fields are what fill the launcher's pickers.
 */
export interface LaunchAgent {
  name: string
  description: string
  /** Its file, such as `lead.md`; null for the default the host makes. */
  file: string | null
  /** The installed backends that can run it, in its order. Empty: it cannot run here. */
  backends: string[]
  /** The backends its file lists, in order; empty for any installed one. */
  listedBackends: string[]
  /** The model each backend runs it with, by backend id. */
  models: Record<string, string>
  /** The effort each backend runs it with, by backend id. */
  efforts: Record<string, EffortLevel>
  permission: PermissionMode | null
  /** The profile it names, by name. */
  profile: string | null
  /** Its file's hash, as `agents:read` gives it; null where there is no file. */
  hash: string | null
  allowedTools: string[]
  disallowedTools: string[]
  worktree: boolean
  canStart: boolean
  /** Why none of the host's backends can run it, as the host's sentence; null when one can. */
  cannotRun: string | null
}

/** An agent file a host could not load, with every problem joined. */
export interface BadAgent {
  name: string
  file: string
  /** Every problem, joined into one sentence fragment. */
  problem: string
  /** Each problem with the field and line it concerns. */
  problems: AgentProblem[]
  /** The file's hash; null when it could not be read. */
  hash: string | null
}

/** One problem the host has with an agent file (`AgentProblem` in floti-protocol). */
export interface AgentProblem {
  /** The field it concerns, as the file spells it; null for the whole file. */
  field: string | null
  /** The file's line, from 1; null when it concerns no one line. */
  line: number | null
  message: string
  /** The backend whose entry of a field keyed by backend (`model`, `effort`) it concerns; absent otherwise. */
  backend?: string
}

/** A shipped agent the host's folder lacks, which Add writes. */
export interface ShippedAgentOffer {
  name: string
  description: string
}

/** One agent's file as the host reads it (`AgentFile` in floti-protocol). */
export interface AgentFileText {
  name: string
  /** The file, such as `reviewer.md`. */
  file: string
  text: string
  /** The SHA-256 of the file's bytes: what a write or a delete names. */
  hash: string
  /** Every problem with it; empty when it is an agent. */
  problems: AgentProblem[]
}

/** What a write, a delete, a restore or an add came to (`AgentChange` in floti-protocol). */
export interface AgentChangeResult {
  changed: boolean
  /** The file as it is now; null when there is none. */
  file: AgentFileText | null
  /** Why nothing changed, as a sentence; null when the change was made. */
  refusal: string | null
  /** The parser's problems, when they are why it was refused. */
  problems: AgentProblem[]
  /** The file on disk, when it is not what the change was based on. */
  conflict: { onDiskHash: string | null; onDiskText: string | null } | null
}

/** One agent backend a host has installed: an id to send and a name to show. */
export interface InstalledBackend {
  id: string
  name: string
}

/** What a launcher reads of a host's agents when it opens. */
export interface LaunchAgents {
  /** The agents a person may start a session as: the host's default is not one. */
  agents: LaunchAgent[]
  bad: BadAgent[]
  /** The host's agent backends, in its order: a plain command line is not one. */
  backends: InstalledBackend[]
  /** The agents folder, as the host spells it. */
  folder: string
  /** Why this host changes nothing in its agents folder; null when it may. */
  readOnly: string | null
  /** The shipped agents the folder lacks, by name. */
  shipped: ShippedAgentOffer[]
}

/**
 * A saved launch composition - the core object everything in Helm is organised
 * around (SPEC 3).
 *
 * `overlays` are composed into the session as plugins, so their skills, agents
 * and commands resolve from a cwd that is not theirs. `access` is the separate
 * question of which directories the session may touch. They overlap in practice
 * and are still not the same thing: composing a repo's skills does not grant
 * its files, and granting its files does not compose its skills.
 */
export interface Profile {
  id: number
  name: string
  /** Working directory. Claude Code resolves `.claude/` config from it. */
  root: string
  /** Project paths composed via `--plugin-dir`. */
  overlays: string[]
  /** Project paths passed to `--add-dir`. */
  access: string[]
  model: string | null
  effort: EffortLevel | null
  permissionMode: PermissionMode | null
  agent: string | null
  /**
   * MCP server names. Persisted and exported, but not yet placed on the argv:
   * no CLI flag selects already-configured servers by name, and the agent's own
   * commands (`/mcp` for Claude Code) manage what is configured.
   */
  mcp: string[]
  /** Submitted as the session's first message. */
  openingPrompt: string | null
  /** Launcher ordering; null means unpinned. */
  pinnedOrder: number | null
  /**
   * `false` turns the session-agents tools off for sessions launched from this
   * profile. Null or absent follows the setting; a profile cannot turn them on
   * where the setting has them off.
   */
  sessionAgents?: boolean | null
  /**
   * `true` runs the profile's sessions with Remote Control under their own
   * names, and gives their children Remote Control too. Null or absent is off.
   */
  remoteControl?: boolean | null
  createdAt: string
  updatedAt: string
}

/** A profile before the store has given it an identity. */
export type ProfileDraft = Omit<Profile, 'id' | 'createdAt' | 'updatedAt'>

/**
 * How an overlay shim carries a source folder: a junction on Windows, which
 * needs no elevation; a directory symlink on macOS and Linux; `copy` when the
 * filesystem under the shim can make neither. Read from the stamp of a shim an
 * earlier version of Helm built.
 */
export type OverlayLinkMode = 'junction' | 'symlink' | 'copy'

export type ThemePreference = 'system' | 'light' | 'dark'

/** The three, as a value, so a validator and a control can share one list. */
export const THEME_PREFERENCES: readonly ThemePreference[] = ['system', 'light', 'dark']

/**
 * The canvas showing between islands, in px. Two is the least that still reads
 * as a gap between two hairlines rather than one thick one; past twelve the
 * panes stop reading as one window.
 */
export const PANE_GAP = { min: 2, max: 12, default: 6 } as const

/**
 * Panel corners, in px. Controls and popups are always one more (DESIGN.md
 * "Shape"), so this is the one number that moves every corner in the app.
 */
export const CORNER_RADIUS = { min: 0, max: 8, default: 3 } as const

/** How tightly lists and strips are packed. Text size never changes with it. */
export const DENSITY_MODES = ['comfortable', 'compact'] as const
export type Density = (typeof DENSITY_MODES)[number]

/** xterm's three cursor shapes, restated here so a validator and a control can
 * share one list without either of them importing xterm. */
export const TERMINAL_CURSOR_STYLES = ['block', 'underline', 'bar'] as const
export type TerminalCursorStyle = (typeof TERMINAL_CURSOR_STYLES)[number]

/**
 * The bounds on the two numeric terminal settings.
 *
 * The font size ceiling is not taste. A pane is a few hundred pixels wide, and
 * a grid narrow enough stops being a terminal: Claude Code's composer and
 * status line assume they have columns to lay out in, and every check that
 * asserts a pane came back with a usable grid is only loose because this is
 * tight. The floor is the point below which the glyphs stop being legible on a
 * 100% display.
 */
export const TERMINAL_FONT_SIZE = { min: 8, max: 32, default: 14 } as const

/** Lines of history a terminal keeps. The ceiling is memory: a line is roughly
 * a kilobyte of cell data, so a million of them per pane is not a setting. */
export const TERMINAL_SCROLLBACK = { min: 500, max: 200_000, default: 10_000 } as const

/**
 * How much of the project page's column the shell may take, as a percentage.
 *
 * The default is where the shell used to be pinned, and the argument that put
 * it there is still the argument for the default: about a third of the page
 * gives a tall display 15+ rows - PSReadLine's ListView threshold - while a
 * small window keeps most of its height for the project pane. What that
 * argument never justified was being the *only* value, which is what a fixed
 * class made it.
 *
 * The ceiling is half, and it is the user's own ask: past half the project
 * pane would be the smaller part of the page it names. The floor here is
 * proportional and is deliberately not the whole floor - the pane carries a
 * pixel floor too (`PROJECT_SHELL_MIN_PX`), which is the binding one on any
 * window this app opens. This bound is what stops a percentage chosen on a
 * short window describing a four-row terminal on a tall one.
 */
export const PROJECT_SHELL_HEIGHT_PCT = { min: 10, max: 50, default: 30 } as const

/**
 * The limits on a family of agents, each a whole number inside the range the
 * host accepts: the host refuses a limit outside it, and a write it refused
 * would drop every other setting sent with it, so Helm keeps to the same range.
 * The defaults are the design's: four children running at once per parent,
 * eight agent-started sessions per host, two levels deep and sixteen starts in
 * a parent's life.
 */
export const AGENT_CHILDREN_PER_PARENT = { min: 1, max: 32, default: 4 } as const
export const AGENT_SESSIONS_PER_HOST = { min: 1, max: 128, default: 8 } as const
export const AGENT_DEPTH = { min: 1, max: 8, default: 2 } as const
export const AGENT_STARTS_PER_PARENT = { min: 1, max: 1000, default: 16 } as const
/**
 * Command children one session may have running at once, counted apart from
 * every agent limit above: a test run never takes a reviewer's slot.
 */
export const AGENT_COMMANDS_PER_PARENT = { min: 1, max: 32, default: 4 } as const

/**
 * The rail's destinations that can be hidden, in the order the rail draws them.
 *
 * Settings is not one, and leaving it out of this list is the whole mechanism:
 * `railHidden` is validated against it, so no write can hide the one way back
 * to un-hiding everything else. The ids are the rail's own (`data-rail`).
 */
export const RAIL_DESTINATIONS = ['sessions', 'profiles', 'agents', 'history', 'files', 'browser'] as const

export type RailDestination = (typeof RAIL_DESTINATIONS)[number]

/** Anything `railHidden` may name: one of Helm's destinations, or a plugin's rail icon. */
export type RailItemId = RailDestination | PluginRailId

/**
 * How much of `helm.db` the transcript archive may take, in bytes.
 *
 * A gigabyte by default, and both halves of that are deliberate. Unbounded is
 * not an option: `helm.db` is the user's file, and a feature that grows it
 * without limit is one they find out about from their disk rather than from
 * Helm. A gigabyte is enormous for what this actually stores - the conversation
 * text out of one 3.9 MB transcript on this machine is 54 KB before
 * compression, a 71x difference, because tool traffic is not archived - so on
 * any ordinary machine the ceiling is a guard rail rather than a budget.
 *
 * The floor is a kilobyte rather than something respectable, because a bound
 * that nothing can drive past is a bound nothing has ever tested: the eviction
 * rule is the interesting part of this feature, and a ceiling smaller than what
 * was just archived is what makes it fire. The settings pane offers sensible
 * sizes; the validator only enforces the shape.
 */
export const TRANSCRIPT_ARCHIVE_BYTES = {
  min: 1024,
  max: 64 * 1024 ** 3,
  default: 1024 ** 3
} as const

/**
 * A shell Helm found on this machine, offered in the shell pickers.
 *
 * `path` is what gets launched and what gets stored, and it is absolute for the
 * same reason `claudePath` is: a bare `pwsh.exe` means whatever the process's
 * PATH happened to resolve it to, which is not necessarily what the user saw in
 * the picker.
 */
export interface DetectedShell {
  /** Absolute path to the executable. */
  path: string
  /** The file name - `pwsh.exe`. What the picker shows as machine data. */
  name: string
  /** A human label - "PowerShell 7". */
  label: string
  /** The arguments Helm would launch it with, from the per-shell table. */
  args: string[]
}

/**
 * One pane on the workspace tab strip, in the form it is written down in.
 *
 * A record and not the strip's `project:C:\...` id string, deliberately: a
 * Windows path can contain a `#` and a `:`, so a tab id is an identity to
 * compare and never a thing to take apart again. Restoring the strip needs the
 * path and the number back, so what is persisted is the fields rather than the
 * id built from them.
 *
 * The renderer's `PaneRef` is this type - one definition, because the shape a
 * pane has and the shape it is stored in must not be free to drift apart.
 *
 * `host` names the host a project, file or plugin tab belongs to, by its id in
 * the `hosts` setting, and is absent for this PC's own (`LOCAL_HOST_KEY`). So a
 * layout written before there were other hosts reads as this PC's, and this
 * PC's tabs keep the ids they always had (`paneId`). The layout is this
 * device's own and never reaches a host.
 */
export type WorkspaceTab =
  | { kind: 'project'; path: string; host?: string }
  | { kind: 'history' }
  | { kind: 'sessions' }
  | { kind: 'settings' }
  /**
   * A file, read beside the session changing it. `root` is the project it was
   * opened from - what the view is scoped to, what its path is shown relative
   * to, and the folder Helm checks it is still inside before reading a byte.
   */
  | { kind: 'file'; root: string; path: string; host?: string }
  /**
   * One of the `tabs` a plugin's manifest declares, with what it was opened
   * with. Kept across a restart; a plugin that is gone or disabled by then
   * takes its tabs with it. `title` is the one the page or the opener gave it,
   * or null for the manifest's.
   */
  | { kind: 'plugin'; plugin: string; tab: string; params: PluginParams; title: string | null; host?: string }
  /**
   * The one tab of a plugin that declares `pageStrip`: its tabs, opened as
   * pages in a strip of their own, in order, and the one in front by its
   * `pluginPageId`. Kept across a restart as a plugin tab is.
   */
  | { kind: 'plugin-pages'; plugin: string; pages: PluginPage[]; active: string | null; host?: string }
  /**
   * One of a host's agents, opened as a page from the Agents view, by its
   * name in that host's agents folder. Kept across a restart; an agent that
   * is gone by then opens saying so.
   */
  | { kind: 'agent'; name: string; host?: string }

/** How many tabs are worth writing down. Past this the list is a bug, not a
 * workspace, and a settings row nobody can shrink is worse than a truncation. */
export const WORKSPACE_TABS_MAX = 100

/**
 * How many projects the pinned list may name.
 *
 * A ceiling on a value that is JSON in one row and is rewritten whole on every
 * toggle - not a statement about how many
 * anybody pins. A list past a screenful has stopped being a shortlist and the
 * tree is what it wanted, but that is the user's call and not a validator's.
 */
export const PINNED_PROJECTS_MAX = 200

/**
 * How two project paths are compared, everywhere this list is read or written.
 *
 * Case folding and nothing else. The sidebar's own grouping already keys its
 * harness map with a bare `path.toLowerCase()`, and a second normalisation here
 * - trailing separators, `resolve`, short-name expansion - would be a way for
 * the pinned section and the group it lifts a project out of to disagree about
 * whether they are looking at the same project, which is how the same row ends
 * up printed twice.
 */
function samePath(a: string, b: string): boolean {
  return a.toLowerCase() === b.toLowerCase()
}

/** Whether this project has been lifted into the sidebar's Pinned section. */
export function isProjectPinned(pinned: readonly string[], path: string): boolean {
  return pinned.some((entry) => samePath(entry, path))
}

/**
 * Whether `path` is itself one of the scanned folders - the whole of what
 * "remove this folder from Helm" can act on. A project found *inside* a root
 * is not one: removing it would mean removing its parent.
 */
export function isScanRoot(roots: readonly string[], path: string): boolean {
  return roots.some((root) => samePath(root, path))
}

/**
 * The live sessions whose working directory is this folder.
 *
 * The whole of the launch-time warning's arithmetic, and it lives here beside
 * `samePath` for the reason that function's comment gives: one normalisation.
 * A warning that compared paths differently from the way the sidebar groups
 * them would be a warning that appears for one surface and not for the other,
 * on the same two directories.
 *
 * Case folding and nothing else, for the same reason. Trailing separators,
 * `resolve` and short-name expansion are all ways for two readings of one path
 * to disagree, and this comparison is between a path Helm holds and a path
 * Claude Code wrote into its own record - two writers, so the risk of a second
 * normalisation is a warning that silently stops appearing.
 *
 * `null` cwds are excluded rather than matched: a record with no working
 * directory is a session whose directory is unknown, and "unknown" is not
 * "here".
 */
export function liveSessionsIn(
  sessions: readonly LiveSession[],
  path: string
): LiveSession[] {
  return sessions.filter((session) => session.cwd !== null && samePath(session.cwd, path))
}

/**
 * The pinned list with one project switched on or off.
 *
 * The whole list every time, because that is how the setting is written, and
 * the comparison is the case-insensitive one above - so pinning `C:\Repos\Api`
 * when the list already holds `c:\repos\api` replaces that entry rather than
 * adding a second spelling of one project. Sorted by path so the stored value
 * does not depend on the order somebody clicked; what the sidebar *shows* is
 * sorted by name instead, which is a different question and answered there.
 */
export function withProjectPinned(
  pinned: readonly string[],
  path: string,
  on: boolean
): string[] {
  const without = pinned.filter((entry) => !samePath(entry, path))
  const next = on ? [...without, path] : without
  return next.sort((a, b) => a.toLowerCase().localeCompare(b.toLowerCase()))
}

/**
 * Persisted application settings. Keys are the column names in `app_settings`;
 * every value is JSON-encoded on the way in, so adding a key here is the only
 * step needed to persist it.
 */
export interface AppSettings {
  /**
   * Which slot is on screen: `dark` and `light` always, `system` whichever one
   * Windows is in. The name predates named themes and is kept because the row
   * it reads is every existing install's choice.
   */
  theme: ThemePreference
  /** The theme the dark slot shows - a built-in id or a user theme's. */
  themeDark: string
  /** The theme the light slot shows. */
  themeLight: string
  /** Px of canvas between islands. Bounded by `PANE_GAP`. */
  paneGap: number
  /** Px of panel corner. Bounded by `CORNER_RADIUS`. */
  cornerRadius: number
  density: Density
  /** `#rrggbb` fitted to the theme by `deriveAccent`, or null for the theme's own. */
  accentColor: string | null
  /** Directories the launcher scans. Empty means "not set up yet". */
  scanRoots: string[]
  /**
   * Projects lifted out of their harness group into the sidebar's Pinned
   * section, as absolute paths.
   *
   * Keyed by **path**, and that is a decision rather than the only option that
   * occurred to anybody - see the validator in `store/settings.ts`, which is
   * where the consequence is written down. Matching is case-insensitive, the
   * same comparison the sidebar's own grouping already makes on these paths.
   *
   * Projects only. A harness is not pinnable: the tree already gives every
   * harness a collapse state, which is most of what pinning one would be, and
   * one pin kind means there is no rule to invent for a pinned project inside a
   * pinned harness.
   *
   * A path that no longer resolves keeps its entry. Discovery will not return
   * it, so the sidebar paints it as gone rather than dropping it - pinning is a
   * deliberate act, and an unplugged drive is not a decision to un-pin.
   * Bounded by `PINNED_PROJECTS_MAX`.
   */
  pinnedProjects: string[]
  /** Window geometry, restored on next launch. */
  windowBounds: { width: number; height: number; x?: number; y?: number } | null
  /**
   * The panes, restored on next launch: how they were split and how big each
   * was, which tabs are open in which group, in the order they were arranged,
   * which one was in front of each, and which group had the focus
   * (`layout/panes.ts`).
   *
   * State rather than a preference, so it sits beside `windowBounds` and not in
   * the settings pane - it is something Helm remembers, not something anyone
   * chose. Null means nothing has been written yet, which is not the same as an
   * empty layout: a user who closed every tab gets an empty window back.
   *
   * **Sessions are written down and never reopened from here.** `before-quit`
   * ends every session, so on an ordinary start a saved session names nothing
   * and is dropped before it is drawn; after a crash, this is where the restore
   * offer finds the pane each one was in (`SavedPane`, `placeRestored`).
   * Browser tabs are not written down at all. A group that held only sessions
   * is restored as no group at all.
   *
   * It replaced `workspaceTabs`, the single strip the window had before it had
   * groups. That row is now an unknown key, ignored on read, so the first
   * launch after the change opens with an empty window once.
   *
   * It has been a tree since the panes could be split any way; before that it
   * was one or two groups side by side, sized by a `paneSplitPct` setting of
   * its own. `readSettings` reads that shape as the row it was
   * (`upgradeSavedLayout`), and the old row is otherwise ignored.
   *
   * `activeId` is a tab id, which is only ever compared: a saved id that no
   * longer matches an open tab falls back to that group's last tab.
   */
  paneLayout: SavedPaneLayout | null
  /**
   * When the first-run flow was finished. Null means it has not been, which is
   * what puts the setup pane on screen instead of the launcher.
   */
  firstRunCompletedAt: string | null
  /**
   * A `claude` executable the user picked by hand, for the machine where it is
   * not on PATH and not in the usual install directory. Null means "find it".
   *
   * A path, never a credential: Helm locates the CLI and hands it a pty, and
   * signing in stays entirely between the user and `claude`.
   */
  claudePath: string | null
  /**
   * What the status bar's usage segment shows. Set in the settings pane's
   * Appearance group; clicking the segment itself still cycles it, because a
   * quick accessor beside the thing it changes is worth keeping.
   */
  usageDisplay: UsageDisplayMode

  /**
   * A font family for the terminal panes, or null for the built-in stack.
   *
   * Whatever is named here is **prepended** to the default stack, never
   * substituted for it. A monospace font chosen for its letterforms is rarely
   * chosen for its box-drawing, CJK or emoji coverage, and Claude Code's TUI is
   * made of box-drawing characters - so a font with holes in it has to degrade
   * one glyph at a time rather than take the whole interface down with it.
   */
  terminalFontFamily: string | null
  /** Point size for the terminal panes. Bounded by `TERMINAL_FONT_SIZE`. */
  terminalFontSize: number
  /** Cursor shape in the terminal panes. */
  terminalCursorStyle: TerminalCursorStyle
  terminalCursorBlink: boolean
  /** Lines of history a terminal pane keeps. Bounded by `TERMINAL_SCROLLBACK`. */
  terminalScrollback: number
  /**
   * The executable new project shells are opened with, or null to let Helm
   * detect one. An absolute path, for the reason `DetectedShell.path` gives.
   *
   * Project shells only. A Claude session is not a shell - Helm hands the
   * `claude` executable its own pty and this setting never reaches it.
   */
  terminalShell: string | null
  /**
   * How tall a project page's shell is, as a percentage of that page's column.
   * Bounded by `PROJECT_SHELL_HEIGHT_PCT`, dragged by the handle above the
   * shell, and settable in the Terminal group for when a drag lands somewhere
   * silly.
   *
   * **One value for every project rather than one per project.** The question
   * being answered is "how much terminal do I want", and that is about the
   * person and the monitor in front of them, not about the repository: someone
   * who wants a tall shell to read a `pnpm dev` in wants it in every checkout
   * they read one in. Per-project heights would also mean the page's
   * proportions moved as you moved between projects, which is furniture
   * rearranging itself.
   *
   * Not a terminal preference, whatever the settings group it is shown in says:
   * it never reaches `applyPrefs` and no session pane has one. It is the
   * project page's layout.
   */
  projectShellHeightPct: number

  /**
   * Whether long lines wrap in a file read from the Files view.
   *
   * **Default off**, following the config editor rather than the prose one: a
   * source file is structure, and a wrapped line hides it. A note is the
   * exception and is not governed by this - markdown in the editor always
   * wraps, because a paragraph is one very long line.
   *
   * The toggle in a file's status line writes this, so the choice holds for
   * the next file and the next start rather than resetting per tab.
   */
  filesWrap: boolean

  /**
   * Destinations taken off the rail, by a right-click on it.
   *
   * Only ids from `RAIL_DESTINATIONS` and plugins' rail ids, which is what
   * keeps Settings on the rail whatever this says. Hiding is not disabling:
   * Ctrl+P, Ctrl+N and every other way into a destination still work. A
   * plugin's id outlives the plugin, harmlessly: it names nothing until a
   * plugin with that id is back.
   */
  railHidden: RailItemId[]

  /**
   * How many bytes of `helm.db` the transcript archive may occupy.
   *
   * The archive itself has no on/off switch, and that is the decision rather
   * than an omission. 91% of the conversations behind `history.jsonl` were
   * already gone when this was measured, and a default-off setting would go on
   * losing them while it sat off - the cost of capturing is a few kilobytes per
   * conversation and the cost of not capturing is the conversation. What *is*
   * a setting is the ceiling, because that is the part with a real trade-off in
   * it: bounded by `TRANSCRIPT_ARCHIVE_BYTES`, enforced after every pass by
   * dropping the oldest archived session whole. See `evictToCeiling` - the
   * ceiling is adjustable and the eviction rule is not.
   */
  transcriptArchiveMaxBytes: number

  /**
   * Whether Helm asks GitHub, once per launch, if there is a newer release.
   *
   * A deliberate amendment to the network posture and the reason README, SPEC
   * 5, PACKAGING and the `update:check` comment all moved together: until this,
   * Helm's own process opened no connection at all unless somebody invoked the
   * channel by hand. It now opens one, at most once a day, on a launch.
   *
   * The reasoning that made "only when asked" right is untouched, because it
   * was never about the request - it was about the *download*. Helm still
   * fetches nothing, replaces nothing and restarts nothing; the whole outcome
   * is a version number, and a line in the status bar when it is higher. What
   * "only when asked" actually cost was the person who would have to think to
   * ask, which is nobody: an update you have to remember to look for is an
   * update you run without for months.
   *
   * On by default and off in one tick, because a machine that must not talk to
   * anything is a real requirement and not an exotic one.
   */
  updateCheck: boolean
  /**
   * When the last check actually reached GitHub, ISO 8601, or null.
   *
   * Internal state and deliberately not in the settings pane, the same as
   * `windowBounds` and `firstRunCompletedAt`: it is something Helm remembers,
   * not something anyone chose. It exists to throttle - `UPDATE_CHECK_EVERY_MS`
   * - so that restarting the app twenty times in an afternoon, which is what
   * developing it looks like, is still one request.
   */
  lastUpdateCheckAt: string | null

  /**
   * How far the browser pane may reach: anywhere, or this machine only.
   *
   * A **posture**, and the widest of the two reach controls: it governs the
   * pane itself, whoever is driving. It is enforced in exactly one function
   * (`browserReachAllows`), which `browserMcpLocalOnly` composes with rather
   * than copying - see `agentReach`.
   */
  browserReach: BrowserReach
  /**
   * What the address bar does with a phrase: hand it to this engine on Enter,
   * or (`off`) say it is not an address.
   *
   * Only the address bar searches - a link, a remembered address and every
   * agent tool pass URLs - and nothing is sent while typing, so the one request
   * a search makes is the one Enter asked for. The results page is held to
   * `browserReach` like any other, so `local` refuses it with a sentence.
   */
  browserSearch: BrowserSearch
  /**
   * Whether Helm serves its browser tools to the sessions it hosts.
   *
   * On by default, because the stated purpose is that Claude can open things in
   * Helm: a session that cannot reach the pane beside it is a pane the user has
   * to drive twice. Off is a real off - `main/browser-mcp.ts` never binds a
   * port, no token exists, and no `--mcp-config` reaches any argv - so the app
   * has no inbound listener at all, which is the state it was in before M17.
   */
  browserMcp: boolean
  /**
   * Whether the tools are confined to this machine even when the pane is not.
   *
   * The **narrower** of the two reach controls, and off by default because the
   * pane's default is `web` and a tool that could not follow the page the user
   * is looking at would be a tool nobody used. On, an agent navigation is
   * allowed only where `browserReach` **and** this both allow it - the
   * intersection, taken by passing both to `browserReachAllows` rather than by
   * writing a second rule (`agentReach`). An agent can therefore never exceed
   * the reach of the pane it is driving, in either setting's direction.
   */
  browserMcpLocalOnly: boolean
  /**
   * Reopen the sessions a crash took without asking first.
   *
   * Off by default: the offer is one screen at the next start, and a crash
   * that took a session somebody had finished with should not bring it back
   * unasked. On, every session the offer would list as reopenable is reopened
   * where it was, and anything that could not be is said once.
   */
  restoreWithoutAsking: boolean
  /**
   * The last URLs a browser pane visited, newest first.
   *
   * The address bar's dropdown and nothing more elaborate - no history page, no
   * manager, no search over it (see the milestone's "explicitly out"). State
   * rather than a preference, so it sits beside `paneLayout` and is
   * deliberately absent from the settings pane. Bounded by
   * `BROWSER_RECENT_URLS_MAX`.
   */
  browserRecentUrls: string[]
  /**
   * The last URL a browser pane was on, per project directory.
   *
   * Also state. It is what makes opening a browser beside a project cheap: the
   * dev server's port is a property of the project, and typing it again every
   * session is the papercut this pane exists to remove. Keyed by the project's
   * path, lower-cased, the same comparison every other path list here makes.
   * Bounded by `BROWSER_PROJECT_URLS_MAX`.
   *
   * The browser *tabs* themselves are not persisted, and that is the same rule
   * the session strip follows for the same reason: a `WebContentsView` is
   * main-owned state with a process behind it, it does not outlive the app, and
   * a strip of tabs pointing at views that no longer exist is not a workspace
   * restored. This is the part worth remembering, so this is the part remembered.
   */
  browserProjectUrls: Record<string, string>

  /**
   * Whether Helm tells a session it hosts what the other sessions are doing.
   *
   * **A second setting rather than `browserMcp`'s, and the reasoning is the
   * decision.** The two are served by one listener, on one port, under one
   * token - so a single tick would have been less to hold and would read as one
   * capability. They are not one capability. Driving a browser acts on the
   * world; this only looks at it, and the thing it looks at is *other people's
   * work*. Somebody may reasonably want either without the other, and the two
   * refusals have nothing in common: "do not let an agent click things" and "do
   * not tell an agent about my other sessions".
   *
   * The second reason is that it makes off assertable at the wire. With two
   * named servers, this off means the route answers 404, the name is absent
   * from the `--mcp-config` document, and the tools are in no list - three
   * independent facts. Folded into `browserMcp`, "nothing in argv" could not be
   * asserted at all, because the browser's `--mcp-config` is in the argv either
   * way.
   *
   * **On by default, and the argument is not that its neighbour is.** The two
   * differ in exactly the way that would decide this: `browserMcp` on gives an
   * agent *reach* over a pane the user is looking at, where this only tells an
   * agent about work in other windows. That is a smaller grant, over the user's
   * own machine, of facts the operating system will hand any process that asks
   * - and against it stands the failure it exists to prevent, which is two
   * agents editing one checkout, and which happens to somebody who never went
   * looking for a setting. A capability that has to be discovered is one that
   * prevents nothing.
   *
   * What makes on defensible is what is *not* served: no conversation, no argv,
   * no conversation id, no child command lines, and no way to send another
   * session anything. If any of that were in it, this would default off.
   *
   * Off is one tick away and is a real off - `main/session-tools.ts` is then
   * not mounted, no route exists, and the app is exactly what it was without it.
   */
  sessionMcp: boolean

  /**
   * Whether the sessions a host starts can start, list and stop children of
   * their own: the host's agent tools and `floti agent`. A profile can turn it
   * off for its own sessions (`Profile.sessionAgents`); this is the setting
   * every other session follows.
   *
   * Told to the host on every connect and every change, and the host serves the
   * tools - Helm's own process serves none. On by default, because the design
   * keeps the tools' reach to the caller's own children, bounds them with the
   * four limits below, and a capability that has to be found first is one most
   * sessions never get.
   */
  sessionAgents: boolean
  /** Children one session may have running at once. A start past it is refused. */
  agentChildrenPerParent: number
  /** Sessions agents started that may run on one host at once; people's own count toward none. */
  agentSessionsPerHost: number
  /** How deep a family goes: 2 is a parent, its children, and their children. */
  agentDepth: number
  /** Children one session may start in its life, running or not. */
  agentStartsPerParent: number
  /**
   * Command children (`floti agent run`) one session may have running at once.
   * Counted apart from the agent limits: a command child takes none of their
   * slots and an agent child takes none of this one.
   */
  agentCommandsPerParent: number
  /**
   * Whether a parent's tab shows its running children beside its terminal while
   * its pane has room, or the parent shows alone with its children in the tree
   * and in its crumb. Kept on this device with the rest of its layout and never
   * told to a host: which children a window shows is that window's own choice.
   * On by default, because watching children work is what starting them is for.
   */
  showChildrenInParentTab: boolean

  /**
   * What quitting Helm does to the sessions it started on a host, by host:
   * `end` them or `keep` them running there. This PC's host is keyed
   * `LOCAL_HOST_KEY` and ends them when it has no entry; a remote host is keyed
   * by its id in `hosts` and keeps them running when it has none
   * (`quitEndsSessions`).
   *
   * Kept on this device, never in the host: each client decides for itself
   * what its quitting does. The quit question's "Keep running and quit" applies to that one quit and
   * never writes here; only Settings does.
   */
  quitSessions: Readonly<Record<string, QuitSessions>>
  /**
   * The hosts on other computers this device reaches, in the order they were
   * added: each one's id, name and how it is reached (an ssh target exactly as
   * typed, or a command). Empty by default: this PC's own host is not in it.
   *
   * Kept on this device, never in a host, like `quitSessions`, which keys each
   * by its id. It holds nothing secret: no
   * ssh key, password or passphrase, and no ssh option (`hosts.ts`).
   */
  hosts: readonly RemoteHost[]
  /**
   * When Helm's saved profiles were copied into the local host, ISO 8601, or
   * null while they never have been.
   *
   * Internal state, like `lastUpdateCheckAt`, and not in the settings pane.
   * Profiles live in the host; the first time Helm connects to one it copies
   * the ones an earlier version kept in helm.db into it, once, and this is
   * what keeps a profile deleted in the host afterwards from being copied back
   * on the next start. Helm's own profiles are never changed by the copy.
   */
  profilesCopiedToHostAt: string | null
  /**
   * When Helm's plugin folders were copied into the local host, ISO 8601, or
   * null while they never have been.
   *
   * Internal state, like `profilesCopiedToHostAt`, and not in the settings
   * pane. Plugins are registered in the host; the first time Helm connects to
   * one it copies its folders, their on/off and tools switches and their
   * setting values into it, once (never a secret),
   * and this keeps a plugin removed in the host afterwards from coming back on
   * the next start. Helm's own plugin records are never changed by the copy.
   */
  pluginsCopiedToHostAt: string | null
}

/** What quitting Helm does to the sessions it started on a host. */
export type QuitSessions = 'end' | 'keep'

export const QUIT_SESSIONS: readonly QuitSessions[] = ['end', 'keep']

/**
 * What quitting Helm does to the sessions it started on `host`: what the
 * setting says, and otherwise End for this PC's own host and Keep them running
 * for a host on another computer, whose sessions are usually long work meant
 * to outlive the laptop that started them.
 */
export function quitChoice(settings: Pick<AppSettings, 'quitSessions'>, host: string = LOCAL_HOST_KEY): QuitSessions {
  return settings.quitSessions[host] ?? (host === LOCAL_HOST_KEY ? 'end' : 'keep')
}

/** Whether quitting Helm ends the sessions it started on `host` (`quitChoice`). */
export function quitEndsSessions(settings: Pick<AppSettings, 'quitSessions'>, host: string = LOCAL_HOST_KEY): boolean {
  return quitChoice(settings, host) === 'end'
}

/** How many addresses the dropdown offers. Ten, and no manager behind it. */
export const BROWSER_RECENT_URLS_MAX = 10

/**
 * How many projects keep a remembered URL.
 *
 * A ceiling on a value rewritten whole on every navigation, exactly as
 * `PINNED_PROJECTS_MAX` is - not a statement about how many projects anybody
 * has. Past this the oldest entries are dropped.
 */
export const BROWSER_PROJECT_URLS_MAX = 200

export const DEFAULT_SETTINGS: AppSettings = {
  theme: 'system',
  themeDark: 'nocturne',
  themeLight: 'daylight',
  paneGap: PANE_GAP.default,
  cornerRadius: CORNER_RADIUS.default,
  density: 'comfortable',
  accentColor: null,
  scanRoots: [],
  pinnedProjects: [],
  windowBounds: null,
  paneLayout: null,
  firstRunCompletedAt: null,
  claudePath: null,
  usageDisplay: 'percent',
  // The four defaults below are the values Spike C measured and `terminal.ts`
  // was built around. They are the documented baseline, not a starting point
  // someone picked: a setting left alone must produce the configuration the
  // fidelity checks measure.
  terminalFontFamily: null,
  terminalFontSize: TERMINAL_FONT_SIZE.default,
  terminalCursorStyle: 'block',
  terminalCursorBlink: true,
  terminalScrollback: TERMINAL_SCROLLBACK.default,
  terminalShell: null,
  projectShellHeightPct: PROJECT_SHELL_HEIGHT_PCT.default,
  // Off, following the config editor rather than the prose one - see the field.
  filesWrap: false,
  railHidden: [],
  transcriptArchiveMaxBytes: TRANSCRIPT_ARCHIVE_BYTES.default,
  updateCheck: true,
  lastUpdateCheckAt: null,
  // `web` rather than `local`, and it is a decision. The pane is framed as a
  // dev-server viewport, but a dev server that pulls an API, a font or a docs
  // page is the ordinary case, and a viewport that could not follow it is one
  // people would turn off on the first afternoon. `local` is one click away for
  // the run where nothing should leave the machine.
  browserReach: 'web',
  // Google, because a browser that refuses a phrase is the "not first class"
  // feeling this replaced, and it is the engine most people expect there.
  browserSearch: 'google',
  // On, because the whole point of the endpoint is that a session can open a
  // page in the app that is hosting it; off is one tick away and removes the
  // listener entirely.
  browserMcp: true,
  // Off, because the pane defaults to `web`: an agent confined to loopback
  // beside a pane that is not would be a surprise rather than a posture.
  browserMcpLocalOnly: false,
  restoreWithoutAsking: false,
  browserRecentUrls: [],
  browserProjectUrls: {},
  // On, because the collision this exists to prevent - two sessions in one
  // working tree - happens to somebody who never went looking for a setting,
  // and because what it serves is a strictly smaller grant than the tick above
  // it. See the field for what is not in it, which is what makes that true.
  sessionMcp: true,
  // On, with the limits that bound it: a family is a parent, up to four children
  // at once, at most eight agent-started sessions on the host, two levels deep,
  // and sixteen starts in a parent's life. Sessions a person starts count
  // toward none of them.
  sessionAgents: true,
  agentChildrenPerParent: AGENT_CHILDREN_PER_PARENT.default,
  agentSessionsPerHost: AGENT_SESSIONS_PER_HOST.default,
  agentDepth: AGENT_DEPTH.default,
  agentStartsPerParent: AGENT_STARTS_PER_PARENT.default,
  agentCommandsPerParent: AGENT_COMMANDS_PER_PARENT.default,
  showChildrenInParentTab: true,
  // Empty: this PC's host then ends the sessions Helm started when it quits,
  // which is what a session on your own computer usually wants, and a host on
  // another computer keeps them running (`quitChoice`).
  quitSessions: {},
  hosts: [],
  profilesCopiedToHostAt: null,
  pluginsCopiedToHostAt: null
}

/**
 * How long a launch waits before asking about releases again.
 *
 * A day. The question changes about as often as a release happens, and the
 * throttle is what keeps "once per launch" from meaning "once per restart" on
 * the machine Helm is being written on.
 */
export const UPDATE_CHECK_EVERY_MS = 24 * 60 * 60 * 1000

// ---------------------------------------------------------------------------
// An agent's config, as composition reads it, and the files a note's save writes
// ---------------------------------------------------------------------------

/**
 * One `.claude/` tree, and where it sits in the precedence chain.
 *
 * `user` is `~/.claude` itself; the other two are directories that *contain* a
 * `.claude`, which is why `path` is the base rather than the config directory -
 * `CLAUDE.md` and `.mcp.json` live beside `.claude/`, not inside it.
 */
export type ConfigScopeKind = 'user' | 'harness' | 'project'

export interface ConfigScope {
  kind: ConfigScopeKind
  /** The directory config resolves from. For `user`, the home directory. */
  path: string
  /** The `.claude` directory. Equal to `path` for the user scope. */
  claudeDir: string
  label: string
  /** Present so the switcher can say so rather than showing an empty tree. */
  exists: boolean
}

/**
 * What a file in a `.claude` tree is, as Claude Code resolves it.
 *
 * `skill` is the `SKILL.md` inside a skill directory, not the directory: the
 * directory name is the skill's name and the file is what gets edited.
 */
export type ConfigFileKind =
  | 'skill'
  | 'command'
  | 'agent'
  | 'workflow'
  | 'hook'
  | 'settings'
  | 'settings-local'
  | 'claude-md'
  | 'mcp'
  | 'rule'
  | 'other'

export interface ConfigFile {
  path: string
  /** Relative to the scope's base directory, with forward slashes. */
  relPath: string
  kind: ConfigFileKind
  /**
   * How it is addressed: a skill's directory name, a command's `spec:plan`
   * namespace path, or the file name for everything else.
   */
  name: string
  size: number
  mtimeMs: number
  /** `description:` from the frontmatter, when there is one. */
  description: string | null
  /** True for a file Helm will not offer to edit as text. */
  binary: boolean
}

export interface ConfigTree {
  scope: ConfigScope
  files: ConfigFile[]
  /** Directories that could not be read. Not fatal; the rest of the tree stands. */
  errors: string[]
  scannedAt: string
}

/** A file's bytes, with the hash every write is checked against. */
export interface ConfigFileContent {
  path: string
  exists: boolean
  content: string
  /** sha256 of the bytes on disk, hex. The editor's basis for its next write. */
  hash: string
  size: number
  mtimeMs: number
  /** Set when the bytes are not decodable text; the editor refuses these. */
  binary: boolean
}

/**
 * Why a snapshot was taken. Stored on the row and shown in the file's history.
 *
 * `rename` and `delete` are ordinary rows and are deliberately not a second
 * mechanism: a `delete` row holds the bytes that were there, so restoring it
 * puts the file back the same way restoring an `edit` row does. That is what
 * makes "undo this delete" and "restore this version" the same button.
 */
export type ConfigWriteReason =
  | 'edit'
  | 'create'
  | 'restore'
  | 'mcp'
  | 'approve'
  | 'rename'
  | 'delete'

export interface ConfigSnapshotMeta {
  id: number
  scopePath: string
  /** Relative to `scopePath`, forward-slashed - the same key the index uses. */
  filePath: string
  contentHash: string
  bytes: number
  reason: ConfigWriteReason
  createdAt: string
}

export interface WriteConfigRequest {
  /** The scope's base directory. Recorded on the snapshot. */
  scopePath: string
  path: string
  content: string
  /**
   * The hash the editor's content was derived from, or null for a file it knows
   * does not exist yet. A mismatch is an external edit and stops the write.
   */
  expectedHash: string | null
  reason: ConfigWriteReason
}

export interface WriteConfigResult {
  ok: boolean
  /** The file after the write, or as it stands now when the write was refused. */
  file: ConfigFileContent
  /** Null when nothing was written; otherwise the row taken first. */
  snapshotId: number | null
  /** The bytes were already what was asked for, so nothing was written. */
  unchanged: boolean
  /**
   * Set when the file on disk is not what the editor was based on. Carries the
   * current bytes so the editor can show what it would have overwritten.
   */
  conflict?: {
    onDiskHash: string
    onDiskContent: string
    mtimeMs: number
  }
  /** Set when the write was refused for a reason other than a conflict. */
  error?: string
}

// --- Effective view --------------------------------------------------------

/** Where a resolved capability came from. */
export type EffectiveSource = 'user' | 'cwd' | 'overlay'

/**
 * One skill, command, agent or workflow as a session would actually address it.
 *
 * The namespace is *predicted*, not observed: the platform prefixes everything
 * an overlay contributes with the plugin's manifest name (Spike A), and Helm
 * chooses that name when it synthesises the shim - so `<overlay>:<skill>` is
 * decidable before anything is launched. Cross-overlay collisions are therefore
 * impossible, and two overlays defining the same skill both appear, each under
 * its own prefix.
 */
export interface EffectiveEntry {
  /** What you type: `acme:think`, or `think` for an unnamespaced one. */
  invocation: string
  name: string
  source: EffectiveSource
  /** The overlay's plugin name, or null when the entry resolves unprefixed. */
  namespace: string | null
  /** The directory it came from. */
  origin: string
  path: string
  description: string | null
}

export type SettingsLayerKind = 'user' | 'project' | 'local'

export interface SettingsLayer {
  kind: SettingsLayerKind
  file: string
  exists: boolean
  /** Leaf paths the layer defines. */
  keys: number
  /** Set when the file is there but could not be parsed. */
  error: string | null
}

/**
 * One setting, and which layer's value a session would see.
 *
 * Keyed by leaf path (`env.FOO`, `permissions.defaultMode`) rather than by
 * top-level key, because the layers merge per leaf: measured on 2.1.225, a
 * project `settings.json` that sets `env.A` and a `settings.local.json` that
 * sets `env.B` yield a session with both, and where they set the same name the
 * local one wins. A top-level view would have reported `env` as wholly replaced.
 */
export interface EffectiveSetting {
  key: string
  /** JSON encoding of the winning value. */
  value: string
  winner: SettingsLayerKind
  winnerFile: string
  /** Every layer defining this key, highest precedence first. */
  candidates: Array<{ layer: SettingsLayerKind; file: string; value: string }>
  /** True when a lower layer's value is being shadowed. */
  overridden: boolean
}

/** An MCP server as configured, and whether a session would actually load it. */
export interface EffectiveMcpServer {
  name: string
  /** `project` is `.mcp.json`; `local` and `user` live in `~/.claude.json`. */
  scope: 'project' | 'local' | 'user'
  /** The file that defines it. */
  file: string
  /** The server's JSON, pretty-printed. */
  config: string
  transport: string
  /**
   * `.mcp.json` servers gate on first launch unless a settings layer has
   * approved them. Null for scopes where the question does not arise.
   */
  approved: boolean | null
  /** Why `approved` is what it is. */
  approvedBy: string | null
  /** Set when a higher-precedence scope defines the same name. */
  shadowedBy: string | null
}

export interface EffectiveView {
  cwd: string
  /** The profile this was computed for, or null for a plain directory. */
  profileId: number | null
  profileName: string | null
  overlays: Array<{
    /** The plugin manifest name, which is the namespace. */
    name: string
    projectPath: string
    exists: boolean
    skills: number
    commands: number
    agents: number
    workflows: number
  }>
  skills: EffectiveEntry[]
  commands: EffectiveEntry[]
  agents: EffectiveEntry[]
  workflows: EffectiveEntry[]
  /**
   * Names carried by more than one source, each under its own invocation. Not
   * a collision report - it cannot be one - but the thing a person wants to see
   * when two repos both define `think`.
   */
  sharedNames: Array<{ name: string; invocations: string[] }>
  settingsLayers: SettingsLayer[]
  settings: EffectiveSetting[]
  /** Instruction files the session would be given, in the order they arrive. */
  instructions: Array<{ path: string; source: EffectiveSource; bytes: number; origin: string }>
  mcpServers: EffectiveMcpServer[]
  warnings: string[]
  computedAt: string
}

// --- MCP scopes ------------------------------------------------------------

export type McpScope = 'local' | 'user' | 'project'

// ---------------------------------------------------------------------------
// Content viewer
// ---------------------------------------------------------------------------

/**
 * A directory of things worth reading, inside a scope.
 *
 * The four the spec names - `notes/`, `context/`, `.claude/skills/`, `docs/` -
 * are always offered when they exist, in that order, because they are the ones
 * a person goes looking for by name. Everything else is *found*: a top-level
 * directory holding anything readable is content whatever it is called, which is
 * how `lessons/` and `reference/` - full of artifacts Claude produced - end up
 * reachable without this file knowing they exist.
 */
export type ContentRootKind = 'notes' | 'context' | 'skills' | 'docs' | 'root' | 'found'

/**
 * *Why* a root is on screen, which is the thing the curated model was hiding.
 *
 * `named` is "offered by rule" - the four the spec names, plus the scope
 * directory itself, all of which are listed whether or not they turned out to
 * hold anything. `discovered` is "offered because walking it found content".
 * The badge is data rather than a UI inference from `kind` so that the pane and
 * a check are reading the same answer.
 */
export type ContentRootOffer = 'named' | 'discovered'

export interface ContentRoot {
  kind: ContentRootKind
  offer: ContentRootOffer
  /** Relative to the scope, forward-slashed. `''` is the scope directory itself. */
  relPath: string
  path: string
  label: string
  files: number
}

/**
 * What Helm will do with a file.
 *
 * `markdown` is rendered, `html` goes to the sandboxed frame, `data`, `text`
 * and `source` are shown as source, and `binary` is listed but not opened. The
 * distinction is by extension rather than by content because it decides which
 * *surface* opens, and a surface that changed after the read would flash the
 * wrong one first.
 *
 * A kind decides how a file **opens**, never whether it is **shown**. The
 * config tree already draws that line with `TEXT_EXT`, and the curated view
 * used to draw it in the wrong place: `contentFileKind` returned null for a
 * script and the walk then dropped it, so an agent's own `tools/` was invisible
 * in the pane meant for reading what the agent wrote.
 */
export type ContentFileKind = 'markdown' | 'html' | 'data' | 'text' | 'source' | 'binary'

export interface ContentFile {
  path: string
  /** Relative to the scope, forward-slashed. */
  relPath: string
  /** The `relPath` of the root it was found under. */
  root: string
  rootKind: ContentRootKind
  kind: ContentFileKind
  /** Basename without extension: what `[[a wikilink]]` names. */
  slug: string
  /** Lower-cased extension without the dot, `''` for a file that has none. */
  ext: string
  /** Frontmatter `title`, else the first heading, else the slug. */
  title: string
  size: number
  mtimeMs: number
  /** Frontmatter `type`, `date` and `tags` - the vault's own convention. */
  noteType: string | null
  date: string | null
  tags: string[]
}

export interface ContentScope {
  kind: ConfigScopeKind
  path: string
  label: string
}

export interface ContentTree {
  scope: ContentScope
  roots: ContentRoot[]
  files: ContentFile[]
  /** Directories that could not be read. Not fatal; the rest of the tree stands. */
  errors: string[]
  scannedAt: string
  /** How long the walk took, for the pane's own honesty about a cold scope. */
  tookMs: number
}

/** Why a tree entry is greyed. `null` for one that is not. */
export type ContentIgnoreReason = 'gitignore' | 'default'

/**
 * One line of a directory listing in the tree view.
 *
 * Ignored entries are carried rather than dropped: the complaint this whole
 * surface answers is "nothing on screen says what was left out", and a tree
 * that hid `node_modules/` would be making exactly that omission at the top
 * level of every repository.
 */
export interface ContentDirEntry {
  name: string
  /** Relative to the scope, forward-slashed. */
  relPath: string
  path: string
  directory: boolean
  /**
   * A symlink or junction. Listed, never descended - an overlay shim's junction
   * points back into a real repository, and a tree that walked one would list
   * another project's files as this scope's.
   */
  link: boolean
  /** How the file would open. `null` for a directory. */
  kind: ContentFileKind | null
  /** Lower-cased extension without the dot. `''` for a directory or no extension. */
  ext: string
  size: number
  mtimeMs: number
  ignored: boolean
  ignoredBy: ContentIgnoreReason | null
}

/** One directory, read on demand. */
export interface ContentDirListing {
  scopePath: string
  /** Relative to the scope, forward-slashed. `''` is the scope directory. */
  relPath: string
  entries: ContentDirEntry[]
  /** How many of `entries` are ignored, so a header can count them. */
  ignored: number
  /**
   * What decided the ignores: the repository's own rules, or Helm's built-in
   * list where there is no git to ask.
   */
  ignoreSource: ContentIgnoreReason
  /** Set when this directory could not be read at all. */
  error: string | null
  tookMs: number
}

/** One `key: value` from the frontmatter, as the header chip row shows it. */
export interface ContentChip {
  key: string
  value: string
  /** A list-valued key (`tags`) renders as several chips rather than one. */
  values: string[]
}

export interface ContentHeading {
  depth: number
  text: string
  slug: string
}

/**
 * One `[[wikilink]]`, and whether the vault has anything to point it at.
 *
 * A broken link is not an error here. The vault's own convention is that a link
 * to a note nobody has written yet is a note worth writing, so the renderer
 * marks it and moves on - which is why `resolved` is nullable rather than the
 * link being dropped.
 */
export interface ContentWikilink {
  /** Exactly what was between the brackets, before `|` and `#` were split off. */
  target: string
  label: string
  heading: string | null
  /** Absolute path, or null when nothing in the scope matches. */
  resolved: string | null
}

/** What the source turned out to contain. Counted while rendering, so a check
 * can compare them against its own read of the same file. */
export interface ContentCounts {
  tables: number
  taskItems: number
  taskItemsChecked: number
  codeBlocks: number
  /** Code blocks that got a real grammar rather than plain text. */
  highlightedBlocks: number
  callouts: number
  wikilinks: number
  brokenWikilinks: number
  tags: number
  headings: number
}

export interface RenderedMarkdown {
  /** Sanitised HTML. The renderer injects it; it never evaluates it. */
  html: string
  frontmatter: {
    /** Present at all - a file with no `---` block has none. */
    present: boolean
    fields: ContentChip[]
    raw: string
    /** Set when the block is there and is not parseable YAML. */
    error: string | null
    /** Line the closing fence sits on, 1-based, for the source view. */
    endLine: number
  }
  headings: ContentHeading[]
  links: ContentWikilink[]
  tags: string[]
  words: number
  counts: ContentCounts
  /** Languages a code fence asked for that no grammar was loaded for. */
  unknownLanguages: string[]
  tookMs: number
}

/**
 * A draft, tokenised, for the editor's underlay.
 *
 * Per line rather than as one block of HTML, because the underlay builds DOM
 * for a window of the file rather than all of it - see `CodeEditor`. The whole
 * file is tokenised even so: a string opened two hundred lines up changes the
 * colour of everything below it, so a window tokenised on its own would be
 * confidently wrong rather than merely late.
 *
 * `tooLarge` is the same ceiling the read views use, and it degrades the same
 * way: the text still edits, in one colour, and the pane says why.
 */
export interface EditorHighlight {
  /** The inner HTML of each line. Empty when `tooLarge`. */
  lines: string[]
  language: string
  highlighted: boolean
  tooLarge: boolean
  /** How long the tokenise took in main, for the latency group to report. */
  tookMs: number
}

// ---------------------------------------------------------------------------
// The Files view
// ---------------------------------------------------------------------------

/** How git sees one path in the working tree, against `HEAD` and the index. */
export type GitFileState = 'modified' | 'added' | 'deleted' | 'renamed' | 'untracked' | 'conflicted'

/**
 * Every changed path under a project, for the tree's letters.
 *
 * `files` is null when git could not be asked - which is "unknown" and never
 * "clean" - and `{}` when it answered with nothing changed. A project outside
 * any repository has `repo: null` and an empty map: there is nothing git could
 * say about it.
 */
export interface FilesStatus {
  root: string
  /** The repository's top level, or null when the project is not in one. */
  repo: string | null
  /** Project-relative, forward-slashed path to its state. */
  files: Record<string, GitFileState> | null
  error: string | null
}

/** The session that worked in an agent worktree. */
export interface AgentWorktreeChild {
  /** The child's name; a session a person started has its own name. */
  name: string
  /** The name of the session that started it, or null when a person did, or its record is gone. */
  parent: string | null
  running: boolean
}

/**
 * One checkout under a repository's `.floti/worktrees`, which Floti made for
 * an agent and left when the agent ended, because its branch holds the work.
 */
export interface AgentWorktree {
  /** The folder's name, which is also the branch's after `agent/`. */
  name: string
  folder: string
  /** The branch checked out there, or null for a detached checkout. */
  branch: string | null
  /** The session that last worked there (a running one first), or null when none did. */
  child: AgentWorktreeChild | null
  /** Whether git sees a modified or untracked file there; null when git could not be asked, never "clean". */
  uncommitted: boolean | null
}

/** What removing an agent worktree came to: gone, or the sentence saying why it stays. */
export interface AgentWorktreeRemoval {
  removed: boolean
  refusal: string | null
}

/** What a standing notice is about. */
export type NoticeKind = 'needs-you' | 'limit' | 'error'

/** The limit that refused a parent's start. */
export type NoticeLimit = 'children' | 'sessions' | 'depth' | 'starts'

/**
 * Something the host keeps for a person who is not looking: a child waiting on
 * them (until it stops), a start a limit refused or a child that ended in
 * error (until dismissed or the parent ends). Every window connected to the
 * host is given the same list.
 */
export interface StandingNotice {
  /** Names the notice for `notices:dismiss`; stable while it stands. */
  id: string
  kind: NoticeKind
  /** The session the notice is under, as its row id. */
  parentId: number
  parentName: string
  /** The child it is about; null for a refused start. */
  childId: number | null
  childName: string | null
  /** For needs you, what the child waits on; for a limit or an error, its sentence. */
  text: string
  /** ISO 8601: when the notice began. */
  since: string
  /** Which limit refused the start; only a limit notice has one. */
  limit: NoticeLimit | null
  /** For the starts limit, how many more starts Allow adds for this parent. */
  allowMore: number | null
}

/**
 * The lines of a file that differ from the last commit, numbered in the file
 * as it is now. Ranges are inclusive and 1-based.
 */
export interface LineChanges {
  changed: Array<[number, number]>
  /** Lines with something removed directly below them; 0 is above the first line. */
  removedAfter: number[]
  changedCount: number
  removedCount: number
}

/**
 * How one file stands against the last commit. Five answers, because each
 * says something different on screen and two of them - "no changes" and "could
 * not look" - must never be painted the same.
 */
export type FileChangeState =
  /** In the last commit. `lines` empty: unchanged since. */
  | { kind: 'tracked'; lines: LineChanges }
  /** Untracked, or staged but never committed: every line is new. */
  | { kind: 'new' }
  | { kind: 'ignored' }
  /** Not inside any git repository. */
  | { kind: 'outside' }
  /** git could not be asked. */
  | { kind: 'unknown'; reason: string }

/** One file, read for the file view. */
export interface FileView {
  root: string
  path: string
  /** Relative to `root`, forward-slashed. */
  relPath: string
  exists: boolean
  size: number
  mtimeMs: number
  binary: boolean
  /** Past the view's ceiling: not read, and the pane says so. */
  tooLarge: boolean
  /** The text, with line feeds only; how the file ends its lines is `eol`. */
  content: string
  eol: 'LF' | 'CRLF' | 'mixed' | null
  changes: FileChangeState
  /** Set when the file could not be shown at all - outside the project, a folder. */
  error: string | null
  /**
   * Set, saying why, when the host will not save it: it is in an agent's own
   * config, which the agent's own commands edit. Absent for every other file.
   */
  readOnly?: string
}

/** Every file in a project, for Ctrl+P. */
export interface FileListing {
  root: string
  /** Project-relative, forward-slashed. */
  files: string[]
  /** The repository's own list, or a walk where there is no repository. */
  source: 'git' | 'walk'
  /** The walk stopped at its ceiling, so the list is not the whole project. */
  truncated: boolean
  error: string | null
}

/** A file, its bytes, and - for markdown - what they render to. */
export interface ContentDocument {
  file: ContentFile
  content: ConfigFileContent
  rendered: RenderedMarkdown | null
  /** Set when the file could not be rendered at all; the source still shows. */
  error: string | null
}

export interface ContentSearchLine {
  line: number
  /** The line, trimmed, with the match still inside it. */
  text: string
  /** Offsets of the match within `text`. */
  from: number
  to: number
}

export interface ContentSearchHit {
  path: string
  relPath: string
  root: string
  title: string
  matches: number
  /**
   * The file's own name or title matched, whether or not its text did. Kept
   * separate from `matches` so a search for `journal-2026-08` finds the note
   * whose *filename* says so and the result can say that is why.
   */
  nameMatch: boolean
  /** The first few matching lines. Bounded, so one file cannot fill the pane. */
  lines: ContentSearchLine[]
}

export interface ContentSearchResult {
  query: string
  hits: ContentSearchHit[]
  filesSearched: number
  /**
   * The kinds whose bytes were read, so the status row can say what was
   * searched rather than leaving the reader to infer it. Every file is matched
   * on its *name* whatever its kind, which is why this is about bodies only.
   */
  bodyKinds: ContentFileKind[]
  /** How many of `filesSearched` had their text read. The rest matched by name. */
  filesWithText: number
  bytesSearched: number
  totalMatches: number
  /** Measured around the search itself, not around the read. */
  tookMs: number
  /** True when the corpus had to be read from disk rather than served warm. */
  cold: boolean
  truncated: boolean
}

// ---------------------------------------------------------------------------
// Templates
// ---------------------------------------------------------------------------

/*
 * The templates folder as the local host answers for it: the New Harness
 * picker and preview, the template manager, and Save as template. The host
 * reads and writes the folder; the window only shows these.
 */

/** One row of the picker. */
export interface TemplateChoice {
  /** Directory name, and what `template:` records. `minimal` for the built-in. */
  id: string
  label: string
  description: string | null
  /** `order:` from `template.yaml`. Sorted on before the label; null sorts last. */
  order: number | null
  /** The built-in scaffold, which has no directory behind it. */
  builtIn: boolean
}

export interface TemplateListing {
  /** `minimal` first and always, then the user's by `order` then label. */
  templates: TemplateChoice[]
  /** Where the user's templates live, so the dialog can say so. */
  dir: string
  /** One sentence per template that could not be read. */
  problems: string[]
}

/** What the New Harness dialog lists under "What gets written". */
export interface TemplatePreview {
  template: string
  /** Display order - the manifest first, then everything else by path. */
  entries: string[]
  /** The sentence under the list. */
  note: string
  problems: string[]
}

/** One file in a template, and what a harness would receive it as. */
export interface TemplateFile {
  /** Relative to the template directory, forward-slashed. */
  relPath: string
  /**
   * The path a new harness gets: `.tpl` dropped, `dot-claude` rewritten, and -
   * for a `.gitkeep` - the empty folder it declares, with a trailing slash.
   * Empty when the file is not written at all.
   */
  target: string
  size: number
  mtimeMs: number
  /** A `.tpl`: its `{{...}}` placeholders are filled in on the way out. */
  substituted: boolean
  /** A link, which the engine refuses. Listed so the author can see why. */
  link: boolean
}

/** One template, as the manager shows it. */
export interface TemplateDetail {
  id: string
  dir: string
  /** From `template.yaml`, or the folder name when it sets none. */
  label: string
  description: string
  /** Whether there is a `template.yaml` at all - absent is legal. */
  hasManifest: boolean
  /** Everything but `template.yaml`, sorted by path. */
  files: TemplateFile[]
  fileCount: number
  totalBytes: number
  /** The newest mtime in the tree, or the directory's own when it is empty. */
  modifiedAtMs: number
  problems: string[]
}

export interface TemplateWriteResult {
  ok: boolean
  /** The template's id afterwards, or null when nothing was written. */
  template: string | null
  /** What went wrong, as sentences. Rendered straight into the dialog. */
  problems: string[]
}

export interface TemplateDeleteResult {
  ok: boolean
  /** The template that is gone, or null when nothing was removed. */
  template: string | null
  problems: string[]
}

export interface TemplateImportResult {
  ok: boolean
  /** Targets that did not exist before, relative to the template. */
  created: string[]
  /** Targets that did, and were replaced. Named rather than counted. */
  replaced: string[]
  problems: string[]
}

/** Which of the two this is. The defaults and the wording differ; nothing else. */
export type FolderTemplateKind = 'harness' | 'folder'

/** One thing at the top of the folder, and whether it would be copied. */
export interface FolderEntry {
  name: string
  directory: boolean
  /** Files under it that would be copied. `truncated` says the count stopped. */
  fileCount: number
  bytes: number
  truncated: boolean
  /** Ticked when the dialog opens. */
  included: boolean
  /** Set when Helm will not copy it whatever the user ticks, and why. */
  refused: string | null
  /** A link. Never followed, never copied. */
  link: boolean
}

export interface FolderTemplatePreview {
  dir: string
  kind: FolderTemplateKind
  entries: FolderEntry[]
  /** Files and bytes of everything ticked by default. The number that matters. */
  fileCount: number
  totalBytes: number
  /** The sentence under the list. */
  note: string
  problems: string[]
}

export interface SaveFolderAsTemplateResult extends TemplateWriteResult {
  /** What landed, relative to the template. */
  created: string[]
  fileCount: number
  totalBytes: number
}
