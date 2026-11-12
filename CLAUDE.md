# Helm - Agent Instructions

Helm is a Windows desktop shell for the Claude Code CLI. It hosts `claude` in
terminal tabs and adds what the CLI does not have around it: project discovery,
profiles, session history, a Files view that renders notes and artifacts, and
a browser pane. Electron and React. It shells out to the
`claude` CLI and never reimplements it.

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) - what each process owns, how
  sessions live and end, tabs and families, the browser pane, overlays,
  templates, the Files view. Read the part you are changing first.
- [docs/AGENT-TOOLS.md](docs/AGENT-TOOLS.md) - the host's agent tools listener
  and its families: `helm-browser`, `helm-sessions`, `floti-agents` (children,
  news, the queue, waits, worktrees, resume, command children), plugin tools,
  and agent definitions.
- [docs/PLUGINS.md](docs/PLUGINS.md) - how Floti runs plugins: API versions,
  pages, backgrounds, programs, tools and sessions.
- [docs/SECURITY.md](docs/SECURITY.md) - credentials, device and host keys, the
  plugin secret store, the browser's permissions.
- [docs/DESIGN.md](docs/DESIGN.md) - the design system. All UI work follows it.
- [docs/TESTING.md](docs/TESTING.md) - how the app is tested.
- [docs/PACKAGING.md](docs/PACKAGING.md) - what the build produces, where the
  data goes, the network posture.
- [docs/SPEC.md](docs/SPEC.md) - the evidence behind past decisions. Look there
  when a change would reverse one; it is reference, not required reading.

This file holds what an agent must not break, one short rule each. How a
subsystem behaves lives in its doc under `docs/`, and the reasoning behind a
rule in a comment at the code it governs. A change in behaviour updates the doc
that describes it, not this file; a line is added here only for a new rule.

## Layout

```
packages/
├── core/     # what clients share: the host's answer shapes (types.ts), helm.db and its copy into the host (store), layout, themes, the start-up sweep
├── ui/       # React components
├── desktop/  # Electron main, preload and renderer
└── plugin-sdk/ # manifest validator, bridge types, helm-plugin CLI, authoring guide
examples/sample-plugin/  # every plugin surface; the plugin the E2E tests drive
crates/
├── floti-protocol/  # the host-client protocol; generates JSON Schema and TypeScript into generated/
└── host/            # floti-host: the `floti` binary (`floti host`); per-OS code only in src/platform/*/{windows,macos,linux}.rs
    ├── src/agent/   # the AgentBackend trait; one module per agent (claude_code/), plain.rs for any command line
    └── src/keeper/  # `floti keeper`: the session keeper holding every session's and plain terminal's pty and tree
test/fake-agents/    # one fake CLI per agent backend, sharing common.mjs; tests drive these, never a real agent
test/fake-ssh/       # fake ssh, wsl.exe and a custom transport; tests reach no other computer or real distro and never touch ~/.ssh
```

pnpm workspaces with `node-linker=hoisted` (`.npmrc` says why). `core` and `ui`
export TypeScript source, so there is one build step. `crates/` is a Cargo
workspace (`Cargo.toml` at the root) for the Floti host; pnpm does not see it.

| command | does |
|---|---|
| `pnpm dev` | the app with its own data directory (see "Where the data lives") |
| `pnpm check` | typecheck, lint and the TypeScript unit and integration tests - the gate |
| `pnpm build` | production build |

## Testing

- Three tiers, and a test sits in the lowest one that can show what it checks:
  unit (most of them: one module's logic, with the disk, processes, network
  and clock faked), integration (real processes, ptys, sockets, folders and
  database files where meeting them is the point, with the fake agents in
  place of real ones) and end-to-end (very few: the main workflows, on their
  happy paths, through the built app). [docs/TESTING.md](docs/TESTING.md) says
  which kind a change needs.
- While building, run only the tests for what changed. A full suite is a
  regression check. End-to-end tests run only before a release is cut (the
  `procedures` skill): never while building, to check a fix or to finish a
  task. Something only an end-to-end run caught is a gap in the lower tiers:
  add the unit or integration test that fails for it, then fix it.
- A Rust test run names the binary that holds the tests: `--lib` for a
  module's tests, `--test <file>` for one of `crates/host/tests/*.rs`. A
  filter alone builds and links every test binary in the crate first.
  `cargo check` finds compile errors without linking. A test that passed runs
  again only after the code it covers changed.
- `pnpm check` is the gate. The host's Rust tests are not in it and run in
  `.github/workflows/host.yml`. A branch merges on its own tests, run
  locally, and a review that runs the suite for what changed locally; nobody
  waits on CI to merge.
- A test cleans up by construction, never by reaching its own teardown: every
  folder it makes is under its run's root, and every process it starts ends
  with it. `node scripts/test-leftovers.mjs <test command>` reports what a run
  left; it never deletes or kills.
- A test waits on an event or a polled condition, bounded by the shared
  patience, never on time. Logic driven by a timer, deadline or grace is tested
  by stepping time.
- Every change ships with its tests: a bug gets a failing test before the fix,
  a feature gets tests that cover it.
- The in-app drivers (`fidelity`, `claude-check`, `packaging-check`) and
  `scripts/drive-dev.mjs` are diagnostic tools, not tests. No change "owes" a
  driver run.

## UI

- Follow DESIGN.md: semantic tokens only (no raw hex in components outside its
  foreign-ground islands), islands with hairline edges on a sunken canvas, the
  accent solid-fills nothing but a checked checkbox, no shadows outside
  modals, no text weight past 500 but 600 on caps labels of 11px or less, mono
  for machine data. Anything the tokens cannot express is a DESIGN.md
  amendment.
- Look at a UI change before calling it done: `pnpm dev --drive`, then
  `node packages/desktop/scripts/drive-dev.mjs shot <file>.png`. A `--drive`
  window starts only the fake agents. The `dev` skill has the rest.

## Boundaries

- `packages/core` never imports Electron (`no-electron-in-core` in
  `eslint.config.js`). If core needs something from the host, the host passes
  it in.
- A value import into the browser bundle comes from `@helm/core/types`, never
  the package root. This fails at rollup, not at typecheck.
- Every channel a Helm page uses is declared in `shared/ipc.ts`. The preload
  exposes three generic functions; a feature adds a channel, not a bridge
  method. `term:*` (the spike page's single pty) and `session:*` (the host's
  sessions) never mix.
- A host channel added to `shared/ipc.ts` is also declared in
  `crates/floti-protocol`, or listed there as client-only or host-internal
  with a reason (`channels/boundary.rs`), and the generated files are
  rewritten with `cargo run -p floti-protocol --bin generate`. A crate test
  reads `shared/ipc.ts` and fails naming any channel that is neither.
- Electron IPC carries only what `channels/boundary.rs` lists as client-only,
  and its host-internal channels from the hidden plugin window; every other
  call goes over the page's own protocol connection to the host.
  `e2e/boundary.spec.ts` fails naming any channel that crossed. Main runs no
  session, terminal, config reader, content, template or scan service of its
  own; those are the host's.
- The host never opens `helm.db` or learns Helm's data folder; what only
  `helm.db` holds, main sends it on `carry:*` (`main/host/own-data.ts`). Main
  owns the window's lifetime and `helm.db`: `will-quit` releases the database,
  and nothing else closes the store.
- The host owns process lifetime. Every session's and plain terminal's pty and
  process tree is held by the session keeper (`floti keeper`,
  `crates/host/src/keeper`), so a host that crashes takes no session with it.
  Helm never stops the host on its own. What quitting, stopping and
  reconnecting end is in [ARCHITECTURE.md](docs/ARCHITECTURE.md).
- Tabs, panes, splits and families are kept per device (`paneLayout` in
  `helm.db`) and never reach the host.
- A host on another computer is reached through the user's own
  `ssh -o BatchMode=yes <target>`, the command they gave, or `wsl.exe`. Floti
  adds no ssh option, never skips a host-key check, reads or writes nothing
  under `~/.ssh` and holds no SSH secret.
- Do not use `@anthropic-ai/claude-agent-sdk`.
- Helm renders nothing for a live session and answers no permission prompt.
  The host reads one thing in a session's output, OSC 7501 status reports,
  draws no conclusion from screen text, passes every byte on untouched and
  puts nothing between the user and the TUI. No agent runs in an RPC or JSON
  mode in place of its own TUI. Rendering an archived transcript (read-only,
  after the fact) is allowed.
- Windows, macOS and Linux: links are junctions (`mklink /J`) on Windows and
  symlinks on macOS and Linux; nothing needs elevation; tests use paths with
  spaces. What differs by OS sits behind one interface per concern (in the
  host, each a folder under `crates/host/src/platform`; in the desktop,
  `treeKill` and `isolate.mjs`), with one implementation per OS, and nothing
  outside it branches on the OS. An OS with no implementation yet reports
  Unknown and the feature around it keeps working.

## Credentials and keys

- Never handle or store any agent's credential, or GitHub's. Each agent
  backend lists its credential paths (`credential_paths` in the host's
  `AgentBackend`), and nothing opens them. A sign-in is checked by the agent's
  own status command where it has one, by an artefact existing where it does
  not (for Claude Code `.credentials.json`, `ANTHROPIC_API_KEY`, the onboarding
  record in `.claude.json`), and is Unknown otherwise. Never open those,
  `hosts.yml`, an agent's or GitHub's keyring entry or `GH_TOKEN`, and never
  run a command that prints a credential.
- `~/.claude/sessions` keeps a `<pid>.<sha256>.key` credential beside every
  `<pid>.json`. The host's Claude Code status reader
  (`agent/claude_code/status.rs`) reads `.json` files only; never widen it.
- Nothing reads the `persist:helm-browser` partition, which holds the user's
  cookies and logins; the only call on what it stores is `clearStorageData`.
- The plugin secret store (`crates/host/src/plugins/secrets`) is the one place
  Helm keeps a secret of the user's, and only one the user typed into it:
  sealed by this OS's store, refused when it cannot be, never sent to any
  client. Helm never puts a Claude or GitHub credential there.
- The user's login is the access to Floti: it adds no approval, code, role or
  prompt and keeps no list of allowed devices. A private key is never logged,
  printed, sent to another client or put in a session's environment; logs
  name a key by its fingerprint. Key files are owner-only
  (`platform/private_files`). Keys and Noise sessions come from snow, through
  `floti_protocol::noise`; never hand-rolled crypto.
- The network posture is stated identically in README, docs/PACKAGING.md, the
  network posture comment in `shared/ipc.ts` and SPEC 5; change all four
  together. The host's update check is the only request Helm makes on its own
  initiative; the app itself contacts nothing.

## Browser pane

- `will-navigate` and `setWindowOpenHandler` are denied on every web-contents
  in `main/index.ts`. Browser views are exempted by a `webContents.id` registry
  read inside those guards. Widen the registry, never the guard.
- Every navigation goes through `browserReachAllows` in `@helm/core`, an
  agent's also through `agentReach` and `browserMcpLocalOnly`. `file:` and
  custom schemes are refused there.
- A view can lose its web contents. Reach them only through `contentsOf` in
  `browser.ts`. `registerIpc` wraps every send handler, because a throw there
  is an uncaught main-process exception.
- A native view paints over all DOM, so it hides while anything is drawn over
  it, through `overlayOpen()` (`ui/src/lib/overlay.ts`). Hide a view with
  `setVisible(false)`. The view never enters the top 36px, where Windows draws
  the window controls, except in fullscreen.
- Self-signed certificates are accepted for loopback only, and there is no
  `certificate-error` handler. Downloads are refused and handed to the system
  browser. Every permission is denied but clipboard writing and fullscreen.

## Agent tools

- The host serves them (`crates/host/src/agent_tools`) on `127.0.0.1` only.
  A request without `Authorization: Bearer` gets 401 before its body is read,
  one announcing a non-loopback `Origin` gets 403, and there is no
  unauthenticated route, not even a health check. Main binds no listener for
  them, mints no token, writes no `--mcp-config` and never serves
  `floti-agents`.
- One token per session, minted at launch and revoked when it ends. A
  session's row keeps the token's SHA-256, never the token.
- A tool family switched off has no route (404), no entry in `--mcp-config` and
  no tool.
- Agent tools are registered per launch (for Claude Code a `--mcp-config` file
  in the session's launch folder, `agent/launch_folder.rs`), never through an
  agent's own add command or global config.
- `sessions_list` and `session_detail` never return any part of another
  session's conversation: no transcript, prompt, output, argv, conversation id
  or child command line. The shaping types (`session/describe.rs`) have no
  field for them.
- `floti-agents` reaches only the caller's own children. Nothing is typed into
  a parent, into a session waiting on a permission prompt or into a terminal a
  person has typed in since its last turn, and no tool returns a transcript,
  argv or conversation id.
- Each tool and `floti agent` subcommand a launch may run without asking is
  named on its own; never a `floti agent:*` wildcard, never `run`, and a
  user's deny rule still wins.
- A host serves a plugin's tools only for a plugin it has itself, on,
  error-free and with its tools on (`PluginRelay::installed`), whatever a
  client offers.

## Plugins

- A manifest change starts in `packages/plugin-sdk`, whose validator Helm loads
  with. The SDK ships what `files` in its `package.json` lists, and nothing it
  ships may reach outside its own folder: run through npx, that folder is a
  cache.
- A plugin page's CSP has `connect-src 'none'`: `floti.fetch` (`helm.fetch` at
  version 1) is its only way out, to origins the manifest lists and its own
  service, checked again on every redirect hop. `helm.exec` runs only programs
  the manifest names, with an argument array and no shell.
- Frames live outside React (`app/pluginFrames.ts`) and move between slots
  with `moveBefore`; `appendChild` would reload them.
- A plugin's failure shows in its own surface (`PluginFrame`), never in Helm's
  chrome, and never takes Helm down.
- A plugin starts a session or opens a link only from a click in its page (the
  relay checks the window's user activation), and starts a session only once
  the user agrees in Helm's dialog.

## Overlays and templates

- The host composes overlays in each session's launch folder under its own
  data folder, never `%TEMP%`: they hold junctions into real repositories.
- Anything removing an overlay or a shim or walking a template unlinks links
  (junctions on Windows, symlinks on macOS and Linux) and never walks into
  one. `fs.rm` with `recursive: true` has been seen to go through a junction
  and empty the repository folder it points to.
- Launch folders and leftover overlay shims are swept only at start, and only
  where every owning process is provably dead. `EPERM` counts as alive;
  unknown means keep.
- The shipped templates and agents are seeded only where their folder is
  absent, and nothing there is ever overwritten: an edit or a deletion stays.
  A host started with `FLOTI_CONFIG_READ_ONLY` (Helm sets it for `pnpm dev`,
  `dev:live` and every check) moves, seeds and writes nothing in its config
  folder.

## Agent config and session state

- Every agent's config directory (`~/.claude` for Claude Code) is read-only to
  Floti; the agent's own commands (`/config`, `/mcp`, `/hooks`, `/agents`)
  edit it. What a launch needs is a per-launch file under the data directory,
  never a write into the agent's config.
- Session state comes from each backend's own signals, read only, then from
  OSC 7501 reports. Anything that cannot be interpreted degrades to the plain
  tab.
- `renderer/src/terminal.ts` and `crates/host/src/session/env.rs` are tuned
  for TUI fidelity (SPEC 8.3), and `ptyEnv` (`main/pty.ts`) is kept to match.
  Terminal preferences are passed into `createTerminal`; never route a setting
  through `term:*`. The palette and contrast options are fixed. `estimateGrid`
  must keep measuring the way xterm does.
- "Could not look" (`null`, painted "Unknown") and "nothing there" (`[]`) are
  never merged.
- Usage figures paint nothing rather than a wrong number. Dollar figures are
  estimates and say so.
- Every backend resumes in the directory history recorded. Claude Code's
  transcripts are found by scanning `projects/*` for `<uuid>.jsonl`, never by
  deriving a path.
- `CLAUDE_CONFIG_DIR` moves credentials too, so a session pointed at a fixture
  home cannot sign in.

## Where the data lives

`appMode` in `paths.ts` is the authority; [PACKAGING.md](docs/PACKAGING.md#where-the-data-goes)
has every run on every OS. The dev and check folders (`isolatedBaseFor` in
`scripts/isolate.mjs`) never sit inside the installed app's folder, on any OS.
`pnpm dev --drive` starts only the fake agents and never runs a real agent or
writes the real `~/.claude`; the `dev` skill has the rest.

## Never touch the installed app

`pnpm verify:installer --yes`, `pnpm dist:win` where it feeds that, and
`pnpm dev:live` act on the installed Helm or its data. Run them only when the
owner asks, never as part of a suite. `packaging-check` also runs only on
request.

## Releases and tracking

- A release is a version bump in `packages/desktop/package.json` plus a
  `## <version>` section in CHANGELOG.md, merged to `main`. CI does the rest.
  The `procedures` skill has the detail.
- The plugin SDK publishes to npm on its own version: a bump in
  `packages/plugin-sdk/package.json` merged to `main`. A change to the SDK
  reaches plugin authors only with that bump.
- Work is tracked in the ClickUp list "Helm - Claude Code Shell"
  (`901114291892`). Nothing in the repository refers to a task by id.
- Machine-specific facts go in `CLAUDE.local.md` (gitignored). No personal path
  or name may appear anywhere in the repository; `pnpm packaging-check
  --only=audit` checks that, when asked for.
