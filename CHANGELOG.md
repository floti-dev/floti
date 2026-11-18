# Changelog

What each release changed **for somebody using Floti**. One section per version,
newest first, and `.github/workflows/release.yml` publishes the section matching
the version being released as the release body.

Not a commit log. The commits are one click away under "Full changelog" on every
release page, and they include the checks, the fixtures and the refactors - all
of which matter to whoever works on Floti and none of which a person downloading
an exe has any use for. Deriving this from `git log` was tried through 0.4.0 and
produced fifty-nine lines, most of them naming probe ids.

A version with no section here does not release: the workflow fails rather than
publishing an empty body, because the step a person can skip is the step that
gets skipped.

## 3.1.0

**Sessions and Floti's window**

- Sessions can open Floti's views for you: a file at a line, a folder in
  Files with its changed files marked, a project's page, History with a
  search typed in, or a section of Settings. What a session opens shows
  beside its tab as a preview, and the keyboard stays where you were typing.
  From a session's shell it is `floti desktop open`.
- Sessions can read Floti's settings and change them by name, also as
  `floti desktop settings` and `floti desktop set` from the shell. Every
  change shows a toast naming the session, with Undo. A session can only
  narrow what agents may reach or do and what Floti does on its own; widening
  those, and choosing the Claude CLI, the shell or how much the archive
  keeps, stay yours to change in Settings.
- When something isn't working, a session can ask Floti how it stands (the
  versions, each agent CLI and its sign-in, the hosts and how each is
  connected, plugins and their errors) and read the latest lines of Floti's
  logs, also as `floti desktop status` and `floti desktop log`. Anything
  shaped like a secret is hidden in both.
- "Sessions can use Floti's window", in Settings > Floti host > Tools and on
  by default, turns all of this off for a host's sessions.

**Settings and hosts**

- Settings that decide what a host's sessions run or may do are kept by that
  host: every window attached to it sees the same values, and they hold with
  no window open. A session changing one changes it on its own host unless
  it names another. The values this computer had move to its host the first
  time 3.1.0 starts.
- Settings lists a Floti desktop group for this device and a Floti host group
  for the host you pick, with new Programs and Tools sections. With several
  hosts or long names, the host switch is a menu showing each host's state.
  A host out of reach shows Unknown, and Secrets and Plugins follow the host
  you picked.
- A plain terminal opens in the shell its host's settings choose.

**Security**

- Electron moves to 43.8.10, and Floti's dependencies to their patched
  releases.

## 3.0.0

The first stable release of Floti 3: everything in 3.0.0-preview.1, plus what
follows.

**Getting started**

- A guided first run sets Floti up in five steps: a welcome, the host on this
  computer, your agents, a workspace to work in, and a short tour of the
  window. The agents step shows whether Claude Code is installed and signed
  in, with the line to install it, and Locate… for a copy Floti did not find.
  It ends on the home screen with your workspace selected. A first run you
  leave picks up at the same step, and Settings > Walkthrough shows it again.
- Starter is the one workspace template Floti ships: a new workspace gets
  `workspace.yaml`, an empty `repos/` and an `AGENTS.md` with its name. Your
  own templates are listed after it.
- Every empty page and list says what it is for and offers its next step,
  such as New session, New profile or New agent. Where Floti could not look,
  it says Unknown and why, with Try again, instead of saying there is nothing.

**Workspaces**

- A folder written in the shared shape, with `AGENTS.md` and `.agents/`, shows
  as configured: the project page names them, a session's effective config
  lists `.agents/skills` and every `AGENTS.md` it loads, and a template copies
  them where that shape keeps them. A `.mcp.json` counts on its own.

**Start-up and the host**

- From the first frame, start-up shows what Floti is doing while it connects
  to this computer's host, and what a step is waiting on when it takes a
  while.
- When this computer's host is older than Floti, Floti restarts it onto the
  newer floti by itself, and the sessions carry over. The status bar and the
  sidebar say the host is updating meanwhile.
- A host that can't be used is said once, in one notice with its fix as a
  button: Try again, or Update Floti for a host newer than this Floti. Lists
  wait for it, and an action you start meanwhile says at once why it can't
  run. A pinned project is no longer marked gone before its folders are read.
- The Claude Code banner shows only when the host says Claude Code is
  missing, not when the check couldn't run.
- `floti update` with nothing published yet says so instead of failing.

**Remote hosts**

- Add a host is shorter on every screen. An install that fails because no
  floti is published for that version says so, in the dialog and in
  Settings > Hosts.

**Browser**

- The address bar suggests pages from your browsing history as you type,
  completing the top match. Shift+Delete forgets a page, and the More menu
  clears the history. Pages an agent opens are not recorded.
- A thin line under the address bar shows a page loading, and each tab shows
  a spinner and then its site's icon.
- The address bar keeps the address being loaded until the page opens, fails
  or is stopped. Reload turns into Stop while a page loads, and Escape stops
  it too.
- Sign-ins survive a restart, and Google's sign-in works in the browser.
- The More and Share menus and the address suggestions open over the live
  page without freezing it, and close when you click anywhere else.
- The console stays hidden until you open it from More or with Ctrl+Shift+J
  (Cmd+Option+J), and counts warnings as warnings.
- The mouse's back and forward buttons go back and forward.

**Plugins**

- Plugins use API version 2 only, and a version 1 plugin is refused with a
  message saying so. The manifest is `floti-plugin.json`, and the plugin SDK
  2.0.0 uses Floti's names throughout; its README lists what changed.

**Look and license**

- The longship is Floti's app icon and in-app mark. On Windows `floti.exe`
  shows the icon too, and on Linux a newer floti updates the menu icon an
  older one installed.
- Floti is open source under the Apache 2.0 license, and every download
  includes the third-party notices for the code it ships.

## 3.0.0-preview.1

A preview, not a release: nothing offers it as an update.

**Workspaces**

- Harnesses are now called workspaces, everywhere Floti names them: the
  launcher, Setup, Settings, the template picker, the welcome pane and the
  docs. A new workspace is written with `workspace.yaml`. A folder with
  `harness.yaml` stays a workspace and works exactly as before, and Floti
  renames or rewrites nothing in it; where a folder has both, `workspace.yaml`
  is read. Floti says all this once, the first time it opens after the
  update.

**Pages**

- Config is removed: its page, its rail entry and its tab. Claude Code edits
  its own config - `/config`, `/mcp`, `/hooks` and `/agents`, or ask the
  session - and Floti now writes nothing into an agent's config folder: Files
  shows a file in `~/.claude` (or `CLAUDE_CONFIG_DIR`) read-only, with no
  Edit, and says who edits it. A saved layout that had a Config tab opens
  without it, and a rail that hid Config forgets it. The project page links
  to Files only.
- Profiles, overlays and templates compose sessions as before. The profile
  editor still shows the subagents and MCP servers a profile's folders give a
  session, and a template still copies skills, commands, agents and settings
  in from any `.claude` folder.
- Agents is a page of its own, after Profiles in the rail: each host's
  agents, the files in its agents folder that are not agents with what is
  wrong with them, and the shipped agents the folder lacks, each with Add.
  Open an agent to edit its fields and instructions, or its whole file in
  Source; save, rename, copy or delete it, and Undo a
  delete. A save the host refuses keeps the edit and says what is wrong, and
  a file changed outside while you edit is shown beside your edit to compare,
  reload or save over. Settings > Agents says how many agents the host has.
- History is a palette on Ctrl+H, over whatever is on screen (the command
  palette lists it too), and no longer a page on the rail. With nothing typed
  it lists the ten sessions you worked in last, each by its title, with its
  folder, when, whether it runs now, its agent and how many sessions it
  started. Type to search names, titles, prompts and archived conversations
  at once, in two counted lists with Tab between them; a folder's name in the
  search narrows to that folder. Enter resumes where it ran, or shows a
  session that runs now; Ctrl+Enter opens the whole conversation read-only in
  a tab at the line that matched, with Find, Rename, Show in Explorer and
  Resume. → unfolds a lead's children with how each ended, and F2 renames in
  place. After a crash, the restore offer's Not now keeps its sessions at the
  top of History until each is back, with Dismiss and Undo. Grouping by
  project and Resumable only are gone, and a saved layout with a History tab
  opens without it.

**Agents**

- The shipped lead, worker and reviewer get work done in fewer rounds. The
  lead sizes each piece to merge on its own, batches small fixes into one
  worker, orders the pieces by what depends on what, points its briefs at the
  project's own rules instead of retyping them, keeps waiting on its children
  while any runs, and gives each change one review whose findings go back to
  the worker that built it. The worker runs only the checks for what it
  changed and no background command that outlives a step. The reviewer
  reports everything in one review, most severe first, saying whether a user
  would hit each finding. An agents folder that already has these agents
  keeps its own; delete one on the Agents page and Add brings in the new one.

**Sessions and notices**

- A session that has ended leaves the sidebar, and its parent's tab, on
  every computer you use Floti on, a child its parent kept included. If it
  is on screen when it ends, it stays until you move away from it. Remove and
  Remove stopped children are gone with the rows they served, and History
  still lists every session. A session that ended while a computer was away
  no longer comes back there as lost with a restore offer.
- While you were away lists only the notices this computer has not shown
  yet, so opening a laptop no longer brings back the same notices every time.
  A test or build an agent runs that fails no longer raises a notice: the
  agent that ran it hears it. A notice says what happened - a child ended in
  error, couldn't start, or reported an error while it kept working - and a
  reported error clears once the child goes on working or is resumed.

**Windows**

- On Windows nothing starts the host at sign-in or at install, and no window
  ever opens for it: the host starts when Floti or a connection needs it,
  with no console, so there is no window to close by mistake. `floti setup`
  registers nothing there and says the host starts when Floti or a
  connection needs it. The logon task "Floti host" an earlier preview
  registered, which opened a console window at every sign-in, is deleted
  when Floti installs or updates, when its host starts and by
  `floti uninstall`; the host it started keeps running until its next
  restart. A task that cannot be deleted is a warning, never a failed
  install. macOS and Linux still start
  the host at sign-in.
- On Windows, the sessions running when you sign out, shut down or restart
  are offered for restore the next time Floti opens. While the host stops,
  Windows lists Floti as "Floti is saving its sessions", for at most 20
  seconds.

**Remote hosts**

- Floti and its host speak Floti protocol 4.1. A desktop or host on an
  earlier release is refused at connect with a sentence saying which side to
  update.
- Each host in the sidebar says where it stands: connecting, loading its
  sessions, reconnecting with its last rows dimmed, or can't be reached, with
  the reason and Try again, and Unknown when its sessions couldn't be listed.
  The "No workspaces" message shows only once a host has listed its sessions
  and has none.
- When your computer wakes or unlocks, Floti checks each host on another
  computer at once and reconnects one that doesn't answer within 3 seconds,
  instead of showing its sessions as live until ssh gives up.
- Adding a Windows PC as a host from another computer works with nobody
  signed in there: `ssh <target> floti connect` starts the host on that PC
  when none is running, and closing the connection leaves the host and its
  sessions running. Floti opened at that PC's desktop later connects to the
  same host. From an administrator's SSH login while you are signed in at
  that PC, it starts nothing and says to open Floti on that PC instead.

**Install and update**

- Installing Floti on Windows puts floti where the install script does,
  `%LOCALAPPDATA%\Programs\Floti\bin`, and adds that folder to your PATH, so
  another computer that reaches this one over ssh finds `floti`. On macOS and
  Linux the desktop puts floti in `~/.local/bin` and sets the host up the
  first time it starts, and again whenever it carries a newer floti. The
  desktop runs that floti from then on.
- On macOS and Linux, installing floti now puts it on your PATH: when
  `~/.local/bin` is not on it already, one line marked `# added by floti`
  goes at the end of the startup file your shell reads (`~/.zshenv` for zsh,
  bash's login file or `~/.profile`, fish's `config.fish`), and the install
  says which file it changed. On a Linux desktop a bash or sh terminal picks
  it up after you next sign in. For a shell Floti does not know, the install
  changes no file and says so. `floti uninstall` takes that line out again
  and leaves every line you wrote. On Windows, a host the desktop installs
  over ssh now gets floti's folder on its Path too.
- Installing an update no longer stops the host or ends a session: the host
  keeps running until you choose Restart host to update, which hands every
  session to the new host.
- The portable exe installs nothing and runs the floti inside it.
- Linux: the browser pane's AppArmor profile is `floti-appimage`, for
  `Floti-*.AppImage`; run the command the app shows once.
