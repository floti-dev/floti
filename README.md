# Floti

A portable, configurable desktop shell **on top of** Claude Code. Not a client,
not a reimplementation - Floti hosts the real `claude` TUI in an embedded terminal
and owns everything that happens *before* and *after* a session.

## Why

Claude Code resolves `.claude/` configuration (skills, commands, agents, CLAUDE.md)
relative to the working directory. That forces a bad choice at launch time:

| Launch from | You get | You lose |
|---|---|---|
| The workspace root | cross-repo access, workspace tooling | every project-local skill and agent |
| A single repo | that repo's skills and agents | cross-repo access |

Floti dissolves the tradeoff. This is the one idea the whole app is built around,
so it is worth the paragraph:

**Overlay composition.** For each project you want composed in, Floti synthesizes
a throwaway *plugin* in its own data directory - a manifest plus junctions
(`mklink /J`, no elevation) pointing at that project's real `.claude/skills`,
`.claude/commands`, `.claude/agents` and `.claude/workflows`. It then launches
`claude` **from the root you chose** with one `--plugin-dir` per overlay,
`--add-dir` for the files the session may touch, and the overlays' `CLAUDE.md`
files concatenated into a single `--append-system-prompt-file` - except an
overlay at or above that root, whose `CLAUDE.md` the CLI already loads. The result is a session whose working
directory is the workspace root and whose skill set is the union of every repo
you named.

Three measured facts make that predictable rather than hopeful:

- **Namespacing is automatic and deterministic.** The platform prefixes
  everything a plugin contributes with the plugin's manifest name, and Floti
  chooses that name. A skill called `think` in a repo overlaid as `api` is
  `/api:think`. Two repos that both define `think` both resolve, each under its
  own prefix - collisions cannot occur, so Floti predicts names instead of
  detecting clashes.
- **`--add-dir` does not carry an overlaid repo's `CLAUDE.md`.** Measured on
  2.1.225: a session launched from the root with both flags reported only the
  user and cwd instruction files. That is why the instructions go through
  `--append-system-prompt-file` - a *file*, because two repos here total 34 KB
  against a 32,767-character Windows command line.
- **The shims live under the app data directory, never `%TEMP%`.** Their
  subdirectories are junctions into your real repositories, and a temp cleaner
  that follows a reparse point instead of unlinking it deletes the repo's
  `.claude/skills`. They are swept only at app start.

A saved composition is a **profile**: root + overlays + access + model, effort,
permission mode, MCP set and opening prompt. One click to launch, exportable as
YAML so it travels with the workspace.

## Install

Windows 10/11, x64. No admin rights are needed for any of these.

**Portable** - download `Helm-<version>-portable.exe`, put it wherever you like
and run it. It keeps everything in a `helm-data\` folder beside itself, so the
whole install travels on a stick and removing it is deleting two things.

**Installed** - download `Helm-<version>-setup.exe` and run it. It installs for
your user only, into `%LOCALAPPDATA%\Programs\Floti\desktop`, and keeps its data in
`%LOCALAPPDATA%\Floti\desktop`. An installed Helm moves there: the installer
replaces it, and the app copies Helm's data from `%APPDATA%\Helm` at its first
start and removes the old folder once the copy has opened. Uninstalling removes the program and **keeps** your data - the
database holds your profiles, session index and the versions of the notes you
saved.

**From floti** - where floti is installed, `floti add desktop` installs the
desktop app of floti's own release for your user, with no admin rights: on
Windows where the installer puts it, on macOS as `~/Applications/Floti.app`, and
on Linux as `~/Applications/Floti.AppImage` with a menu entry. It checks the
download's signature and SHA-256 before anything runs, changes nothing where
the app is already installed, and refuses in a session with no display.
`floti update` keeps it up to date from then on. `floti setup`, which the
install scripts run, asks once whether to install it as well where there is
a terminal and a display, and says where it is when it is installed already;
`floti setup --desktop` installs it without asking and `--no-desktop` leaves
it out. Setup also puts floti's folder on your `PATH` where it is not there:
on macOS and Linux by adding one line to the startup file your shell reads,
which it names.

**Uninstalling Floti** - `floti uninstall` stops the host, which ends any
running sessions (their history stays), then removes what installing put
there, either way in: the start-at-sign-in entry, the desktop app (on Windows
through its own uninstaller), floti's entry in your `PATH` (on macOS and
Linux the one line setup added to your shell's startup file), and floti with
its Node. It prints one line for each, or `not found`, so running
it again changes nothing. Your data and config folders stay;
`floti uninstall --purge` removes them as well, with Floti's own entry in your
keychain or keyring, and never touches `~/.claude`, `~/.ssh` or any
repository. The desktop app's own settings are the app's and stay, as its
uninstaller leaves them. On Linux it leaves lingering as it is, since other
services may rely on it. Run it from a terminal Floti did not start.

Workspace templates are the one thing outside that: they live in
`~/.config/floti/templates`, because they are folders you write and edit by
hand. While the app was called Helm they were in `~/.config/helm`; the host
moves that folder to `~/.config/floti` once, at its first start as Floti.
A portable install keeps them in its own `helm-data\templates`, so it still
leaves nothing behind on a machine you plug it into.

Floti manages them without trying to replace your editor. **Settings ->
Workspace templates**, or **Manage templates…** in the New Workspace dialog, will
create, rename, describe and delete one, copy a skill you have already written
into it out of any `.claude` tree Floti can see, and open the folder in
Explorer - which is where you edit the files. You can also **save a workspace you
have been working in as a template**, ticking off what belongs to that workspace
rather than to the layout, or **import a folder** somebody sent you.

Expect **"Windows protected your PC"** either way. These builds are not
code-signed, so SmartScreen has no publisher to check and warns about every
download: **More info** -> **Run anyway**. That warning is doing its job, and
you should not wave it away for software you have no reason to trust. If you
would rather check than trust, both files are built from the tagged commit by
[`.github/workflows/release.yml`](.github/workflows/release.yml) and the run is
linked from the release.

You also need Claude Code itself. Floti runs the real `claude` CLI; it does not
bundle one and it never handles your credentials. If you have not signed in,
first run says so and asks you to run `claude` once in a terminal - that is the
entire flow, and there is no login screen anywhere in this app.

### First run

Three things, in the order they matter:

1. **Claude Code.** Floti looks on `PATH` and in the usual install directory. If
   it is somewhere unusual, point at it by hand; the file is run with
   `--version` before it is accepted. The tested range is 2.1.277 up to 2.2.0,
   kept by the host's Claude Code backend - outside it you get a warning strip
   and nothing is blocked. `floti setup` and `floti doctor` check the same
   range and say what to run when Claude Code is missing or outside it.
2. **Signed in.** Detected from the presence of a login, never by reading one.
   The remedy is to run `claude` yourself.
3. **Where your work is.** Add a folder, or create a workspace. Nothing is scanned
   until you say what to scan.

A folder of ordinary repositories is enough - there is no required layout, and
Floti is useful pointed at a directory with no workspace anywhere in sight.

### Workspaces

A *workspace* is any folder with a `workspace.yaml` in it. That is the whole
definition. A workspace made while they were called harnesses has a
`harness.yaml` instead, and is read exactly the same way; with both, Floti reads
`workspace.yaml`. Floti never renames or rewrites either. It reads four optional
keys:

```yaml
name: "work"        # what the sidebar calls it
template: "minimal" # informational
version: "1"        # manifest format
repos: "projects"   # where the repositories are; omit for `repos/`
```

`repos:` is the one that matters for an existing folder. Without it a workspace
lists `repos/*` and nothing else, so dropping a manifest into a folder whose
repositories sit at its **top level** would hide all of them. `repos: "."` says
they are right here. The "turn a folder into a workspace" action writes that for
you.

Creating a workspace scaffolds the minimum and nothing else - `workspace.yaml`,
`repos/`, an empty `.claude/`. No starter skills, notes or rules: what belongs
in a workspace is yours to decide.

## What it does

- **Profiles.** The saved composition described above. One click to launch,
  exportable as YAML.
- **A terminal that hosts the real thing.** xterm.js + node-pty running the
  unmodified `claude` TUI in tabs. Floti supplies argv, cwd and environment, then
  gets out of the way - it renders no messages, parses no output, handles no
  permission prompts.
- **Project discovery.** Workspaces and their repos, auto-detected, with each
  one's skill/agent/command counts and git state at a glance.
- **History** (Ctrl+H). Every session recorded in `~/.claude/history.jsonl`,
  across every directory - which the CLI's `/resume` cannot show you, because it
  reads only the one you started it in. A palette over the window: the sessions
  worked in last by title, a search over names, prompts and archived
  conversations, Enter to resume and Ctrl+Enter to read one back in a tab.

  Claude Code keeps prompts indefinitely and reaps the transcripts behind them,
  so most of that list is a record rather than a door. Floti marks which is which
  instead of offering a resume that fails.
- **Files**: a project's tree with git status, Ctrl+P for a file by name and
  Ctrl+Shift+F by what it says, and the markdown and HTML artifacts Claude
  writes opened rendered - with wikilinks, an editor beside a live preview for
  notes, and a sandboxed frame for artifacts.
- **Usage in the status bar** - session and weekly percentages read from Claude
  Code's own cached figures, or an estimate in dollars. A reading Floti cannot
  stand behind shows nothing at all rather than a stale number.
- **Plugins** - a folder with a `helm-plugin.json` adds a rail icon and panel,
  tabs, a status bar item, commands for Ctrl+Shift+P and a settings page, all
  drawn in Floti's theme. Each runs in a process of its own, so one that hangs
  or crashes takes nothing else with it. Add one in **Settings → Plugins**;
  [the SDK](packages/plugin-sdk/README.md), `@floti-dev/plugin-sdk` on
  npm, has `helm-plugin create`, `helm-plugin validate` and the authoring
  guide.

### The version guard warns; it never gates

Claude Code's flags are a stable public surface and a newer CLI is far more
likely to work than not, so Floti pins a tested range, says when you are outside
it, and otherwise stays out of the way.

## Updates

floti updates itself. While **Settings → Updates → "Update floti by itself"**
is ticked (it is by default), this PC's host reads floti.dev's signed
`update.json` at most once a day, and when there is a newer floti it downloads
it, checks it against the key built into floti and its SHA-256, puts it in
place and restarts on it. It ends no session to do it: every session and
terminal runs on in the session keeper and the new host takes them back.
`floti update` does the same when you run it, and updates the desktop app too
- see [docs/PACKAGING.md](docs/PACKAGING.md#floti-update).

Floti itself contacts nothing. It hears what this PC's host found and puts a
line beside the version in the status bar when there is a newer release than
this Floti. **Settings → Updates** shows this build's version and the newest
release, and has three buttons. **Check now** has the host check at once,
tick on or off. **Update** has the host run `floti update` for floti and
Floti: Floti closes when the update is ready for it, is replaced, and opens
again, and every session keeps running throughout. **Release notes** is a
link handed to your browser rather than a request, so it works offline. A
development run's host checks nothing and updates nothing, and the pane says
so.

**The host's update check, at most once a day to floti.dev and to the
GitHub release it names, is the only request Floti makes on its own
initiative; the app itself contacts nothing.
Everything else on the network happens because you asked for it: Check
now and Update ask the host to check or to update, the browser pane
fetches the page you navigate to, a plugin you added reaches the hosts
its manifest names, and a host you added on another computer is reached
through your own ssh, or the command you gave.** There is no telemetry, no
crash reporting, no fonts and no CDN, and with the tick off nothing asks
anything unless you press something.

A host on another computer is reached through your own ssh login, or a command
you choose, running `floti connect` there as you. That login is the access:
adding a host is connecting to it, with no pairing code, approval or role, and
every device that gets in can do what you can at that host. Floti pins the
host's key at the first connection and refuses a host that proves another.
**Settings → Devices** shows who is attached to each connected host, with
Disconnect beside each; to keep a lost device out, take its SSH key away as you
would for any machine.

**Floti listens only on this machine.** Its windows reach the local host over
the host's local socket, which only your OS account can open. The local host
listens on `127.0.0.1` for its sessions' agent tools, behind a token minted
for each session and revoked when it ends, and holds no port while no tool
family is on. Floti's own process listens on `127.0.0.1` only for a port you
forwarded from a session on another computer, and closes it when that session
ends.

A Claude session the local host runs can reach three sets of tools - one that
drives Floti's browser pane (open a page, read it, screenshot it, click and
type), and one that answers what the *other* Claude Code sessions on this
machine are doing, so an agent can stay out of a working tree somebody else is
in, and one that starts and talks to the session's own child sessions - and
the tools of a plugin you added that offers them. All of them arrive
through one small server the host binds to `127.0.0.1`, on a port the OS picks.
Nothing outside this machine can reach it, and every request needs the token
of the session making it. The host has no browser of its own: it hands each
browser call to Floti's window, which answers it in the pane.

Each set has its own tick - **Settings → Browser**, **Settings → Sessions** and
**Settings → Agents** (Sessions can start agents) - and any one off means those
tools are not registered with a session at all.
The browser settings also decide how far those tools may go: they can never
reach further than the pane itself may, and can be held to this machine while
the pane is not. The session-awareness tools are the read-only set: they say
what is running and give an agent no way to affect it, and they never return
any part of another session's conversation. The child-session tools reach only
the session's own children.

A **forwarded port** is opened only when you ask for one. A session on a host
on another computer that runs a dev server lists its port in the sessions
pane, and **Open in browser** forwards it: Floti listens on `127.0.0.1` on this
PC, on the same port number when it is free here and on another free one when
not, and carries each connection made to it over its own link to that host,
which connects only to its own loopback and only for a port that session's
processes listen on.

A **plugin** you add reaches the network on the terms its manifest states.
Its pages have none of their own: every request goes through Floti, to the
hosts the manifest names and nowhere else, with every redirect checked against
the same list. A plugin can do that on its own schedule - that is what its
background page is for - so "because you asked" means you added it. When it
needs a token, you type it into **Settings → Secrets** and say which hosts and
plugins may use it; the local host stores it sealed by your OS's own store
for your account, refuses to store it at all when that is unavailable, fills it into
a request at the last moment and never hands the value back to any page. The
programs and the background service a plugin declares are a different thing:
they are ordinary programs running as you, and Floti does not limit what they
reach. **Settings → Plugins** lists each plugin's hosts, programs and service,
and turning a plugin off stops all of them. A service listens on `127.0.0.1`
on a port Floti hands it, behind a token for that run - the plugin's process
listening, not Floti's.

## Architecture

```
packages/
├── core/      # shared: types.ts, store/ (helm.db), layout/, theme/ - ZERO electron imports
├── ui/        # React components
├── desktop/   # Electron main + preload + renderer + pty host
└── plugin-sdk/ # the manifest validator, the bridge's types, the helm-plugin CLI
```

Stack: Electron, TypeScript strict, React + Vite, Tailwind, xterm.js + node-pty,
better-sqlite3 + Drizzle, electron-builder (portable + NSIS).

`core` and `ui` export TypeScript source rather than a build output, so the
bundler compiles them in and there is one build step instead of three.

The one hard rule: **`core/` never imports Electron** (ESLint-enforced). That is
what keeps the app portable and a future mobile client possible.

## Documentation

- [docs/SPEC.md](docs/SPEC.md) - the v1 spec, with the measured evidence behind
  each design decision. Read this before changing anything.
- [CLAUDE.md](CLAUDE.md) - the rules that are load-bearing rather than
  stylistic. Each is short on purpose; the argument behind it is in a comment at
  the code site it governs, and how each subsystem behaves is in the docs below.
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) - what each process owns, how
  sessions live and end, tabs and families, the browser pane, overlays,
  templates and the Files view.
- [docs/AGENT-TOOLS.md](docs/AGENT-TOOLS.md) - the tools a session is given:
  the browser, session awareness, child sessions, plugin tools, and agent
  definitions.
- [docs/PLUGINS.md](docs/PLUGINS.md) - how Floti runs a plugin.
- [docs/SECURITY.md](docs/SECURITY.md) - credentials, keys, the secret store
  and the browser's permissions.
- [docs/PACKAGING.md](docs/PACKAGING.md) - what the build produces, where data
  goes, and why there is no auto-updater.
- [packages/plugin-sdk/README.md](packages/plugin-sdk/README.md) - writing a
  plugin: the manifest, the bridge, the primitives and the CLI.
- [docs/SPEC.md](docs/SPEC.md) section 8 - the three spikes in full: the
  packaging and terminal-fidelity findings the current configuration rests on.

## Development

Requires Node 22+ and pnpm 10. `node-linker=hoisted` is set in `.npmrc` because
packaging native modules into a portable exe needs the flat `node_modules`
shape.

```bash
pnpm install
pnpm dev               # the app, with hot reload, isolated
pnpm dev --fresh       # ...against no database at all, which is first run
pnpm dev --drive       # ...with the port scripts/drive-dev.mjs clicks through
pnpm dev:live          # ...against %LOCALAPPDATA%\Floti\desktop, the installed app's own
pnpm check             # typecheck + lint + unit and integration tests
pnpm dist:win          # portable exe + NSIS installer
```

`pnpm dev` keeps its data in `%LOCALAPPDATA%\Floti Dev\dev`: its own database - a
copy of the real one, taken at launch - its own Chromium profile and its own
overlay shims. It can therefore run beside an installed Floti, and a second
`pnpm dev` gets a directory of its own rather than failing on a held database.
What it does **not** isolate is `~/.claude`: `CLAUDE_CONFIG_DIR` moves
credentials too, and a dev build that cannot sign in cannot host a session.
`pnpm dev:live` is the old behaviour, kept because it is the only way to see the
real database in a dev build - it says so on the console at startup, and the
status bar names the mode.

`pnpm check` and `pnpm test:e2e` run the tests locally. CI also runs
`pnpm check` on Windows, macOS and Linux on every push, and a failure on any
of them blocks the run and the release. `pnpm test:e2e` stays local, and
`pnpm test:posix` runs the tests of the POSIX code under Linux in WSL from
Windows. [docs/TESTING.md](docs/TESTING.md) describes the tiers and the
rules every test follows.

A few drivers open the real window to diagnose what a test cannot reach. They
are tools, not tests, and nothing has to pass them before a change is done:

```bash
pnpm shell             # interactive pane hosting pwsh
pnpm --filter @helm/desktop claude   # ...hosting the real claude TUI
pnpm fidelity          # terminal fidelity inside xterm
pnpm claude-check      # the real claude TUI in the pane
pnpm selftest          # native modules in a packaged build
```

Each runs in a data directory of its own and writes a JSON report there.

Schema changes go through Drizzle:

```bash
pnpm db:generate       # drizzle-kit generate, then embed the SQL into the bundle
```

### Releasing

Bump `version` in `packages/desktop/package.json` and merge it to `main`. That
is the whole procedure. `.github/workflows/release.yml` runs the checks, builds
floti for every host and the desktop app for Windows, macOS and Linux on clean
runners, and publishes a GitHub release tagged `v<version>` with every file
attached, each with its minisign signature and a build attestation, and that
version's section of CHANGELOG.md as its notes. A version
with a pre-release part publishes as a public pre-release, never `latest`. A
push to `release-dry-run/<name>` runs all of it and publishes nothing
([docs/PACKAGING.md](docs/PACKAGING.md#releasing)).

A merge that does not change the version runs the checks and stops there, so
`main` is always checked and only a bump cuts a release. Nothing is built on
anybody's laptop, which is the point: the one time a release *was*, a stale
`pnpm.exe` shadowed the managed one and shipped an exe with no
`app.asar.unpacked` that died on its first `dlopen`.

```bash
pnpm verify:artifact   # assert a built exe carries its unpacked native modules
```

[docs/PACKAGING.md](docs/PACKAGING.md) has the reasoning, including why the
release is published outright rather than left as a draft.

## License

MIT - see [LICENSE](LICENSE).
