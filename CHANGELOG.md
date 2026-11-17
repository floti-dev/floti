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
