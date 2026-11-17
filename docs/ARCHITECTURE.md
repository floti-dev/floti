# Architecture

How Floti and Floti are put together: what each process owns, how sessions live
and end, what a window shows of them, and how the browser pane, overlays,
templates, the Files view and the other surfaces behave. CLAUDE.md lists the
rules an agent must not break; this is the behaviour behind them. The agent
tools are in [AGENT-TOOLS.md](AGENT-TOOLS.md), plugins in
[PLUGINS.md](PLUGINS.md), and credentials and keys in
[SECURITY.md](SECURITY.md).

## Processes and what each owns

- Every channel a Floti page uses is declared in `shared/ipc.ts`. The preload
  exposes three generic functions; a feature adds a channel, not a bridge
  method. `term:*` (the spike page's single pty) and `session:*` (the host's
  sessions) never mix.
- Each Floti page (the window, and the guided first run's setup window while
  it is open) reaches the host over its own protocol connection, which main
  pipes to a bridge unread (`hostLink:*`); the bridge holds the device key,
  never a window. Electron IPC carries only what `channels/boundary.rs` lists
  as client-only: `registerIpc` registers nothing else, and a page's router (`app/host/router.ts`) refuses
  any other channel at once. `e2e/boundary.spec.ts` fails naming any channel
  that crossed. Main runs no session, terminal, config reader, content,
  template or scan service of its own; those are the host's.
- The host never opens `desktop.db` or learns Floti's data folder. What only
  `desktop.db` holds (archived conversations whose transcript is gone, their
  usage, History's names) main reads through core's
  read-only pages (`store/copy.ts`) and sends on `carry:*`, in batches, until
  the host records in `floti.db` that the copy finished
  (`main/host/own-data.ts`). The host's merge keeps what it already holds,
  and a copy that stops part way is sent again at the next connect. Config
  file versions kept in `desktop.db` are not copied; those already in
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
  quits. Floti's setting is `quitSessions`, kept in `desktop.db` keyed by host
  (this PC's is `local`, End when absent; a remote host's is its id, Keep when
  absent). Quitting with sessions it would end asks first: End and quit, Keep
  running and quit (this quit only, never written to the setting), or Cancel.
  A quit is held until every host has been told, so a reopen in that wait
  (macOS's `activate`) opens no window in the quitting process; Floti calls
  `app.relaunch()` as the quit finishes and a fresh Floti starts once the old
  one has exited (`main/reopen.ts`). Floti never stops the host on its own,
  except to restart an older local host onto a floti that speaks its
  protocol, whose keeper carries the sessions over: its deliberate quit sends `host:quit`
  and lets go, only Settings' Stop host (after saying how many sessions it
  ends) sends `host:stop`, and a host Floti started (`floti host --desktop`)
  exits by itself once it runs nothing and no client is attached. Installing
  the desktop app stops no host: the installed app runs floti from the user's
  own folder, outside its install (`floti install-self`, PACKAGING.md's
  "floti in your own folder"), and a running copy there is renamed aside, so
  the old host runs on until the new desktop reaches it. A host that refuses
  the desktop's `hello` as older (`update: 'host'`) is restarted onto the
  floti the desktop runs, with no question, inside main's connect
  (`connectUpdatingLocalHost` in `main/host/launcher.ts`), once `floti
  version` there says it speaks a protocol the `hello` is accepted by
  (`hostUpdatePlan` in `@helm/core`); the keeper hands its sessions to the
  new host. A host that is ending stops answering on its socket before it
  lets go of its data folder, so the desktop waits for that too (`floti
  wait-host-exit`), after a restart and whenever a host it starts finds the
  folders held, and then starts one more host, not one per try. At most one such restart runs per connect, and a floti that
  cannot serve is never restarted onto: the refusal names it instead. Every
  read the window makes waits on that connect, so none fails while the host
  restarts. Restart host to update, the same restart on the user's click,
  applies the same check. Main publishes each step of the connect
  (preparing, connecting, starting, updating, ready, refused, unreachable)
  on the client-only `hostLink:step`, answers the one under way on
  `hostLink:currentStep`, and the window keeps it in one store
  (`renderer/src/app/host/localHostStep.ts`). Until the window's layout
  arrives, the main area shows those steps in place of empty panes
  (`StartupSteps`), stepping aside while the host cannot be used, when
  an empty layout shows a wait (`StartupWaiting`) instead of the welcome page. A local host that refuses the
  desktop and was not updated, or that cannot be reached, is said once: the
  window's one notice (`HostNotice` in the banner slot, read from
  `localHostUsable.ts`) gives the sentence and the action as a button (Try
  again, or Update Floti for a host newer than the desktop), and the
  Sessions pane and the Restore tab say they wait for the host. The window's
  reads wait rather than fail: main answers a page's `hostLink:open` only
  once its own link has a host (`HostLink.usable`), main's link tries a failed
  connect again on its own backoff (`retryFailedConnects`), and only that
  retry or the notice's Try again (`hostLink:reconnect`) connects again, so a
  waiting window starts no connect and no `floti host` of its own. Reads that
  answer with what Floti keeps itself (its own settings, its build) answer
  without the host meanwhile, and what a person starts (a launch, a write, a
  restore) fails at once with one short sentence pointing at the notice
  (`connectionForAction`). A saved Restore tab stays while it waits. A remote host keeps its own state in its tree
  header. An update of floti on another host runs
  `floti update` there where that floti has it (over the host's connection
  with `update:install`, or over its login), which relaunches the host
  through the keeper, so it ends no session the new host takes back
  ([PACKAGING.md](PACKAGING.md#installing-floti-on-a-hosts-computer)). An
  update that installs over an older floti on another host,
  and the one install over a Floti whose host ran from inside its install
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
  screen, dimmed with input off, and Floti starts a new host if a desktop
  started the one that died, then reconnects. The new host's list decides
  each: a session or shell it still runs is carried again from its redraw, and
  nothing is resumed. A session the window held running that the list no
  longer names is asked after (`session:endings`, protocol 4.1), and told as
  its row says it ended: exited or stopped meanwhile, or lost. A host before
  4.1 cannot say, and such a session reads as lost. One lost, or that is lost
  while it stays (its keeper died), comes back through the host's restore
  offer, which the window asks for again then; a lost shell, as a new shell in
  its folder.
- The host stops on the OS's stop requests (`StopSignals`,
  `crates/host/src/platform/startup`). Ctrl+C, and on macOS and Linux SIGTERM
  and SIGHUP, are a plain stop, which records every session as stopped by
  Floti; `floti host` run in a Windows terminal also stops so on Ctrl+Break
  and its console closing. Windows tells a process that has loaded user32, as
  floti has, that the user is signing out, the computer is shutting down or
  Restart Manager is closing it only through its top-level windows, never
  with the console's logoff and shutdown events, and it ends processes from
  the highest shutdown level to the lowest, in no set order within one level,
  where every process starts at 0x280. So every Windows host, with or without
  a console or `--desktop`, asks for the top of the applications' range
  (0x3FF, `SetProcessShutdownParameters`), so it is told before its keeper
  and its sessions' processes are ended, and keeps a hidden top-level window
  of its own (class `FlotiHostEndSession`, `end_session_windows.rs`) on a
  thread of its own from its start to the end of its stop. It answers
  `WM_QUERYENDSESSION` yes at once, so it never holds up a sign-out. On
  `WM_ENDSESSION` for a session that ends it sets a shutdown block reason
  ("Floti is saving its sessions") and stops the host as for an update: the
  records freeze first, so no session Windows kills is recorded as exited,
  every row is left running, and the next start offers each session for
  restore once. It returns only once `run_host` says the stop is over
  (`HostStopped`), or after 20 s: once it returns, Windows may end the
  process at any moment. The notification area's window is not used for
  this: it exists only for a host a desktop started. A host started over SSH
  runs in a non-interactive window station and may get no `WM_ENDSESSION`;
  there it is simply ended, as before, and whether a session is offered for
  restore depends on the order Windows ends things in.

## First run

A first start is one with no data folder: the open that creates `desktop.db`
(`settleWalkthrough` in core) writes the `walkthrough` setting at step 1. A
database that was there before has its migrations applied already, so it
opens the shell. `walkthrough` is the step (1 to 6) and the workspace step 4 chose, kept in `desktop.db` on this device; null
means it is not showing.

- Steps 1 to 4 run in a setup window of their own, before the shell exists:
  main opens it instead of the shell whenever the step is 1 to 4
  (`openSetup` in `main/index.ts`). It is the shell's page with
  `?window=setup` (`app/main.tsx` renders `setup/SetupWindow.tsx`), so it has
  the same preload, navigation guards and IPC boundary, and its own
  connection to the host (`isHelmPage` names it).
- The setup window writes the step on every move. Once step 4 makes or
  chooses a workspace, is skipped, or setup is skipped on step 1, it stamps
  `setup:complete` on the host and asks main for `walkthrough:openShell`:
  main opens the shell (or brings the open one forward with the settings the
  setup window wrote) and only then closes the setup window, so the app is
  never left with no window.
- Steps 5 and 6 run in the shell's main area (`setup/ShellWalkthrough.tsx`),
  with the workspace selected in the sidebar. Step 6 starts the session with
  its first message as the launch's `openingPrompt` (protocol 4.2), passed to
  the agent as its opening prompt and never typed; starting it or skipping
  sets `walkthrough` to null.
- Closing either window leaves the step where it was, so the next start
  resumes there. Settings > General's Show the walkthrough sends
  `walkthrough:open`: main writes step 1 and opens the setup window beside
  the shell.
- The agents step reads each backend's own check (`setup:status`), which
  decides a sign-in from its artefacts existing and never opens one; Unknown
  stays Unknown. The host step writes this PC's `quitSessions` and opens the
  add-a-host dialog as Settings does.

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
  nothing is approved or typed on the host. A drop ends nothing there;
  Remove ends the link and removes the host's client folder, and ends
  nothing on the host.
- When the computer wakes or its screen is unlocked (`powerMonitor`
  `resume` and `unlock-screen`), main asks each connected host `host:ping`
  over its own link, one check per connection at a time (`remote.ts`). Any
  answer within 3 s, a refusal included, leaves the link alone. With none,
  the transport died while the computer slept, which ssh can take minutes to
  notice: main ends that host's page bridges, telling each page at once that
  its connection ended without a stop, lets go of its own connection, ends
  each of those bridges with every process it started on this computer
  (`treeKill`, so a ProxyCommand under ssh goes too; the host is on the
  other end and is never one), and reconnects as after any drop, so the host shows as reconnecting until it
  is back. A host already waiting to try again tries at once. Nothing is
  added to the transport and nothing branches on the OS.
- The link starts nothing itself. `floti connect`, which the transport runs
  on the host's computer, starts the host there when none answers, as the
  OS's startup rule says (`HostStartup::on_connect` in
  `platform/startup`): on Windows always, since nothing starts a host there
  at sign-in; in a WSL distro whose setup recorded it (`start-on-connect`),
  where no systemd runs; on macOS and Linux otherwise never, since the host
  starts at sign-in. On Windows the host it starts runs detached, out of the
  login's job (`start_detached`), with `SSH_CONNECTION`, `SSH_CLIENT` and
  `SSH_TTY` taken out of its environment, so OpenSSH closing the login ends
  neither the host nor its keeper, and nothing the host starts looks like an
  SSH login. A login whose job would not let it go starts none and says so.
  OpenSSH runs a login in session 0, with no desktop, so nothing such a host
  runs shows a window there; Floti opened at the desktop connects to it as
  to any host. An administrator's login gets the full, elevated token:
  `floti connect` there starts nothing while the user has a desktop session
  (`TokenElevation`, then `desktop_session`), and says to open Floti on that
  PC; with nobody signed in it starts the host, whose pipes and owner-only
  files carry a Medium integrity label (`UserOnlyDescriptor`), so the same
  user's desktop client, at Medium, is served by it.

## Tabs, panes and families

- Every client sees every session in the host, and tabs, panes and splits are
  kept per device: `paneLayout` stays in `desktop.db` and never reaches the host.
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
- A session that has exited, by itself or stopped, has no row in the
  Sessions tree on any device, a kept child and a command child included
  (`inSessionTree` in `core/layout/family.ts`). One whose tab is in front of a
  pane on screen when it ends keeps its row until the user moves away from it,
  so the end is seen; its tab stays until it is closed. A lost session (its
  host died or restarted under it) keeps its row as the restore offer. Every
  ended session is still in History, and a stopped child is still its parent's
  to resume by name, which gives it a row again. Each device applies the rule
  to the host's rows, because which tab is in front is the device's own.
- A child that has exited, by itself or stopped, kept or not, leaves its
  parent's family view and the parent's crumb at once (`inFamilyTree`), and
  its parent still hears how it ended. A lost child stays there as a header
  with no Close. When a parent ends, a running child started to outlive it
  stays as a top-level row naming it.

## Live sessions

- Floti renders nothing for a live session and answers no permission
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
with in [AGENT-TOOLS.md](AGENT-TOOLS.md#floti-browser).

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
- Sign-ins survive a restart. `browserSession()` installs `keepSignIns`
  (`main/browserCookies.ts`): every cookie a page sets without an expiry is set
  again with one 30 days out. A change to the cookie cancels a rewrite still
  pending, and a rewrite that reached the store after the cookie changed is
  undone, which the order of the store's change events shows. The partition's
  user agent is Electron's without its `Electron/` token and the app's name,
  so Google's sign-in accepts it. The cookie store is written to disk after
  each page load and, awaited, in the held `before-quit` round trip, once the
  rewrites in flight have landed (`flushBrowserCookies`, bounded at 2 s).
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
- A blank page (a new tab sent nowhere yet: no address, nothing pending,
  loading, refused or wrong; `browserPageBlank` in `BrowserPane.tsx`) is kept
  off the screen too. Its view is clear until its first navigation, and a
  clear view still takes clicks, so the Browser tab's empty state ("No pages
  open", whose Open a page puts the caret in the address bar) is drawn where
  the page would be and the view comes back the moment an address is sent.
- The bar's own menus (More, Share) and the address suggestions are drawn
  over the live page instead, in the **menu layer** (`main/menuLayer.ts`):
  one transparent `WebContentsView` per window, made with the window's first
  page, loading `layer.html`, a Floti page with the window's preload and CSP.
  It is not a browser view: it is not in the exemption registry, so
  `will-navigate` and `setWindowOpenHandler` stay denied for it. It is raised
  above every page whenever one is added or raised (`onViewRaised` in
  `browser.ts`, the fullscreen raise included). A transparent part of a view
  still takes clicks, so its bounds are always the open menu's rectangle, and
  idle it is hidden. It is shown only once its page has said it is listening
  (`browser:layer-report` `ready`), so a page still loading, or loading again,
  never shows an empty view that takes clicks; a layer whose renderer died is
  replaced by a new one at the next open. The window measures the menu by drawing it hidden and
  inert with the same recipe, places it with `placePopup`
  (`ui/src/lib/popup.ts`: dropped from its control, kept 8px inside the
  window), and sends `browser:layer` with the rectangle in CSS pixels, the
  rows as data (the window keeps what each does, by id) and the window's
  look (`lookOfDocument` in `theme.ts`: every custom property, the kind and
  the data attributes on `<html>`, painted again in the layer). Main converts
  the rectangle with the window's zoom factor, keeps it inside the window and
  below the title bar, draws the layer page at the same zoom, shows it and,
  for a menu, gives it the keyboard, so `Menu`'s own keys work. A pick,
  Escape or a click anywhere else (the page or Floti's chrome, which blurs the
  layer) closes it; main hides it, gives the keyboard back to the window if
  the layer had it, and tells the window on `browser:layer-event`, and the
  window returns focus to the control that opened the menu. A turn main
  closed takes no more content: until the window hears, it can still send
  that turn again (a pick that changes its own menu, as Open console, a zoom
  step or a width does), and that send must not show the layer again. For a blur, main
  reads where the keyboard went once Chromium has placed it: Floti's own
  window by a press on its page (`window`, a `before-mouse-event` press while
  the menu was open), or anywhere else (`blur`: the browser's page, another
  program, or the window by a way that sends its page no press, such as the
  title bar). The window can hear `window` before or after the press that
  caused it (measured: within 2ms), so it takes that press when it comes, and
  a press on the opening control is left to that control's click, which
  closes the menu. An overlay opening (a dialog Floti opens on its own, drawn
  in the window under the layer) closes a layered menu or the suggestions,
  and leaves the dialog its focus. A window resize or a pane move closes it
  from the window, and the Browser tab leaving the screen unmounts the menu,
  which closes it. These menus are no overlay: no still, nothing stands down.
  Dialogs and every other menu keep the still.
- The browser's keys are one table (`shared/browserKeys.ts`): main reads it in
  `before-input-event` for a key pressed in a page, the window for a key in
  Floti's chrome, and both run the window's one dispatch. A key an agent sends
  (`agentInput`) is always the page's.
- The console is closed until it is asked for: nothing is drawn under the page,
  no row and no count. The More menu's Console item and Ctrl+Shift+J
  (Cmd+Option+J on macOS) open and close it, as the `console` command in
  `shared/browserKeys.ts`; which pages have it open is held in `App.tsx`, so
  the menu and the key are one path. The artifact viewer's console is its own
  and unchanged.
- The mouse's back and forward buttons act on the page in front of the Browser
  tab, never on a page an agent drives in the background. On Windows and Linux
  the window gets an `app-command` and main sends it with where the pointer was
  (`browser:pointerCommand`); the window acts when the pointer is over the
  Browser tab (its view, strip or bar) and the tab is in front of any visible
  pane, focused or not (`useBrowserPointerCommands`). On macOS, which has no
  `app-command`, a DOM `mouseup` with button 3 or 4 on the tab's strip and bar
  does the same, and over a page nothing reads the buttons. A press is handled
  by one of the two paths, never both.
- The address bar shows `pendingUrl` while a load is under way, and `url`
  otherwise. `url` is the committed address (`getURL()`), and agent tools read
  only that. `load()` in `browser.ts`, which serves the bar and an agent's open
  and navigate, sets `pendingUrl` to the address it loads: a phrase's search
  results address, and after a redirect the address redirected to. Committing,
  failing, stopping or a load ending with nothing committed (a 204) clears it.
  After a failure the bar shows the failed address, which is where Chromium
  commits its error page. A main-frame navigation that has an `initiator` (a
  link, a form, a script setting `location`), or that goes to any other
  address, clears it and never sets it, so a page cannot put an address in
  the bar before it is there. From Enter until main answers, the bar keeps
  what was typed.
- The address bar suggests from the browser's history, the `browser_history`
  table in `desktop.db`: the newest 500 pages by last visit, with title and visit
  count. Main owns it (`openBrowserHistory`, read once into memory): `remember`
  records a visit when a page the user has commits (http or https, a real
  response, or a move within the page), and `page-title-updated` keeps its
  title. A page an agent opened is never recorded. `browser:suggest` ranks in
  main (`suggestBrowserAddresses` in `@helm/core`): the address exactly as
  typed, then an address starting with the text (after the scheme and
  `www.`), then a word start in the host or title, then a substring; ties by
  visits, then recency; at most 8 rows. With nothing typed it lists the pages
  visited last. When Enter would search, a "Search <engine> for" row comes
  first, after the top page when that page completes inline. The completion
  is selected after the caret; a deletion holds it back, so Backspace removes
  it, and Enter on it opens the page as visited. In the bar the arrows move
  the highlight (the row's address shows in the bar), Enter opens it, Escape
  first restores what was typed, and Shift+Delete forgets a page
  (`browser:forget`). The More menu's Clear browsing history empties it
  (`browser:clearHistory`). `BrowserSuggestions` draws the list from props
  alone, and is drawn in the menu layer.
- The suggestions in the menu layer never hold the keyboard: the bar keeps
  it and sends each change of rows or highlight, and the layer reports the
  pointer's pick (on the press) and hover by row. A press on the list still
  gives the layer focus, as Chromium focuses the view a press lands in; main
  sees the layer's `focus` and hands the keyboard straight back to the
  window, and the press's pick follows. Measured in the dev app, the bar then
  sees no blur at all, or a blur and a focus in the same millisecond. Focus
  moving within the window closes the list in the window; the window losing
  the keyboard is reported as `blurred`, and main closes the list
  (`closed: 'blur'`) unless the keyboard is back in the window by then. A
  person's mouse press in a page closes the list too (`before-mouse-event` on
  the page, `pagePressed`), whether or not the keyboard left the bar; an
  agent's click (`agentInput`) does not. The window keeps the same list
  where it hangs from the bar, transparent and taking no pointer: the layer's
  rectangle is measured from it, and it is the listbox the bar's
  `aria-controls` and `aria-activedescendant` point into, so assistive
  technology follows the keys' highlight by the option ids it always had.
  The list moves with the bar on a resize or a pane move rather than
  closing.
- Stop (`browser:stop`, `stop` in `browser.ts`) stops the load and any quiet
  reconnect. The reload button turns into Stop while a page loads. Escape
  stops a load from the bar when nothing is typed. In a page, main takes
  Escape only while the page is loading and is not fullscreen; otherwise it
  stays the page's. The window never takes Escape as a command
  (`COMMANDS_FOR_THE_PAGE_ONLY`).
- While the page in front loads (`BrowserState.loading`), a 2px indeterminate
  line runs under the address bar; Electron reports no load progress. A page's
  tab shows a spinner while it loads, then its site's icon, else the globe.
  The icon travels on its own `browser:favicon` event (`{ id, icon }`, a
  `data:` URL or null), never in `BrowserState`, and only when it changes:
  `page-favicon-updated` names candidates, `main/favicons.ts` fetches the first
  the tab's reach allows (see [SECURITY.md](SECURITY.md#browser-pane)) and
  caches it by icon address, and a navigation to another origin clears it. An
  icon fetched for a page that has since moved on is dropped. A renderer that
  reloads shows the globe until each page next reports its icon.
- Hide a view with `setVisible(false)`, which keeps the page capturable,
  scriptable and clickable.

## Overlays and templates

- The host composes overlays in each session's launch folder under its own
  data folder, never `%TEMP%`: they hold junctions into real repositories.
- Anything removing an overlay or a shim or walking a template unlinks links
  (junctions on Windows, symlinks on macOS and Linux) and never walks into
  one. `fs.rm` with `recursive: true` has been seen to go through a junction
  and empty the repository folder it points to.
- Launch folders are swept only at host start, and only where every owning
  process is provably dead. `EPERM` counts as alive; unknown means keep.
- Templates live in `templates` in the host's config folder: for the
  installed app `~/.config/floti`; otherwise the folder holding `templatesDir` in
  `paths.ts` (`floti-data/templates` when portable). The host reads and seeds them at
  its start. The shipped README is written only when the directory is absent,
  and nothing there is ever overwritten or removed. Starter
  (`workspace/templates/workspace.rs`, id `starter`) is the one template Floti
  ships: code, not a folder, listed first and chosen in the picker, writing
  `workspace.yaml`, an empty `repos/` and an `AGENTS.md`
  (`templates/starter/AGENTS.md`) with the workspace's name filled in. A
  manifest recording `template: minimal`, the id of the scaffold before it,
  lists, opens and launches as any other, and a request for `minimal` makes
  Starter. The in-app
  Templates manager makes templates (from a folder too, `template:fromFolder`),
  renames, imports, fills (`template:substitute`) and deletes them and edits
  their label and description; it has no file editor.
- A workspace is a folder with a `workspace.yaml`, or the `harness.yaml`
  workspaces had while they were called harnesses
  (`discovery/scan.rs`, `WORKSPACE_MANIFESTS`). Discovery, launches and
  overlays read either the same way, `workspace.yaml` first when both are
  there. `workspace:create` writes `workspace.yaml` itself, refuses a folder
  that has either, and a template cannot supply either; converting a folder
  writes that manifest and nothing else. Nothing renames or
  rewrites a `harness.yaml`.

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
  archive copies into the host's `floti.db`, and never writes there. Copying `desktop.db`'s archive into
  `floti.db` writes neither the agent's config nor `desktop.db`, and logs
  counts, never a message. What a launch needs (agent
  tools, settings, composed instructions, overlays) is a per-launch file under
  the data directory, never a write into the agent's config.
- A folder is written in Claude's shape (`CLAUDE.md`, `.claude/`) or the
  shared one (`AGENTS.md`, `.agents/skills`), or both, and a launch composes
  either (`agent/workspace.rs`). Everything that shows what a folder holds
  reads both: the scan's inventory (`workspace/discovery/inventory.rs`, its
  `agentsMd` and `agentsDir` since protocol 4.2), the project pane, the config
  scopes (a folder with either shape's files is a scope that exists) and their
  trees (a scope's `AGENTS.md`, kind `agents-md`, and its `.agents/skills`,
  which a template import copies to the same place), and the effective view,
  which lists an overlay's `.agents/skills` and every `AGENTS.md` a launch
  loads. A skill folder in both skills folders counts and lists once, from
  `.claude/skills`, as a launch takes it. None of them writes into the folder.

## Processes and ports

- The process-and-ports pass costs about half a second, so it runs only while
  the sessions pane is on screen or a `session_detail` call is waiting, every
  4s, in the host (`server/features/resources.rs`). Passes never overlap.

## Files view

- The Files view writes through `content:write`, and puts back a snapshot
  through `content:restore`. The host saves any file the view reads as text
  (the view offers Edit for a markdown note): snapshotted first, refused on a
  hash conflict, scoped to the deepest folder Floti knows that holds the file.
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
