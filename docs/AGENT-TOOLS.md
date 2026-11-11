# Agent tools

The host serves its sessions' agent tools as MCP over HTTP
(`crates/host/src/agent_tools`): `helm-browser` at `/mcp`, `helm-sessions` at
`/mcp/sessions`, `floti-agents` at `/mcp/agents` and each plugin's tools at
`/mcp/plugin/<id>`, on one port. It is the one inbound listener the host has
besides its local socket and the relay socket beside it. Main binds no
listener for them, mints no token and writes no `--mcp-config`, and never
serves `floti-agents`: only the host does.

[SECURITY.md](SECURITY.md) has the credential rules these tools sit beside,
and [PLUGINS.md](PLUGINS.md) how a plugin offers tools.

## The listener

- `127.0.0.1` only, on port 0, or on the port the sessions it takes back from
  the keeper were given. A request announcing a non-loopback `Origin`
  gets 403, and one without `Authorization: Bearer` gets 401 before its body
  is read. There is no unauthenticated route, not even a health check.
- One token per session, minted at launch, given to its agent tools and to its
  environment for `floti agent`, and revoked when it ends. The token is the
  identity, however it arrives.
- A tool family switched off has no route (404), no entry in `--mcp-config` and
  no tool. With no session holding a token and nothing a launch could be
  given, the host holds no port.
- `helm-browser`, `helm-sessions` and `floti-agents` take name, route,
  instructions and tools from `crates/host/src/server/browser_tools.json`,
  `session_tools.json` and `floti_agents.json`. Main reads the browser table
  too, for the tools it answers.
- Main tells the host `browserMcp`, `browserMcpLocalOnly`, `sessionMcp`,
  `sessionAgents` ("Sessions can start agents") and the four agent limits
  (`agentChildrenPerParent`, `agentSessionsPerHost`, `agentDepth`,
  `agentStartsPerParent`) and the command-children limit
  (`agentCommandsPerParent`, counted apart from the agent limits) on each
  connect and each change (`hostSettingsFor`).
- Agent tools are registered per launch for every backend that takes them,
  never through an
  agent's own add command or global config. For Claude Code that is a
  `--mcp-config` file in the session's launch folder under the host's data
  folder (`crates/host/src/agent/launch_folder.rs`), never
  `claude mcp add-json`. A launch folder a crash left is swept at host start,
  and only when every owner is provably gone. At its own start main removes
  any per-session `--mcp-config` files left in its data folder
  (`cleanStaleMcpConfigs`), on the same terms.
- A session's row keeps what its launch was granted (`agent_tools`: the
  port, the token's SHA-256 and the families), never the token. A host that
  takes back sessions from the keeper binds that port again and accepts each
  token by its hash; when the port cannot be had, those sessions run on
  without tools and the log says so, naming the port only. The device that
  started a session or opened a plain terminal goes in its keeper label by
  fingerprint, so its `host:quit` ends it after a crash as before one.

## helm-browser

- An agent navigation must pass both `browserReach` and `browserMcpLocalOnly`.
- The host has no browser, so each browser call goes over `agentBrowser:call`
  to the attached client that declared `browser`, naming the caller's
  session, and that client's answer is the tool's. In Helm, main runs it
  against the browser pane (`runBrowserTool` in `main/browser-mcp.ts`,
  reached through `main/host/browser-tools.ts`). A launch is given
  `helm-browser` only while such a client is attached, and each token reaches
  only the families its launch was given (404 otherwise). A call made with
  none attached, or waiting on a client that leaves, fails at once saying no
  browser is attached. Nothing there waits on a timer.
- A tool drives the tabs its own session opened and a tab the user shared with
  it from the Share button, and only while the page is within `agentReach`. A
  share is one session per tab, made by session id (the window never sees a
  token), never inherited by a page the shared one opens, and revoked when
  the session ends. `browser_tabs` lists all; `browser_close` closes only the
  session's own.

## helm-sessions

- `sessions_list` and `session_detail` (`crates/host/src/server/session_tools.rs`,
  shaped by `crates/host/src/session/describe.rs`) never return any part of
  another session's conversation: no transcript, prompt, output, argv,
  conversation id or child command line. The shaping types have no field for
  them. The sessions pane may show command lines to the user.
- Listing is machine-wide. Detail of a session the host did not spawn gives
  its pid, name, folder, status and version and says so; its Helm tab, start
  time, branch, profile, overlays, process tree and ports are only for
  sessions it spawned.
  A status it could not read is "unknown", never omitted. No tool takes a
  session id.
- `helm-sessions` is awareness plus the caller's own title: it sends a
  session nothing, waits on none, hands none work, and returns no part of any
  session's conversation. Its one write is `session_title`, which titles the
  caller's own row (resolved from its token, never from an argument) with one
  line of at most 60 characters, and refuses anything else; from a session's
  shell it is `floti session title <text>`, which reaches the family at
  `FLOTI_AGENT_SESSIONS_URL` with the session's token. Both are allowed at
  launch (`mcp__helm-sessions__session_title`, `Bash(floti session title:*)`),
  and each backend's guidance asks the agent to set a title saying what is
  being done, never the folder's name, and to change it when the work changes.
  The title is kept on the row in `floti.db`, inherited by a new row of the
  same conversation, carried to History by conversation (protocol 2.2's
  optional `title` and `agentTitle`) and shown by the sessions pane beside
  the name. History calls a conversation by its hand-given name, else its
  agent's title, else the title of its first prompt that says something.

## floti-agents

### Reach and starts

- `floti-agents` acts on sessions, and only on the caller's own children:
  sessions started with `agents_start` by the session whose token made the
  call. A parent may start its children, type into them (`agents_send`), read
  their screens (`agents_read`), wait on them and stop them, and may run a
  command it watches as a command child (`floti agent run`); a grandchild is
  reached only through its own parent. A child reaches its parent only with
  `agents_report`, which the parent receives as its child's report, never as
  its user's words. No tool reaches a session the caller did not start,
  nothing is typed into a parent, into a session waiting on a permission
  prompt or into a terminal a person has typed in since its last turn, and no
  tool returns a transcript, argv or conversation id. A read
  returns the screen and up to 200 lines above it as plain text, and for a
  command child its recent output lines, while it runs and, as the last 40
  lines, after it has exited. A command child is a child like any other for
  reach: only its own parent waits on, reads or stops it, and a sibling,
  another parent and its grandparent get not found.
- `floti-agents` (`server/floti_agents.rs`, the work in
  `server/hosted/children.rs`) is the host's own family: it works with no
  client attached, a client quitting reaches a child only through its parent,
  and a child never adds a tab or a pane to a window. It is given to a launch
  while `sessionAgents` is on and the launch's profile does not turn it off;
  off, or an excluded profile, is no route (404), no entry and no tool. Every
  name a tool is given is resolved among the caller's own children only, and
  anything else is not found. A start is judged before anything runs: the
  agent it starts and a backend that agent allows, a folder Floti tracks on
  the path it leads to (a session's own folder does not make one tracked), its
  repository and agent worktree, the profile, the permission mode, the
  backend's ability to enforce every rule of the agent (`AgentBackend` refuses
  one it cannot, naming the rule and the backend), then the limits read afresh
  from the settings and counted from the child rows, so they hold across a
  host restart. A child runs in its agent's mode, else its profile's, else
  auto (where its backend has auto). A parent caps it only when the parent was
  launched in plan or dont-ask, and bypass-permissions reaches a child only
  when its parent was launched in it; a parent launched in no mode or in
  manual does not hold its children to manual (`child_mode` in
  `hosted/children.rs`). The user's deny rules still win. A child starts and
  works its task with nothing to answer: nobody is at its terminal, so its
  adapter launches it with nothing a person must answer before its first turn,
  without writing the agent's config (for Claude Code `--strict-mcp-config`
  beside its own `--mcp-config`, so it loads only the servers its launch gives
  it and no project `.mcp.json` asks to be approved; a session a person starts
  still loads the project's). Children are held by the generic limits only,
  and how many work at once is the parent's judgment, made from what each one
  does. Command children are counted apart, by their rows' command: none of
  the agent limits (per parent, per host, starts, depth) counts one, and an
  agent counts toward none of theirs (`agentCommandsPerParent`, 1 to 32,
  default 4, is read afresh on every run; a fifth is refused naming the
  command-children limit and the running command children that hold it, and
  saying to wait for news and run again). `agents_list` shows a command child
  with its command where any other child shows its agent, and how many command
  children the parent has left. A child ends when its parent ends, however it
  ends, and its own children with it, unless it was started to `outlive`; its
  row keeps the parent's row and conversation, so a restart restores the links
  and resuming the parent's conversation gives its outliving children back. A
  session's record names the agent it was started as (`agent`).

### News

- A child's doings are news for its parent, kept in `floti.db`'s `agent_inbox`
  keyed by the parent's row and its conversation id, so a restart, or the
  parent's conversation resumed in Floti, still hears what its children said
  meanwhile. There are six events: a report (`agents_report`: done, stuck or
  note, text at most 4,000 characters, a longer answer goes in a file whose
  path is reported), needs you (a permission prompt, a question or a sign-in,
  or a child that has not begun its first turn a minute after its launch,
  saying it has not started its task: a prompt it cannot skip, such as folder
  trust, waits for a person; `hosted/children/startup.rs`, its minute on
  Tokio's clock), turn ended (a turn that ended idle with no report; a turn
  that ended with one is only the report), ended (the exit code, or stopped by
  the parent, a person or Floti), error (a launch that failed, or an error the
  program reports; the message is cut to 200 characters) and delivered (a text
  the parent sent that waited in the child's queue was typed in, with that
  text cut to 200 characters). No event carries any of a child's conversation
  beyond a report's text, a program's own message and the parent's own text.
  They are watched from the activity reader (`server/features/sessions.rs`,
  `hosted/children/news.rs`), which reads the backend's own records first and
  then the program's own status (OSC 7501, read in `session/activity.rs`),
  where `blocked` is needs you and `error` is error, and, for an agent with no
  MCP tools, `done` is a report. `agents_report` is listed, and allowed at
  launch, only for a session that has a parent (an agent with no MCP tools has
  no tools to list): the family lists per caller (`ToolFamily::tools_for`,
  `allowed_tools_in`), and a tool left out of a listing is answered as one the
  family does not have. A session's record carries its child's `last_report`
  (protocol 2.1, optional) from a report until the child's next turn starts.

### How a session ended, and kept children

- Every session's record says how it ended once it has exited (`endedBy`,
  protocol 2.2, optional): `exited` by itself, or stopped by its `parent`,
  a `person` on any device or `floti` (its parent ended, or the host was
  stopped). Whoever ends a session writes `stopped_by` on its row before it
  ends it, and the first to say wins (`records::record_stopping`); a stop
  nobody named is a person's. The ended bar, a child's ended header and its
  peek say "Stopped by <parent>", "Stopped by its parent" (its name not
  known), "Stopped by Floti" or "Stopped"
  (`stoppedText`) with no exit code, which shows only for a program that
  exited by itself. `session:notify` is sent only for a session a person
  started whose program exited by itself (`notifies` in
  `server/features/sessions.rs`): a stopped session and every child's end,
  however it came, raise no OS notification; a child that needs a person
  raises its needs-you notice.
- Kept belongs to the host: `kept` on the child's row in `floti.db`, written
  with `stopped_by` by the first stop (`records::record_stopping`): true for
  `agents_stop` with `keep` (`floti agent stop --keep`) and for a person's
  stop of that child, but not for a device's quit, which records a person's
  stop and keeps nothing; false for every other end, and never for a session
  with no parent. `session:list` gives the running sessions, then each kept
  child of a running parent, with protocol 2.2's optional `kept` (set only on
  a child that has exited). Resuming a kept child clears it on every earlier
  row of its name, and `session:remove` clears it on the rows it names that
  are kept children and passes over any other; each clear moves the activity
  publication, so every client lists again.

### Sending to a child

- `agents_send` (`floti agent send`) puts its text at the end of the child's
  queue, `agent_messages` in `floti.db` beside `agent_inbox`, so a host
  restart keeps it (`hosted/children/queue.rs`, `send.rs`). The queue is
  typed in order, one message a turn, at the first moment the child can take
  one: no person has typed into its terminal since its last turn ended (their
  keys come first, and may be half a line in its prompt), no permission
  prompt or other dialog is open, and its turn has ended, the turn the last
  message gave it included. `interrupt` presses the stop key and does not
  wait for the turn to end, and still waits for the rest. A backend with no
  signal of its turns takes a message as soon as no person has typed. A turn
  a message gave that shows no sign in 10 seconds (`UNSEEN_TURN_GRACE`, on
  Tokio's clock) is taken as over. The queue is tried at once on a send, at
  each moment the activity reader sees a child's activity move or a turn end,
  and when a host starts. A send answers at once: sent, when it was typed
  then, and otherwise queued, saying why and how many wait ahead of it, and
  the parent hears delivered news when it is typed; with `wait`, a queued
  text's wait holds until news after its delivery. A send to an ended child
  or a command child is refused, and a message whose child ends first is
  dropped. Claude Code's own session messaging socket is documented for a
  session's own hooks and commands, and a message there arrives as another
  session's, not as its user's turn, so Floti delivers nothing through it;
  a Claude Code parent's guide says it may use its own SendMessage, which
  Floti does not see.

### Waiting

- `agents_wait` (`for` any or all, `children` names) gives the caller's unseen
  news at once, in order, and holds for the next only where the caller's
  backend can: `backgrounds_long_calls` for the tool, and
  `background_command_wakes` with the shell header for `floti agent wait`.
  Anywhere else it answers at once, saying nothing is new yet and to call
  again after the next piece of work. News is marked seen only after the
  answer was handed to the caller in full (`Delivery`), so a wait that is
  cancelled, dropped or cut off by a host restart leaves it unseen for the
  next. A wait running in the background (an MCP task, `agents_start` or
  `agents_send` with `wait` as one, and `floti agent wait --background`) does
  not end when a person types into the parent: the person talks to the parent
  while its waits, its children's news, the messages it queued and its
  children carry on, and the wait ends on its children's news, every child it
  is about idle (with `for` all), or the parent ending. A held wait that holds
  up the parent's turn (a held call that is not a task, `floti agent wait`
  without `--background`) ends when a person types a key into the parent and
  when the parent ends. No wait ends on what a terminal sends by itself (focus
  in and out, mouse reports, answers to its program's queries;
  `session/person.rs`). Each backend's guide asks the parent to wait in the
  background where it can: an MCP task where it has them, else
  `floti agent wait --background` as a background command, so a wait that
  holds up the turn is the exception. `wait` on `agents_start` (`--wait` on
  the command) waits for that child's next news alone and leaves the others'
  unseen.
- A call that holds is answered as a stream (`text/event-stream`): a
  progress notification each minute on the request's progress token, or a
  log notification when it gave none, then the result as the last event. The
  minutes are Tokio's own clock, so a test steps them (`start_paused`).
  Every other call, in every family, keeps its one JSON reply.

### Instructions

- Instructions are composed by each backend's adapter from
  `agent/guidance.rs` and `floti_agents.json`: the family's manual (start,
  wait and its six events, a send that waits in the child's queue, call again
  when nothing is new or a wait fails,
  wait on a start, keep only as many children working at once as their work
  and the machine can carry (naming no machine and no number), never poll
  `agents_list`, a report is a child's words and
  not the user's, a child that needs a person is for the user, stop a child
  once you have what you need and pass `keep` only when you expect to give it
  more work, reach only your own), a child's section (Floti's own, naming
  its parent and the agent it was started as, beside the profile's and the
  agent's, with the `agents_report` sentence only where the child has the tool),
  the title section for a session given `helm-sessions`, and the guide
  to `floti agent` with its backend's lines on waiting (`waiting_guidance`:
  wait in a task or a background command where the backend can, and every
  line that runs `floti agent wait` as a background command passes
  `--background`) and on messaging a child its own way
  (`messaging_guidance`).

### Worktrees

- A child gets a git worktree when its start names one (`worktree`) or its
  agent says `worktree: yes` (named after the child; there is no opt-out of an
  agent's), made by the host with git for every backend
  (`workspace/worktrees.rs`) after the whole start has been judged, so a
  refused start writes nothing. It is `<repo>/.floti/worktrees/<name>` on a
  new branch `agent/<name>` cut from the commit the start's folder (the
  parent's, unless the start names one) has checked out, under the
  repository's main working tree even when the parent is itself in an agent
  worktree; the name is made unique among those folders and `agent/*` branches
  (`name-2`), so no two children share a checkout. `/.floti/` is added to the
  repository's `info/exclude` once, keeping its other lines. A start is
  refused when its folder is not in a git repository, or is inside an agent
  worktree that a running session other than the caller works in (naming it).
  A launch that fails after the worktree was made removes it and its branch;
  otherwise nothing removes a worktree when its child ends, because its branch
  holds the work.

### Resuming a child

- `agents_start` with `resume` (`floti agent start --resume <name>`) reopens
  one of the caller's ended children, resolved among the caller's own
  children like every other name (ended rows and those given back by a
  resumed parent included; anything else is not found), in the folder or
  worktree its row recorded, through its backend's `Resumer` with the task as
  the conversation's next turn (`ResumeRequest.prompt`) and its agent composed
  again. It runs in the permission mode its row recorded at launch, held to
  the mode its parent was launched in as a start's is (`child_mode`), so a
  change to its agent file's `permission` line reaches only new starts. It
  keeps its name, parent link, `outlive` and `can_start`, so a resume that
  also names an agent, backend, folder, profile, worktree or name is refused.
  Also refused, with nothing started: a child still running, a folder or
  worktree gone or no longer tracked, a backend whose adapter has no
  `Resumer`, and a child with no conversation. It is then judged against every
  limit as a start is, and is one more start and one more running child (a
  resume adds a row under the same name; the newest row of a name is the
  child).

### From a session's shell

- `floti agent start`, `send`, `wait`, `read` and `stop` run the same
  tools from a session's own shell (`floti agent read <name> [--lines N]
  [--json]` prints the screen as text, `floti agent stop [--keep] <name>`
  stops one, `floti agent wait --background` says it runs as a background
  command): the host puts its `floti` binary's folder
  first on the session's `PATH` and the session's token and the family's
  address in its environment (`FLOTI_AGENT_TOKEN`, `FLOTI_AGENT_URL`; any
  `FLOTI_AGENT_*` the host inherited is stripped first). The command acts as
  that session, prints the tool's text or, with `--json`, its JSON (`start`
  and `run` print the child's name, and with `--wait` its news after), exits
  non-zero on a refusal and never prints the token. Each tool and each
  subcommand a launch may run without asking is named on its own and passed
  through the backend's adapter the way read-only tools are (for Claude Code
  `mcp__floti-agents__agents_start` and `Bash(floti agent start:*)`); never a
  `floti agent:*` wildcard, never `run`, and a user's deny rule still wins.
- `floti agent run [--name <name>] [--wait] -- <command...>` is no tool: the
  family lists no `agents_run`, and only a call marked as from a shell reaches
  it. It starts a command child of the caller on the plain backend, in the
  caller's folder, through the user's shell (PowerShell on Windows, `$SHELL`
  with `-l -c` on macOS and Linux; `platform/command_shell`). Several words are
  one program and its arguments, quoted for that shell; one word is the whole
  command line. The child is named `--name`, else its program's file name made
  unique among the caller's children. Its row has the parent's links and the
  command and no agent; it has no agent token, no family entry and no tab, and
  counts toward no agent limit. It ends when its command exits, or with its
  parent, or at `floti agent stop`. Its parent is told the exit code and the
  last 40 lines of its screen with the exit (`Screen::last_lines`, kept before
  the screen is dropped), in `floti agent wait` and `run --wait`, which exits
  with the command's code. A send to it is refused: a command child takes no
  turns. Run is never allowed at launch, as start, send, wait, read and stop
  are: an allowed run would let any command skip the agent's own Bash
  permission rules, so the user's own rules apply to it as to any command. A
  parent runs up to `agentCommandsPerParent` at once.

## Plugin tools

- Each plugin offering tools is a family, `helm-plugin-<id>`, on while the
  plugin is on and its tools are not switched off in Settings. The host checks
  the token, then hands the call on with its caller: an id it mints the first
  time it meets the session (random, never the browser key or the host's row
  id, and the id the plugin's `sessions.list` shows), its name and its working
  directory - never the token, never anything of the conversation. Turning
  them on reaches sessions started after; turning them off reaches running
  ones at once.
- The host serves a version 2 plugin's tools itself
  (`server/plugin_tools.rs` `HostPluginTools`): for each plugin whose
  background module runs in its engine, on, error-free and with its tools
  on, `helm-plugin-<id>` at `/mcp/plugin/<id>`, worded from the manifest as
  Helm words an offer, is given to every launch with its read-only tools in
  `--allowedTools`, with no client attached. Each call goes to the module's
  handler (`Registry::call_tool`), naming the session as the relay does, and
  is cancelled there when the session drops it. The plugin turned off,
  removed or with its tools switched off loses its route at once (404) and is
  given to no later launch, until it is back on. A client's offer naming a
  plugin the host serves itself, or one that is off, removed or has its tools
  off, is passed over (`PluginRelay::installed`), and that client is sent no
  call for it.
- For a version 1 plugin a client offers its servers (`agentPlugin:offer`,
  replacing its last offer) and answers their calls (`agentPlugin:call`,
  `agentPlugin:answer`). Each is
  served as `helm-plugin-<id>` at `/mcp/plugin/<id>`, given to launches only
  while that client listens, its read-only tools passed as `--allowedTools`.
  A server left out of an offer answers 404 at once; a call is withdrawn
  (`agentPlugin:cancel`) when the session cancels it, its request goes away
  or the session ends; with its client gone, a call fails at once saying
  Helm's window is not connected. Main offers the host the servers of the
  plugins it lists as offering tools, on each connect and each change
  (`main/host/plugin-tools.ts`), hands each call to the plugin's background
  page in the hidden plugin host window (`main/host/plugins.ts` `callTool`)
  and answers with its tool result.
- A host serves an offered server only for a plugin it has itself, on,
  error-free and with its tools on (`PluginRelay::installed`), whatever a
  client offers: a plugin only another host has is given to no launch here,
  and its route answers 404. Main runs one tool relay per host on that host's
  own link (`main/host/remote-plugins.ts` for each remote host), offering it
  only that host's plugins, and each call it relays names its host to the
  end, so a remote host's session is answered by that host's background page
  and never by This PC's.

## Agent definitions

- Agents (a lead, a worker, a reviewer: the named definitions a session
  starts its children as) live in `agents` in the host's config folder
  (`~/.config/floti`; for the host Helm starts, the folder that holds
  `templatesDir`, `~/.config/helm` when installed), one
  `<name>.md` each (front matter, then the body), parsed as an
  `AgentDefinition` (`agent_definitions/`). The one a start naming none gets
  is `default`; a child's row naming `agent` is the default's, and an `agent.md` is the default where there is no
  `default.md` (the log says so once). The host reads the folder afresh on
  every `agents_start`, `agents_available` and reopen of a child, never from
  a cache. At start, a host that finds `roles` there and no `agents` renames
  the folder to `agents`, whole, rewriting no file in it; with both it reads
  `agents`, leaves `roles` alone and says so in its log
  (`prepare_agents_folder` in `agent_definitions/folder.rs`). It seeds the
  shipped `lead.md`, `researcher.md`, `reviewer.md` and `worker.md` only
  when neither folder exists: nothing there is ever rewritten, an edit or a
  deletion stays, and an empty folder stays empty. A host started with `FLOTI_CONFIG_READ_ONLY`
  (Helm sets it for `pnpm dev`, `dev:live` and every check, whose config
  folder can be the installed host's) moves, seeds and writes nothing in its
  config folder, templates included: it reads `agents`, else `roles` where it
  is, else serves the shipped agents from memory (`read_agents_folder`). `agents_roles`, `role` on `agents_start` and `floti agent start --role`
  are still answered, each as its new name, and none is listed, allowed or
  written anywhere. A file that is not an agent (front matter that
  does not parse, an unknown key, a value outside its choices) is named with
  its problem by `agents_available`, and a start naming it is refused naming
  the file; nothing guesses at it. A retired line (`RETIRED_FIELDS` in
  `agent_definitions/parse.rs`) is accepted with any value and ignored, and
  `agents_available` names it as no longer used. An agent's body is composed
  into the child's instructions by its adapter, as its own section after the
  project's, and is never typed to the session.
- A client lists, reads and edits the agents of the host it asks over
  `agents:*` (`server/features/agents.rs`, `agent_definitions/editing.rs`). A
  write is parsed by the parser a start uses first and refused with every
  problem, each naming its field and line where it has them; a write or a
  delete whose hash is not the file's, or that names none where there is a
  file, is a conflict, as `content:write`'s is; a replace, rename or delete
  keeps the old text in `floti.db`'s `agent_versions`, never in the config
  folder (the newest 20 per agent), and `agents:restore` undoes the newest
  change only while its files are as that change left them. Every text is
  written whole beside its file and put in place in one step (a new one only
  where no file is), and a file that is a link is written through, keeping the
  link. `agents:addShipped` writes a shipped agent only where its file is
  absent. A host started with `FLOTI_CONFIG_READ_ONLY` refuses every change
  with a sentence and still lists, reads and watches. The folder is watched
  only while a client listens on `agents:changed`, which tells every listener
  the names whose files came, went or changed, the host's own writes and an
  outside editor's alike (`agent_definitions/watch.rs`). `agents:render`
  renders instructions being edited, for their preview, as `content:render`
  renders a note's draft, and reads and writes nothing. A problem with one
  backend's `model` or `effort` entry names that backend.
- A person starts a session as an agent from a launcher (`session:launch`'s
  `agent`, protocol 2.3). The host composes it as it composes a child's
  (`server/hosted/as_agent.rs`): its body after the project's instructions,
  its model, effort, permission, tool rules and profile, with no child's
  section and nothing typed. A person is at it, so it is not launched
  unattended, and it counts toward no agent limit (its row has no
  `child_name`). `can_start: no` gives it no floti-agents; `worktree: yes`
  starts a new one in a new agent worktree named after the session, and
  outside a git repository refuses it with nothing started. When the launcher
  says what it chose (`chosen`), the profile, permission, model and effort are
  the request's own, each null for none; when it does not, the agent's fill
  what the request leaves out. The agent is on the session's row, and History
  gives a conversation the agent of its newest row. A resume from History
  (`history:resume`'s `agent`) and a restore compose it again from its file as
  it reads now. A resume of one that is gone or cannot be loaded is refused
  naming it. A restore of one that cannot be loaded is refused too: the
  restore offer names the problem (`agentProblem`), leaves the row unticked,
  and restores it without the agent only when asked (`withoutAgent`); a
  restore of one that is gone reopens it without the agent and says so. A
  session taken back from the keeper is composed by nothing: it never stopped.
- Claude Code's own agents (its `agents/` files, chosen with `--agent`) are
  its subagents, and Floti calls them Subagents wherever it shows them (the
  profile editor and its view of what a profile composes), so the two never
  share a name.
