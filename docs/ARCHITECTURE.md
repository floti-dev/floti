# Architecture

How Floti and Helm are put together: what each process owns, how sessions live
and end, what a window shows of them, and how the browser pane, overlays,
templates, the Files view and the other surfaces behave. CLAUDE.md lists the
rules an agent must not break; this is the behaviour behind them. The agent
tools are in [AGENT-TOOLS.md](AGENT-TOOLS.md), plugins in
[PLUGINS.md](PLUGINS.md), and credentials and keys in
[SECURITY.md](SECURITY.md).

## Processes and what each owns

- Every channel a Helm page uses is declared in `shared/ipc.ts`. The preload
  exposes three generic functions; a feature adds a channel, not a bridge
  method. `term:*` (the spike page's single pty) and `session:*` (the host's
  sessions) never mix.
- Each Helm page (the window, and the hidden plugin host window) reaches the
  host over its own protocol connection, which main pipes to a bridge unread
  (`hostLink:*`); the bridge holds the device key, never a window. Electron
  IPC carries only what `channels/boundary.rs` lists as client-only, and its
  host-internal channels from the hidden plugin window: `registerIpc`
  registers nothing else, and a page's router (`app/host/router.ts`) refuses
  any other channel at once. `e2e/boundary.spec.ts` fails naming any channel
  that crossed. Main runs no session, terminal, config reader, content,
  template or scan service of its own; those are the host's.
- The host never opens `helm.db` or learns Helm's data folder. What only
  `helm.db` holds (archived conversations whose transcript is gone, their
  usage, History's names) main reads through core's
  read-only pages (`store/copy.ts`) and sends on `carry:*`, in batches, until
  the host records in `floti.db` that the copy finished
  (`main/host/own-data.ts`). The host's merge keeps what it already holds,
  and a copy that stops part way is sent again at the next connect. Config
  file versions kept in `helm.db` are not copied; those already in
  `floti.db` stay there.
- The host owns process lifetime. Every session's and plain terminal's pty and
  process tree is held by the session keeper (`floti keeper`,
  `crates/host/src/keeper`), a process the host starts outside its own tie,
  which holds nothing else: a host that crashes, panics or is killed takes no
  session with it, and the next host takes them back under the same ids, with
  their agent tools and the device that started each. A keeper that dies ends
  what it holds, and the host offers those sessions back. Only a deliberate
  stop ends the keeper. Quitting a client ends the sessions it started only
  where its setting for that host says so; a crash ends nothing; stopping the
  host ends every session. The plain terminals a client opened end when it
  quits. Helm's setting is `quitSessions`, kept in `helm.db` keyed by host
  (this PC's is `local`, End when absent; a remote host's is its id, Keep when
  absent). Quitting with sessions it would end asks first: End and quit, Keep
  running and quit (this quit only, never written to the setting), or Cancel.
  A quit is held until every host has been told, so a reopen in that wait
  (macOS's `activate`) opens no window in the quitting process; Helm calls
  `app.relaunch()` as the quit finishes and a fresh Helm starts once the old
  one has exited (`main/reopen.ts`). Helm never stops the host on its own: its deliberate quit sends `host:quit`
  and lets go, only Settings' Stop host (after saying how many sessions it
  ends) sends `host:stop`, and a host Helm started (`floti host --desktop`)
  exits by itself once it runs nothing and no client is attached. Installing
  the desktop app stops no host: the installed app runs floti from the user's
  own folder, outside its install (`floti install-self`, PACKAGING.md's
  "floti in your own folder"), and a running copy there is renamed aside, so
  the old host runs on until Restart host to update hands its sessions to the
  new one through the keeper. An update of floti on another host runs
  `floti update` there where that floti has it (over the host's connection
  with `update:install`, or over its login), which relaunches the host
  through the keeper, so it ends no session the new host takes back
  ([PACKAGING.md](PACKAGING.md#installing-floti-on-a-hosts-computer)). An
  update that installs over an older floti on another host,
  and the one install over a Helm whose host ran from inside its install
  folder, stop the host through `floti update-stop --stop`
  (`crates/host/src/update_stop.rs`), asked first when it would end sessions,
  and their rows stay running, so the next host offers them back. `floti
  update` (`crates/host/src/update`) replaces the floti binary, then the
  desktop app where it is older once it has quit, each only after its
  download has passed its signature and SHA-256 checks
  ([PACKAGING.md](PACKAGING.md#floti-update)), and asks the host on this
  computer to start the new floti in its place; it asks the person nothing.
  The host updates floti by itself too, at most once a day while its
  `updateCheck` setting is on. Either way the host relaunches without ending
  a session: it lets go of its keeper, starts the new floti in its place
  (`exec` on macOS and Linux, a new process on Windows that waits for the old
  one to end) and exits, and the new host takes the keeper back, or, when it
  does not speak the keeper's link version, ends it and offers its sessions
  back in their tabs ([PACKAGING.md](PACKAGING.md#the-handover)). When the
  connection ends without a stop or a quit, the window keeps each tab's
  screen, dimmed with input off, and Helm starts a new host if a desktop
  started the one that died, then reconnects. The new host's list decides
  each: a session or shell it still runs is carried again from its redraw, and
  nothing is resumed. One it lists lost, or that is lost while it stays (its
  keeper died), comes back through the host's restore offer, which the window
  asks for again then; a lost shell, as a new shell in its folder.

## Hosts on other computers

- A host on another computer is reached by main through `floti bridge`,
  over the user's own `ssh -o BatchMode=yes <target>`, the command they
  gave, or `wsl.exe -d <distro>` (on this PC, or through ssh to a Windows
  host), one link per host and one page link per host for each window
  (`hostLink:open` names it). Floti adds no ssh option, never skips a
  host-key check, reads or writes nothing under `~/.ssh` and holds no SSH
  secret: a transport that needs a password or a key accepted fails with its
  last line, and the fix is `ssh <target>` in a terminal. Adding a host is
  connecting to it: the first connection pins the key it proves, and
  nothing is approved or typed on the host. The link never
  starts that host and a drop ends nothing there; Remove ends the link and
  removes the host's client folder, and ends nothing on the host.

## Tabs, panes and families

- Every client sees every session in the host, and tabs, panes and splits are
  kept per device: `paneLayout` stays in `helm.db` and never reaches the host.
  A window gives a tab only to a session its saved layout names or that it
  opened. A running child of a session it has a tab for is shown inside that
  tab while "Show children in their parent's tab" is on and the pane has room
  (`familyRoom`), and counts as open here, so the window's connection carries
  its terminal (`shownSessionIds`) along with those of the sessions that have
  a tab. A running child's pane there can be closed without ending it: it
  keeps running, is not open here (its terminal is not carried), and is shown
  as a child with no room is, by its chip in the parent's crumb and its row in
  the tree, whose peek's Open puts it back where it was (its Open as tab gives
  it a tab; Open never does). Which children are shown, shrunk, closed or
  opened as tabs is this device's `paneLayout` (`families`) and never reaches
  the host. Every other running session is in the sessions pane as not open
  here, with Open (a tab whose screen comes through the host's attach, never a
  second restore path) and End. A child with nowhere on screen here, running
  or ended, has a peek instead (from its row, or its chip in the parent's
  crumb): its screen comes through the host's watch-only attach
  (`session:data` opened with `watch`), which takes no keys and no size, so a
  peek never types into the child or resizes it; it is drawn at the grid the
  host reports (`session:size`); its Open as tab is the attach above and
  closes the peek. Closing a running session's tab asks first, naming its
  host, which the window names, and the children that end with it and any
  started to outlive it; its button counts what it ends (End 4 sessions).
  Close tab, keep running in the tab's menu ends nothing, and puts a child
  back in its parent's family view.
- A child that has exited is drawn in the Sessions tree and its parent's
  family view only while it is kept and its parent runs (`inFamilyTree` in
  `core/layout/family.ts`): its parent stopped it with `keep`, or a person
  stopped it. Any other ended child (one that exited by itself, one its parent
  stopped without `keep`, one that ended with its parent) leaves both at once,
  stays in History and is still its parent's to resume, and its parent still
  hears how it ended. When a parent ends, its kept children leave with it, and
  a running child started to outlive it stays as a top-level row naming it. A
  kept child's row has Remove and its parent's row has Remove stopped
  children, which ask nothing and send `session:remove`: the host clears
  `kept` and every device's tree follows.

## Live sessions

- Helm renders nothing for a live session and answers no permission
  prompt. The host reads one thing in a session's output, OSC 7501 status
  reports, kept as the program said them (a message cut to 200 characters),
  and draws no conclusion from
  screen text. It passes every byte on untouched and puts nothing between
  the user and the TUI. What a parent sends its child (`agents_send`) waits
  in the child's queue and arrives as its next turn once it can take one, and
  a person typing in that terminal comes first;
  what a parent reads of its child (`agents_read`) is the screen as text,
  interpreted by nobody. The host keeps the screen, hands a parent its
  child's screen as text, and a command child's recent output lines and its
  last 40 at exit (`session/screen.rs`).
  Rendering an archived transcript (read-only, after the fact) is allowed.
  This holds for every agent: no RPC or JSON mode in place of its own TUI.

## Browser pane

The browser's permissions and reach rule are in
[SECURITY.md](SECURITY.md#browser-pane), and the browser tools agents drive it
with in [AGENT-TOOLS.md](AGENT-TOOLS.md#helm-browser).

- `will-navigate` and `setWindowOpenHandler` are denied on every web-contents
  in `main/index.ts`. Browser views are exempted by a `webContents.id` registry
  read inside those guards. Widen the registry, never the guard.
- Pages live in one Browser tab, in its own strip (`browserStrip.ts`), never
  among a pane's tabs.
- From a browser view, a `window.open` with features opens a real popup
  window; `_blank` and a plain `window.open` open a page in the Browser tab
  that adopts the web contents Chromium made (`createWindow`), so both keep
  `window.opener` (OAuth needs it), and a middle-click loads its link in a new
  page there. Both share the partition,
  permissions, reach rule and exemption registry. A popup shows its host in
  its title bar and is never an agent's: an agent page's opens are more of its
  pages.
- A view can lose its web contents (a page that called `window.close()`). Reach
  them only through `contentsOf` in `browser.ts`. `registerIpc` wraps every
  send handler, because a throw there is an uncaught main-process exception.
- A native view paints over all DOM, so it hides while anything is drawn over
  it, through `overlayOpen()` (`ui/src/lib/overlay.ts`), subscribed once in
  `useBrowsers`, and leaves a still of the page in its place
  (`browser:snapshot`). The attention toasts (needs you, While you were away)
  are drawn clear of the view while the sidebar shows, and hide it as a dialog
  does while the sidebar is hidden. The view never enters the top 36px, where
  Windows draws the window controls, except in fullscreen, when the page has
  the whole window and there is no title bar.
- The browser's keys are one table (`shared/browserKeys.ts`): main reads it in
  `before-input-event` for a key pressed in a page, the window for a key in
  Helm's chrome, and both run the window's one dispatch. A key an agent sends
  (`agentInput`) is always the page's.
- Hide a view with `setVisible(false)`, which keeps the page capturable,
  scriptable and clickable.

## Overlays and templates

- The host composes overlays in each session's launch folder under its own
  data folder, never `%TEMP%`: they hold junctions into real repositories.
- Anything removing an overlay or a shim or walking a template unlinks links
  (junctions on Windows, symlinks on macOS and Linux) and never walks into
  one. `fs.rm` with `recursive: true` has been seen to go through a junction
  and empty the repository folder it points to.
- Launch folders are swept only at host start, and overlay shims left in
  Helm's data folder only at Helm's start
  (`createServices`), each only where every owning process is provably dead.
  `EPERM` counts as alive; unknown means keep.
- Templates live in `templates` in the host's config folder: for the
  installed app `~/.config/helm` where that is there, else
  `~/.config/floti`; otherwise the folder holding `templatesDir` in
  `paths.ts` (`helm-data/templates` when portable). The host reads and seeds them at
  its start. The shipped README and example are written only when the
  directory is absent, and nothing there is ever overwritten. The in-app
  Templates manager makes templates (from a folder too, `template:fromFolder`),
  renames, imports, fills (`template:substitute`) and deletes them and edits
  their label and description; it has no file editor.
- A workspace is a folder with a `workspace.yaml`, or the `harness.yaml`
  workspaces had while they were called harnesses
  (`discovery/scan.rs`, `WORKSPACE_MANIFESTS`). Discovery, launches and
  overlays read either the same way, `workspace.yaml` first when both are
  there. `workspace:create` writes `workspace.yaml` only, refuses a folder
  that has either, and a template cannot supply either. Nothing renames or
  rewrites a `harness.yaml`.
- Someone upgrading sees once, over their sessions, that harnesses are now
  called workspaces and nothing in their folders changed
  (`UpgradeNoteDialog`). Whether it was seen is `upgradeNoteSeenAt` in
  `helm.db`; a first run that completes stamps it too, so a new install never
  shows it.

## Terminals and session state

- `renderer/src/terminal.ts` and the host's session environment
  (`crates/host/src/session/env.rs`) are tuned for TUI fidelity (SPEC 8.3);
  `ptyEnv` (`main/pty.ts`) is main's own ptys (the spike page's and the
  drivers'), kept to match. Terminal preferences are passed into
  `createTerminal`; never route a setting through `term:*`. The palette and
  contrast options are fixed. `estimateGrid` must keep measuring the way xterm
  does.
- Session state comes from each backend's own signals, read only, then from
  OSC 7501 reports; a session with neither shows the plain tab. For Claude
  Code that is `~/.claude/sessions/<pid>.json`. Stale records are filtered by
  liveness at read time, and anything that cannot be interpreted degrades to
  the plain tab.
- Every backend resumes in the directory history recorded; for Claude Code,
  `claude --resume` there. Claude Code's transcripts are found by scanning
  `projects/*` for `<uuid>.jsonl`, never by deriving a path.

## Agent config

- Every agent's config directory (`~/.claude` for Claude Code) is read-only to
  Floti; the agent's own commands (`/config`, `/mcp`, `/hooks`, `/agents` for
  Claude Code) edit it. What composition reads of it - the scopes and trees a
  template imports from, and the view of what a profile composes - is
  `config:scopes`, `config:tree` and `config:effective`
  (`crates/host/src/agent_config`), none of which writes. A Files-view
  save or restore at or under a backend's config folder or global config
  file (`AgentConfigFolders`, from each backend's own `agent_config`), as
  spelled or where its links lead, is refused naming the agent's own commands, and the Files view shows those
  files read-only (`FileView.readOnly`). The transcript
  archive copies into the host's `floti.db`, and never writes there. Copying `helm.db`'s archive into
  `floti.db` writes neither the agent's config nor `helm.db`, and logs
  counts, never a message. What a launch needs (agent
  tools, settings, composed instructions, overlays) is a per-launch file under
  the data directory, never a write into the agent's config.

## Usage figures

- Usage figures paint nothing rather than a wrong number. A stale reading of a
  window that is still running paints lower bounds. Roll-over is judged before
  age. Dollar figures are estimates and say so.

## Processes and ports

- The process-and-ports pass costs about half a second, so it runs only while
  the sessions pane is on screen or a `session_detail` call is waiting, every
  4s, in the host (`server/features/resources.rs`). Passes never overlap.

## Files view

- The Files view writes through `content:write`, and puts back a snapshot
  through `content:restore`. The host saves any file the view reads as text
  (the view offers Edit for a markdown note): snapshotted first, refused on a
  hash conflict, scoped to the deepest folder Helm knows that holds the file.
  `ContentGuard` (`workspace/content/guard.rs`) refuses a relative path, a
  binary file, an agent's config and anything under `repos`, `.git`,
  `node_modules`, build output and the rest of its list. Everything else in
  it is read-only, and
  every `files:*` call names a root the host checks it knows
  (`workspace/roots.rs`: a scanned project, a profile's folder, a hosted
  session's working directory). Changed lines are the working tree against `HEAD`, said
  as that and never attributed to a session: git does not know who made a
  change. Only what is on screen is watched, and the window tells each host
  the whole set (`files:watch`).

## Agent worktrees

- A project's agent worktrees (`worktrees:list`, `worktrees:remove`, served by
  `server/features/worktrees.rs` over `workspace/worktrees.rs`) are the folders
  `git worktree list` gives under `<repo>/.floti/worktrees` and no other
  worktree of the repository. The project must be one the host knows (a
  scanned project, a workspace or a profile's folder; a hosted session's working
  directory does not make one). A removal is `git worktree remove` without
  `--force`, never a recursive delete, and keeps the `agent/<name>` branch. It
  is refused, in the answer and with the worktree left as it is, for a folder
  that is none of the project's agent worktrees, for one a running session
  works in (naming it) and for one with uncommitted changes (modified or
  untracked). Whether a worktree has uncommitted changes is `null` when git
  could not be asked, never "clean". The project page of a folder on this PC
  lists them as its Agent worktrees section (`LocalProjectPane`, read when the
  page opens, after a removal and when a session in the project starts or
  ends) and Remove asks nothing first, because the host already refuses what
  would lose work and says why on that row.
