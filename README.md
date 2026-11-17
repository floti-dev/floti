<h1 align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/brand/lockup-colour-dark.svg">
    <img src="assets/brand/lockup-colour.svg" alt="Floti" width="280">
  </picture>
</h1>

<p align="center">
  <strong>Your coding agents, on every computer you have, in one window.</strong>
</p>

<p align="center">
  <a href="https://floti.dev">floti.dev</a> ·
  <a href="#install">Install</a> ·
  <a href="https://github.com/floti-dev/floti/wiki">Wiki</a> ·
  <a href="https://github.com/floti-dev/floti/releases">Releases</a> ·
  <a href="CHANGELOG.md">Changelog</a>
</p>

Floti is a desktop app for [Claude Code](https://claude.com/claude-code). It
runs the real `claude` terminal in tabs, keeps your sessions running when the
app closes, and shows the ones on your servers and WSL distros beside the ones
on this PC.

Floti is Old Norse for a fleet, said FLOH-tee.

> [!NOTE]
> Floti is a personal project, shared as-is. I don't take pull requests,
> issues or feature requests, and it changes when I need it to. You're welcome
> to fork it under another name. Security reports go through
> [SECURITY.md](.github/SECURITY.md).

## Features

- **Sessions outlive the window.** Close the app, or let it crash, and your agents keep working.
- **Every computer in one window.** This PC, your servers over your own `ssh`, and your WSL distros.
- **The real terminal.** Claude Code runs in its own TUI, and Floti never draws over it.
- **Skills from every repo at once.** A profile starts a session with other projects' skills, agents and `CLAUDE.md` composed in.
- **History across every folder.** Search and resume any session you've run (Ctrl+H).
- **Files.** Browse and search a project, and read the Markdown and HTML your agents write, rendered.
- **A browser pane** your agents can drive.
- **Agents that start agents**, each in a tab you can watch.
- **Plugins, themes and workspace templates.** Plugins are built with the [plugin SDK](packages/plugin-sdk/README.md).

## Install

You need [Claude Code](https://claude.com/claude-code) installed and signed in.

Install `floti` on every computer you want agents on. No admin rights needed.

```sh
# macOS, Linux and WSL
curl -fsSL https://floti.dev/install.sh | sh
```

```powershell
# Windows, in PowerShell
irm https://floti.dev/install.ps1 | iex
```

Setup offers to install the desktop app too. To add it later, run
`floti add desktop`, or download it from the
[latest release](https://github.com/floti-dev/floti/releases/latest):

| OS | File |
|---|---|
| Windows 10 or 11, x64 | `Floti-<version>-setup.exe`, or `Floti-<version>-portable.exe` |
| macOS, Apple Silicon | `Floti-<version>-arm64.dmg` |
| Linux x64, glibc 2.34+ | `Floti-<version>-linux-x64.AppImage` |

The app isn't signed with a paid certificate, so the first launch shows a warning:

- **Windows:** More info → Run anyway.
- **macOS:** System Settings → Privacy & Security → Open Anyway.
- **Linux:** on Ubuntu 23.10 and later, the browser pane stays off and shows the `sudo` command that turns it on.

Every release is built in CI from its tagged commit, with minisign signatures
and GitHub build attestations.

## Getting started

The first time Floti opens, a short walkthrough checks Claude Code, sets up a
workspace and shows you around the window. `floti doctor` checks Claude Code
from a terminal.

To add a server or WSL distro later, open **Settings → Hosts**. Floti reaches
a server through your own `ssh` login and can install `floti` there for you.

## Updates

`floti` updates itself, checking at most once a day, and your sessions keep
running through it. Turn that off in **Settings → Updates**, or update by hand
with `floti update`, which updates the desktop app too.

## Privacy

There's no telemetry, no crash reporting and no account.

**The host's update check, at most once a day to floti.dev and to the
GitHub release it names, is the only request Floti makes on its own
initiative; the app itself contacts nothing.
Everything else on the network happens because you asked for it: Check
now and Update ask the host to check or to update, the browser pane
fetches the page you navigate to, a plugin you added reaches the hosts
its manifest names, and a host you added on another computer is reached
through your own ssh, or the command you gave.**

**Floti listens only on this machine.** Its windows reach the local host over
the host's local socket, which only your OS account can open. The local host
listens on `127.0.0.1` for its sessions' agent tools, behind a token minted
for each session and revoked when it ends, and holds no port while no tool
family is on. Floti's own process listens on `127.0.0.1` only for a port you
forwarded from a session on another computer, and closes it when that session
ends.

Floti never reads your Claude Code or GitHub credentials. The details are in
[docs/SECURITY.md](docs/SECURITY.md).

## Uninstall

```sh
floti uninstall          # removes floti and the app, and keeps your data
floti uninstall --purge  # removes Floti's data and settings as well
```

Uninstalling ends running sessions, so run it from a terminal Floti didn't
start. Neither command touches `~/.claude`, `~/.ssh` or your repositories.

## Documentation

- [The wiki](https://github.com/floti-dev/floti/wiki): workspaces, templates, themes and writing plugins.
- [The plugin SDK](packages/plugin-sdk/README.md): manifests, the bridge and the `floti-plugin` CLI.
- [docs/](docs/): how Floti is built, starting with [ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Building from source

You need Node 22+, pnpm 10 and the Rust toolchain named in `rust-toolchain.toml`.

```sh
pnpm install
pnpm dev      # the app with hot reload, in its own data folder
pnpm check    # typecheck, lint and tests
pnpm build    # a production build
```

`pnpm dist:win`, `dist:mac` and `dist:linux` package the app, each on its own OS.

## License

[Apache 2.0](LICENSE), with [NOTICE](NOTICE). The license doesn't cover the
Floti name or logo. Third-party licenses ship in the app as
`THIRD-PARTY-NOTICES.txt`, and with each release as
`floti-<version>-THIRD-PARTY-NOTICES.txt`.
