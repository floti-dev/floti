# Packaging and updates

What `pnpm dist:win`, `pnpm dist:mac` and `pnpm dist:linux` produce, where
their data goes, how a release is cut, and why Helm has no auto-updater.

## Artefacts

`pnpm dist:win` writes two files to `packages/desktop/dist-app/`:

| File | What it is |
|---|---|
| `Helm-<version>-portable.exe` | One file, ~95 MB. No install. Keeps its data in `helm-data\` beside itself. |
| `Helm-<version>-setup.exe` | NSIS one-click installer. Per-user, no elevation. Data in `%APPDATA%\Helm`. |

`pnpm dist:mac` writes two more, and runs only on macOS, because
electron-builder makes a dmg only there:

| File | What it is |
|---|---|
| `Helm-<version>-arm64.dmg` | Disk image: Helm.app beside an Applications link to drag it onto. Apple Silicon only. |
| `Helm-<version>-arm64-mac.zip` | The same Helm.app, zipped, for anyone who would rather not mount an image. |

`pnpm dist:linux` writes one, on Linux:

| File | What it is |
|---|---|
| `Helm-<version>-linux-x64.AppImage` | One file that runs with no install and no root on x64 Linux with glibc 2.34 or later: Ubuntu 22.04, Debian 12, Fedora 35, RHEL 9 and anything newer. Data in `~/.config/Helm`. |

Linux is x64 only. There is no arm64 Linux desktop build; floti itself does
ship for arm64 Linux, so such a computer can still be a host (see
[floti for every host](#floti-for-every-host)). The AppImage's executable is
`helm` (`executableName`), its icon the 256px `build/icon.png`, its menu entry
`helm.desktop`, which is also the package's `desktopName`, so a Linux desktop
ties the running window to the entry's name and icon.

**glibc 2.34 is the floor.** better-sqlite3's Linux prebuild needs it, and
node-pty's addon, compiled at install, needs whatever glibc it was compiled
against. The release therefore builds the AppImage in an `ubuntu:22.04`
container (glibc 2.35), not on the runner's newer system, and
`verify-linux-artifact.mjs` refuses an AppImage in which any ELF file needs a
glibc newer than 2.34. A file that did would stop Helm at start on every
older distribution.

**The AppImage's mount stays out of the host.** An AppImage runs from a mount
that goes away when it quits, and its AppRun puts that mount at the front of
`PATH`, `LD_LIBRARY_PATH`, `XDG_DATA_DIRS` and `GSETTINGS_SCHEMA_DIR`. The host
and its sessions outlive Helm, so Helm takes the mount's entries out of those
lists, and drops `APPDIR`, `APPIMAGE`, `ARGV0` and `OWD`, before starting
them (`withoutAppImage` in `main/host/launcher.ts`): a session never searches
a folder that is gone, which anybody who saw its name could make again. The
host is told to open Helm again by the AppImage's own path, `APPIMAGE`,
rather than `process.execPath`, which is inside the mount (`desktopProgram`).

### The browser pane needs the sandbox

Every renderer runs inside Chromium's OS sandbox, and the browser pane is the
one that loads arbitrary sites. Inside an AppImage the sandbox cannot use a
setuid `chrome-sandbox` (the image is mounted `nosuid`) and needs unprivileged
user namespaces instead. Where they are refused, Electron would not start, so
the AppRun electron-builder writes probes them (`unshare -Ur true`) and adds
`--no-sandbox` when the probe fails. Ubuntu 23.10 and later refuse them to any
program without an AppArmor profile allowing it, so there every AppImage
starts that way.

Helm never runs a web page that way. The menu entry carries no `--no-sandbox`
(`executableArgs: []` under `appImage`; electron-builder's default adds it,
and `verify-linux-artifact.mjs` refuses an entry with it), and when main sees
the switch on Linux the browser pane stays off for that run: no view is
opened, no session is given the browser tools, and the Browser tab shows
the fix where a page would be (`main/browser-sandbox.ts`). Everything else
works as usual. The fix on Ubuntu is a profile letting the AppImage make user
namespaces, the way Ubuntu's own profiles let browsers make them. It covers
every `Helm-*.AppImage` in the AppImage's folder, so the next version keeps
it:

    abi <abi/4.0>,
    include <tunables/global>
    profile helm-appimage "/home/<you>/Apps/Helm-*.AppImage" flags=(unconfined) {
      userns,
    }

The Browser tab writes the command that installs it with this AppImage's own
folder filled in, as one `sudo`; with the AppImage in `~/Apps` it is

    printf '%s\n' 'abi <abi/4.0>,' 'include <tunables/global>' 'profile helm-appimage "/home/<you>/Apps/Helm-*.AppImage" flags=(unconfined) {' '  userns,' '}' | sudo sh -c 'cat > /etc/apparmor.d/helm-appimage && apparmor_parser -r /etc/apparmor.d/helm-appimage'

Then start Helm again. Where the namespaces are refused some other way (for
example `kernel.unprivileged_userns_clone=0` on an older Debian), the tab says
the sandbox is off without a command, since the fix is the system's
setting.

Every build comes out of the same code. No node module is rebuilt for
Electron: `npmRebuild: false`, and the native modules are N-API, so the binary
the install put there loads in Electron unchanged. On Windows and macOS each
ships a prebuild, so a clean checkout needs no C compiler, no Python and no
build tools. On Linux node-pty ships none, and `pnpm install` compiles its
addon with node-gyp, which needs Python, make and a C++ compiler (the
ubuntu-latest runner has them). The one thing compiled on purpose is the
`floti` host, which runs every session: each command first runs
`scripts/stage-floti.mjs`, which builds it in release with the Rust toolchain
`rust-toolchain.toml` names and copies it to `packages/desktop/dist-host/`.
electron-builder carries it beside the app, at `resources\floti.exe` on
Windows, `Contents/Resources/floti` on macOS and `resources/floti` on Linux,
where the app looks for it. A release build carries the release's own floti
instead of building it again (see [floti for every host](#floti-for-every-host)),
with the release's SHA-256 list beside it as `floti-SHA256SUMS`; a local build
carries no list.

### floti for every host

Every release also carries `floti` on its own, for each computer a host can run
on, so the desktop can put this version on a host whose OS or CPU differs from
its own:

| File | What it is |
|---|---|
| `floti-<version>-windows-x64.exe` | Windows on x64. |
| `floti-<version>-windows-arm64.exe` | Windows on arm64. |
| `floti-<version>-macos-arm64` | macOS on Apple Silicon. |
| `floti-<version>-linux-x64` | Linux on x64, statically linked: it needs no C library of the computer's own. |
| `floti-<version>-linux-arm64` | Linux on arm64, statically linked. |
| `floti-<version>-SHA256SUMS` | The SHA-256 of each of the five, in `sha256sum` format; `sha256sum --check` reads it. |

`.github/workflows/floti.yml` builds them, each on its own native runner
(`windows-latest`, `windows-11-arm`, `macos-latest`, `ubuntu-latest`,
`ubuntu-24.04-arm`), on every branch push that touches the host, its build or
the workflows. `scripts/floti-release.mjs check` runs and checks each file: its
header names its own OS and CPU, a Linux file names no program interpreter and
needs no shared library (and `file` and `ldd` agree), and `floti --version` and
`floti version` name this version, OS and CPU. Then `floti-release.mjs sums`
refuses anything but exactly the five and writes their list. The files go up
as workflow artifacts only; a branch push tags, releases and publishes nothing.

The Linux files are the gnu targets built with `-C target-feature=+crt-static`
and an explicit `--target`, set in that workflow's `RUSTFLAGS` and in no cargo
config a local build reads. V8, which `deno_core` brings, ships no musl build,
so musl would mean building V8 from source. The explicit target keeps the flag
off build scripts and proc macros, which must stay dynamic.

`release.yml` calls the same workflow, and its desktop jobs hand
`stage-floti.mjs` the release's file for their own platform (`FLOTI_PREBUILT`)
and the list (`FLOTI_SHA256SUMS`). It refuses a file whose header is another
platform's or whose SHA-256 the list does not give, and stages the list as
`dist-host/floti-SHA256SUMS`, which electron-builder carries as
`resources\floti-SHA256SUMS` on Windows, `Contents/Resources/floti-SHA256SUMS`
on macOS and `resources/floti-SHA256SUMS` on Linux. The floti inside the app is
therefore byte for byte the one the release lists. On macOS that holds only
because signing the app leaves floti alone (`signIgnore` in
`electron-builder.yml`): signing a binary rewrites its signature, and so its
SHA-256. The linker has already signed it ad-hoc, which is all Apple Silicon
needs to run it.

### The protocol types

Every release also carries the generated protocol types, the files
`cargo run -p floti-protocol --bin generate` writes to
`crates/floti-protocol/generated/`, for anybody writing a client of the host:

| File | What it is |
|---|---|
| `floti-protocol-<version>.tar.gz` | A folder `floti-protocol-<version>/` holding `floti-protocol.schema.json` (the JSON Schema of every message), `floti-protocol.frames.json` (encoded example frames, for a client's codec tests) and `floti-protocol.generated.ts` (the TypeScript declarations, with `PROTOCOL_VERSION`). |

The `<version>` is the release's, as in every file's name; the protocol's own
version is the `PROTOCOL_VERSION` inside. The release's `protocol` job runs
the crate's `generated_files` test first, so the archive is never stale
against the protocol the release's floti speaks, and packs it reproducibly:
the same commit gives the same bytes.

### Installing floti on a host's computer

Settings > Hosts installs or updates floti on another computer over the
transport the user gave it, and only after the user says yes to an offer that
names the computer's OS and CPU, the floti found there, and what will be done
where. The app carries `scripts/install/install.sh` and `install.ps1` as
`resources/install` (top-level `extraResources`). Each is one function, called
on its last line, and is sent as the line itself: `sh -c '<script>' ...`, or
`powershell -EncodedCommand` with its comments left out to stay inside cmd's
line limit. Each writes floti into the user's own folder (`~/.local/bin`, or
`%LOCALAPPDATA%\Programs\Floti\bin`) under a temporary name, checks its
SHA-256, and only then moves it into place, so a floti that was there keeps
working when anything fails. When the computer's OS and CPU are this PC's,
the app sends its own floti on the script's standard input, with that file's
SHA-256. Otherwise the computer downloads the release's file for itself from
`releases/download/v<version>/`, checked against the SHA-256
`floti-SHA256SUMS` gives; with no list (a local build) or no file for that
computer, nothing is offered. Then the app runs `<path> setup` there and keeps
the installed path, which the host's bridge is given with `--floti`.

Beside `floti` goes the Node runtime the local host runs plugin services on.
`scripts/node-runtime.json` pins its version (a Node 24 LTS release) and, for
win32-x64, darwin-arm64 and linux-x64, the archive from nodejs.org and its
SHA-256 from that release's `SHASUMS256.txt`. `stage-floti.mjs` downloads the
archive into `<cargo target>/node-runtime/`, or reuses one already there, and
checks its SHA-256 before extracting anything: a mismatch, or a platform the
pin file does not list, fails the build. It then extracts only Node's binary
and `LICENSE`, with the system tar (Windows' own `tar.exe`, which reads zip),
into a fresh folder there, and stages them as `dist-host/node/`. The app
carries them at `resources\node\node.exe` on Windows,
`Contents/Resources/node/bin/node` on macOS and `resources/node/bin/node` on
Linux, each with Node's `LICENSE` beside it. Node is fetched at build time only; the app never downloads it. To move to
a newer Node, change the version, archive names and checksums in the pin file
together.

### The Mac build is ad-hoc signed

Apple Silicon runs no arm64 binary without a code signature, and Helm has no
Apple Developer ID to sign with, so `identity: '-'` in `electron-builder.yml`
signs it ad-hoc: a valid signature with no certificate behind it. A copy
downloaded in a browser is quarantined, so macOS refuses it the first time it
is opened; **System Settings > Privacy & Security > Open Anyway** opens it, once.
A copy built on the machine itself carries no quarantine and just opens.
Developer ID signing and notarization would remove that step and wait on an
Apple Developer account.

`hardenedRuntime` is off with it. The hardened runtime is only for notarization,
and with an ad-hoc signature it turns on library validation, which refuses to
load the native modules. `darkModeSupport` is on, or the app is pinned to the
light appearance and the `system` theme never follows macOS into dark mode.

node-pty starts every pty on macOS through `spawn-helper`, an executable in its
macOS prebuilds, and publishes it without the execute bit; without one every
terminal fails with `posix_spawnp failed`. `scripts/spawn-helper.mjs` sets the
bit in two places: as `@helm/desktop`'s postinstall, for a checkout on a Mac,
and from electron-builder's `afterPack` hook (`scripts/after-pack.mjs`) on the
copy under `app.asar.unpacked`. `afterPack` runs before signing, so the
signature covers the file as changed. Neither rebuilds anything, and both do
nothing on Windows and Linux.

The icon is `build/icon.icns`, packed by `pnpm icon` from the same drawing as
the .ico and committed beside it. Its large sizes sit on Apple's icon grid (the
artwork in the middle 824px of 1024) so the wheel stands the size of the icons
around it in the Dock.

`pnpm verify:mac-artifact` opens the dmg and the zip a person downloads and
checks the Helm.app in each: an arm64 executable, a signature that verifies and
is ad-hoc, `CFBundleIconFile` naming a file identical to `build/icon.icns`, no
`NSRequiresAquaSystemAppearance`, better-sqlite3, node-pty (with an
executable `spawn-helper`) and koffi unpacked beside the asar, and a `floti`
identical to `dist-host/floti` with a signature of its own that verifies. It
needs macOS;
`--icon` checks only the .icns, anywhere.

`pnpm verify:linux-artifact` is the Linux counterpart. It extracts the AppImage
a person downloads with the AppImage's own runtime (`--appimage-extract`, no
FUSE) and checks the app inside: an x64 Linux `helm`; better-sqlite3's
prebuild, node-pty's compiled addon and koffi's Linux package unpacked beside
the asar; a `floti` and `floti-SHA256SUMS` byte for byte the ones it is given
(`--floti`, `--sums`; by default `dist-host/`'s), floti executable; the pinned
Node, executable, with its LICENSE; the install scripts and the icon; no ELF
file anywhere in the image needing a glibc newer than 2.34, read from each
file's `.gnu.version_r`; and a `helm.desktop` whose `Exec` has no
`--no-sandbox`. It needs Linux.

## Where the data goes

`src/main/paths.ts` decides, from one signal. electron-builder's portable
launcher extracts the app to `%TEMP%\<random>\` and sets
`PORTABLE_EXECUTABLE_DIR` to the directory the exe was double-clicked from. If
that variable is set, `userData` is redirected to `<exe dir>\helm-data\`;
otherwise it is `app.getPath('userData')`, which is `%APPDATA%\Helm`.

`process.execPath` in portable mode points at the temp extraction, so **never
derive an app path from execPath.** Use `PORTABLE_EXECUTABLE_DIR` or
`app.getPath('userData')`.

The check drivers use the same mechanism for isolation: they start the app with
`PORTABLE_EXECUTABLE_DIR` pointed at a directory of their own, so a check runs
against a real profile layout without touching the one somebody is using.
`pnpm dev` does too, at `%LOCALAPPDATA%\Helm\dev` - `productName` is `Helm`, so
an unpackaged run with no such directory resolves `userData` to `%APPDATA%\Helm`
and shares the installed app's database, shims and Chromium profile. That is
`pnpm dev:live`, which is the one mode that does it on purpose.

`appMode` therefore reads **two** signals, not one: `app.isPackaged` says build
or checkout, and `PORTABLE_EXECUTABLE_DIR` says whether the data is its own -
`installed`, `portable`, `dev`, `dev-live`.

**Harness templates take the same branch.** `templatesDir` reads the same
variable: set, they live in `<exe dir>\helm-data\templates`, so **a portable
install keeps its templates on the stick and leaves nothing on a machine it is
plugged into**; unset, they live in `~/.config/helm/templates` rather than under
`%APPDATA%`, because a template is a directory somebody authors by hand and
likely keeps in git. One rule, four run modes, and no second override
mechanism - `pnpm dev` and every check get a templates directory of their own
for free, because both already set that variable for the database.

The shipped `README.md` and example template are written on a start that finds
the directory **absent**, and nothing there is ever overwritten. Helm records no
hashes of what it seeded, so it cannot distinguish an edited file from an
untouched one; deleting the directory is what puts the originals back.

Where each run keeps its data, on each OS (`appMode` in `paths.ts` decides):

| run | Windows | macOS | Linux |
|---|---|---|---|
| installed | `%APPDATA%\Helm` | `~/Library/Application Support/Helm` | `${XDG_CONFIG_HOME:-~/.config}/Helm` |
| portable | `helm-data` beside the exe | none | none |
| `pnpm dev` | `%LOCALAPPDATA%\Helm\dev\helm-data` | `~/Library/Application Support/Helm Dev/dev/helm-data` | `${XDG_DATA_HOME:-~/.local/share}/Helm/dev/helm-data` |
| checks | `%LOCALAPPDATA%\Helm\checks\<name>\helm-data` | `~/Library/Application Support/Helm Dev/checks/<name>/helm-data` | `${XDG_DATA_HOME:-~/.local/share}/Helm/checks/<name>/helm-data` |
| `pnpm dev:live` | the installed app's own | the installed app's own | the installed app's own |
| templates | `~/.config/helm/templates`; `helm-data/templates` when portable | `~/.config/helm/templates` | `~/.config/helm/templates` |

Templates are `helm-data/templates` in the run's own folder for `pnpm dev` and
every check, on every OS.

## What the installer does, and what it deliberately does not

- **Per-user**, into `%LOCALAPPDATA%\Programs\Helm`. Nothing Helm does needs
  administrator rights, and an installer that asks for them cannot be run on a
  machine the user does not own. `allowElevation: false` is set explicitly, so a
  config change that *would* need elevation fails the install rather than
  quietly putting a UAC prompt in front of everyone.
- **Uninstall removes the program and leaves the data.**
  `deleteAppDataOnUninstall` stays false: `%APPDATA%\Helm\helm.db` holds the
  user's profiles, their session index and every config snapshot Helm has taken
  on their behalf, and uninstalling the program is not a request to destroy
  those. `pnpm verify:installer --yes` asserts the install directory,
  the start-menu shortcut and the uninstall registry entry are gone and that the
  data directory is not.
- **No desktop shortcut.** A start-menu entry is enough.
- **It stops this PC's local host first, after saying what that ends.**
  `floti.exe` runs the sessions and can outlive Helm, and a running
  `floti.exe` cannot be replaced. electron-builder's
  own check would force-stop every process under the install folder, the host
  included, ending its sessions without a word. So `build/installer.nsh`
  (`nsis.include`) defines `customCheckAppRunning`, which, when a Helm with a
  `resources\floti.exe` is installed, runs the floti.exe **this installer
  carries** before anything is replaced. The installer embeds
  `dist-host\floti.exe`, the same file it installs, and writes it to its own
  temporary folder, so the first update over a Helm whose floti.exe predates
  `update-stop` asks too.
  `floti update-stop "%APPDATA%\Helm\host-client" --ask` connects with Helm's
  client folder keys, as `floti bridge` does, so the device key never leaves
  that process, and it never pins a host. When the host runs sessions it
  prints the question ("Installing this update stops the local host on this
  PC, ending 2 sessions. Helm offers them back when it next starts.") and the
  installer shows it with OK and Cancel. Cancel quits the installer and every
  session keeps running; OK runs `--stop`, which stops the host for an update
  (`HostStopReason::Update`: the sessions end with their rows left running,
  so Helm's next start offers them back once) and returns once the host has
  exited. A host running no session (status 7) is stopped the same way with
  no question. Anything else asks nothing and leaves the host to the default
  check: no `floti.exe` installed yet, no host answering (status 3), a client
  folder that has pinned no host yet (status 4) or a host it cannot talk to. Then electron-builder's own check runs, which the macro
  still inserts, with the `getProcessInfo.nsh` include and `Var pid` that
  defining the macro drops. A silent install (`/S`) answers OK, as that check
  does. The uninstaller runs the default check alone. On macOS and Linux files
  are replaced under a running host and nothing stops it. The decision is
  tested in `crates/host` (`update_stop.rs` and `tests/update_stop.rs`);
  nothing in the suite runs an installer.

## Releasing

**A release is a version bump merged to `main`.** Nothing else: no tag pushed by
hand, no button to press, no artefact built on a developer's machine.

`.github/workflows/release.yml` reads the version out of
`packages/desktop/package.json` and, if `v<version>` is not already a tag on the
remote, runs the checks, builds floti for its five targets, builds the Windows
artefacts on a clean `windows-latest` runner, the macOS ones on `macos-latest`
and the Linux one on `ubuntu-latest`, packs the protocol types, signs every
file, and publishes one GitHub release with twenty-four files attached: the
five desktop files, the protocol types, the five floti files and their SHA-256
list, and a minisign signature beside each of those twelve. A stable release
then updates the Homebrew formula and opens the winget pull request ("Homebrew
and winget"). A merge that does
not bump the version runs the checks and the floti builds, skips every build
after them and finishes green.

| Job | Does |
|---|---|
| `version` | The gate (`scripts/release.mjs gate`): the version, its tag, whether it is a preview, whether the build runs and whether this is a dry run. |
| `check`, `floti` | `ci.yml` and `floti.yml`, called. |
| `protocol` | The crate's `generated_files` test, then `floti-protocol-<version>.tar.gz`. |
| `release-windows`, `release-mac`, `release-linux` | One desktop build each, verified from the inside and handed on as a workflow artifact. None publishes anything. |
| `assemble` | Gathers every file into one folder, composes the notes, checks floti's SHA-256s and that the folder holds exactly the twelve files to sign (`scripts/release.mjs check --unsigned`), and uploads them as `release-unsigned`. |
| `sign` | The only job that reads the signing key, in the `release` environment (a prerequisite: "The release environment"). Signs every file, checks every signature against `crates/host/src/update/minisign.pub`, lists the twenty-four files with their SHA-256s in the run's summary and uploads them as `release-files`. Never runs in a dry run. |
| `sign-dry-run` | A dry run's `sign`: the same signing and checks, with a key pair made in the run. Runs only in a dry run. |
| `publish` | The only job with write access. Attests every file in `release-files`, then creates the release from them, publishes it and confirms it. Never runs in a dry run. |
| `homebrew`, `winget` | After `publish`, for a stable release only: push `Formula/floti.rb` to floti-dev/homebrew-tap and open the winget pull request ("Homebrew and winget"). Never run for a preview or in a dry run. |
| `homebrew-dry-run`, `winget-dry-run` | A dry run's formula and manifests, built from the run's files and checked by Homebrew and winget. Run only in a dry run. |
| `sdk` | The plugin SDK's npm publish, on its own version. Never runs in a dry run. |

- **floti is built once, by `floti.yml`.** Every desktop build needs it, so a
  failed floti build on any target skips the release.
- **Each desktop build is its own job and publishes nothing.** A failed build
  on any OS skips `assemble` and `publish` with it: a version is never
  published with half its downloads. Every runner is a standard GitHub-hosted
  one, and no certificate or paid account is involved: the Mac build is
  ad-hoc signed and the Windows build unsigned. There are three secrets, each
  read by one job: the minisign key by `sign`, the tap's deploy key by
  `homebrew` and the winget token by `winget`.
- **The gate is whether the tag exists**, asked with `git ls-remote --tags`, not
  whether the commit changed the version. Diffing against the parent gives a
  different answer after a squash, a rebase, a revert or a re-run.
- **Releases only move forward.** A version that is not tagged yet must be
  greater, by semver with pre-release parts compared as semver orders them,
  than the highest `v*` tag; the gate fails otherwise, naming that tag. The
  desktop's numbering continues: the version is the desktop's, and floti's is
  the same (`crates/host/build.rs`). The gate also refuses a version that is
  not `<major>.<minor>.<patch>` with an optional `-<pre-release>` part, which is
  what the in-app check can read.
- **`ci.yml` does not run on pushes to `main` on its own.** `release.yml` calls
  it through `workflow_call`, so `main` is checked once per push rather than
  twice. It runs `pnpm check` (typecheck, lint, unit and integration tests)
  and then builds, on Windows, macOS and Linux runners, each its own job; the
  end-to-end tests run locally only. All three jobs are required: a failure on
  any one fails `check`, which skips every build, the release and the SDK
  publish: no tag, no release, no artefacts.
- **The release is created as a draft and published in the same job**, which is
  not an approval gate. Publishing the draft is what creates the tag, so the
  ref and all twenty-four assets appear together rather than leaving a window where
  the app announces a version whose download page is empty. A run cancelled
  mid-upload leaves an unpublished draft; the next run deletes it and starts
  over.
- **A preview is a public pre-release.** A version with a pre-release part
  (`2.4.1-preview.12`) is built, published and tagged exactly like a release,
  so anybody can download it without a GitHub sign-in, but as a pre-release and
  never `latest`. `/releases/latest` ignores pre-releases, so no installed Helm
  is offered it. The in-app check reads only the `<major>.<minor>.<patch>`
  part, so a preview of 2.4.1 is newer than 2.4.0 and older than the next
  release after it.

- **Notes are the `## <version>` section of [CHANGELOG.md](../CHANGELOG.md)**,
  and **a release with no section for its version fails rather than
  publishing.** No tag is created when that job fails, so the remedy is to write
  the section and push again. GitHub's generated notes list merged pull
  requests, and this repository's history is branches merged locally, so the
  generated body would be a "Full Changelog" link and nothing else.
- **Why it is written rather than derived.** Through 0.4.0 the notes were
  `git log` subjects, on the argument that they are already one-line statements
  of what landed. They are - of what landed in the *repository*. 0.4.0 produced
  fifty-nine lines, most of them naming a probe id, a fixture or a refactor, and
  three identical lines from one cherry-pick onto three branches. Filtering by
  path removes some and cannot remove the rest: the same file is touched for a
  reason somebody using Helm sees and for a reason they never will, and only the
  person who wrote the commit knows which. The changelog is for them; the
  commits are one click away under "Full changelog".
- **A release page is a changelog and nothing else.** Install notes, the
  SmartScreen warning and first run are the README's, one click away and
  maintained once rather than restated at every tag.
  `.github/release-body.md` is what gets prepended to the changes and is
  deliberately empty; it stays as a seam, so a release that genuinely needs a
  sentence at the top can have one without editing release logic.
- **Artefacts are verified before upload, from the inside.**
  `pnpm verify:artifact` unwraps the NSIS exe and the `app-64.7z` nested in it
  and asserts the win32-x64 `better-sqlite3` and `node-pty` binaries are present
  under `app.asar.unpacked`. What it guards against is an exe built with the
  wrong package-manager collector, which carries no `app.asar.unpacked` and dies
  on its first `dlopen`. Checking `dist-app/win-unpacked/` instead would check
  electron-builder's scratch directory rather than the file a person downloads.
  It runs on any exe: `pnpm verify:artifact -- path/to/Helm-x.y.z-setup.exe`.
  The Mac artefacts get `verify-mac-artifact.mjs` and the AppImage
  `verify-linux-artifact.mjs`, described above.
- **The confirmation expects all twenty-four files.** After publishing a
  release, `publish` asks `/releases/latest` - the endpoint the in-app check
  asks - and fails unless it names this tag, is not a draft, and lists exactly
  the files `scripts/release.mjs names` gives, signatures included. A preview
  must be published, a pre-release, carry the same twenty-four and not be what
  `/releases/latest` answers.
- **Every version downloads without a sign-in.** For a release and a preview
  alike, `publish` then fetches the setup exe and `floti-<version>-linux-x64`
  from `releases/download/<tag>/` with no token at all, and fails unless each
  answers 200 with the bytes the run built: the desktop app as a new user
  fetches it, and floti as Settings > Hosts has a computer of another OS or CPU
  fetch it. `floti-<version>-linux-x64.minisig` is fetched the same way, as
  `floti update` fetches a signature.

### Signatures and attestations

Every file a release carries - the five desktop files, the protocol types, the
five floti files and their SHA-256 list - has a minisign signature beside it,
`<file>.minisig`, and a GitHub build attestation.

- **The trusted comment is exactly `floti <version> <file>`**, for example
  `floti 3.0.0 floti-3.0.0-linux-x64`. minisign signs the comment along with
  the file, so a genuine signature cannot be moved to another file or another
  version. `floti update` refuses a signature with any other comment.
- **Signatures are prehashed** (algorithm `ED`), minisign's default since
  0.10. `floti update` accepts no other kind, and the workflow refuses a
  signature of any other kind before publishing.
- **The public key is `crates/host/src/update/minisign.pub`**, built into
  floti. It is the only key `floti update` trusts.
- **The secret key is kept only in the `release` environment**, as the secret
  `MINISIGN_SECRET_KEY`: the whole minisign secret key file, made with
  `minisign -G -W`, so it has no password. `sign` is the only job that names
  it. That environment, admitting `main` alone, is a prerequisite the owner
  creates before setting the secret ("The release environment" below), and no
  repository secret of that name may exist. SECURITY.md says how the key is
  handled in the run.
- **Every signature is checked before anything is published.** `sign` checks
  each one with minisign itself against the committed public key, trusted
  comment included (`scripts/minisign.mjs verify`). A release fails there,
  with nothing tagged or published, when that file is missing, when the secret
  is missing, or when any signature does not verify against it.
- **minisign is pinned**: version 0.12, its Linux x64 archive checked against
  the SHA-256 written in `.github/actions/minisign/action.yml`.
- **The signing is reusable.** `scripts/minisign.mjs sign --comment <comment>
  <file>...` signs any file with the key in `MINISIGN_SECRET_KEY`, `{name}` in
  the comment standing for each file's name; `verify` checks the same way. The
  update manifest is signed through it with the comment `floti update.json`.
- **Attestations** come from `actions/attest-build-provenance`, pinned to a
  commit, in `publish` before the release is created: one attestation naming
  every file, signatures included. `publish` is the only job with
  `attestations: write`. A dry run makes none.

To check a file by hand, with minisign and the public key's second line (the
one starting `RW`):

    minisign -Vm floti-3.0.0-linux-x64 -P <public key>

or with the key file itself, `minisign -Vm <file> -p minisign.pub`. It prints
the trusted comment, which must be `floti <version> <file>`. To check the
attestation, with the GitHub CLI:

    gh attestation verify floti-3.0.0-linux-x64 --repo floti-dev/floti

### The release environment

The signing key lives in the repository's `release` environment, and the
environment has to exist, admitting `main` alone, before the key is put in
it. Neither is in the repository; the owner sets them up once, in this order,
with the GitHub CLI:

    # 1. The environment, with a deployment branch policy that admits main alone.
    gh api -X PUT repos/floti-dev/floti/environments/release \
      -F 'deployment_branch_policy[protected_branches]=false' \
      -F 'deployment_branch_policy[custom_branch_policies]=true'
    gh api -X POST repos/floti-dev/floti/environments/release/deployment-branch-policies \
      -f name=main -f type=branch

    # 2. Only then, the key, as that environment's secret: the whole file
    #    `minisign -G -W` wrote, so it has no password.
    gh secret set MINISIGN_SECRET_KEY --env release --repo floti-dev/floti <minisign.key

The order matters: a secret set in an environment that does not limit its
branches yet could be read by a job on any branch. **A repository secret
named `MINISIGN_SECRET_KEY` must never exist**: every job could read it, a
dry run's included. `gh secret list --repo floti-dev/floti` must not list it;
`gh secret list --env release --repo floti-dev/floti` does. Until both steps
are done, a real release fails at `sign`, before anything is published.

### Homebrew and winget

A stable release also goes to two package managers, with the floti binary
only: the desktop app is not in either.

- **Homebrew**: the formula `floti` in floti-dev/homebrew-tap, for macOS
  arm64 and Linux x64 and arm64, installed with

      brew trust floti-dev/tap && brew install floti-dev/tap/floti

  Homebrew refuses to load any formula from a third-party tap until the user
  trusts that tap, so `brew install` alone fails with "Refusing to load
  formula ... from untrusted tap". This is the line the install page gives.
  There is no cask for the desktop app until it is signed: Homebrew
  quarantines casks. The formula's test runs `floti version`.
- **winget**: the package `FlotiDev.Floti`, installer type portable, run as
  `floti`, with the Windows x64 and arm64 exes.

Both are written by `scripts/package-managers.mjs` from the release's own
download URLs and its SHA-256 list. The `homebrew` and `winget` jobs fetch
that list from the published release with no token, as anybody does, and check
its signature against `crates/host/src/update/minisign.pub` before using it.
A preview updates neither: Homebrew and winget users get stable releases.

- **`homebrew`** clones the tap over HTTPS, writes `Formula/floti.rb`, commits
  it as `floti <version>` and pushes it with the deploy key, one push. A
  formula that already says this release is left alone.
- **`winget`** brings the fork's `master` up to date with
  microsoft/winget-pkgs, makes one commit holding the three manifests under
  `manifests/f/FlotiDev/Floti/<version>/` on the branch
  `FlotiDev.Floti-<version>`, and opens the pull request, titled "New package"
  the first time and "New version" after. It uses gh and GitHub's API, not an
  action: winget-releaser and komac update a package winget already has from
  their own reading of the release, so neither can submit the first version,
  and both would write manifests of their own rather than these.

**Both run after `publish`, so a release is already published when either
fails.** While a prerequisite below is missing, its job fails with one line
naming what to create. Create it, then re-run the failed jobs from the run's
page: a re-run pushes the formula only if it changed, moves the winget branch
to a fresh commit and reuses a pull request already open.

The owner creates these once, in this order, each secret in the `release`
environment ("The release environment"), never as a repository secret:

    # 1. The tap: a public repository with a main branch.
    gh repo create floti-dev/homebrew-tap --public --add-readme \
      --description "Homebrew formula for floti"

    # 2. Its deploy key, with write access; the private half is the secret.
    ssh-keygen -t ed25519 -N "" -C "floti release" -f homebrew-tap-key
    gh repo deploy-key add homebrew-tap-key.pub --repo floti-dev/homebrew-tap \
      --allow-write --title "floti release workflow"
    gh secret set HOMEBREW_TAP_DEPLOY_KEY --env release --repo floti-dev/floti <homebrew-tap-key
    rm homebrew-tap-key homebrew-tap-key.pub

    # 3. A fork of winget-pkgs in a personal account (an organisation's fork
    #    cannot let winget's maintainers edit the pull request), and a classic
    #    token of that account with the public_repo scope, made at
    #    https://github.com/settings/tokens/new, as the secret.
    gh repo fork microsoft/winget-pkgs --clone=false
    gh secret set WINGET_TOKEN --env release --repo floti-dev/floti

The job finds the fork as `<the token's account>/winget-pkgs`. The first pull
request asks that account to accept Microsoft's contributor licence agreement,
once, by commenting `@microsoft-github-policy-service agree` on it; winget's
reviewers merge it after that. `gh secret list --env release --repo
floti-dev/floti` lists `HOMEBREW_TAP_DEPLOY_KEY` and `WINGET_TOKEN` beside
`MINISIGN_SECRET_KEY`, and `gh secret list --repo floti-dev/floti` lists none
of them.

### A dry run

A push to a branch named `release-dry-run/<name>` runs the whole workflow and
publishes nothing: no tag, no release, no attestation, no npm, no tap push and
no winget pull request. Everything a
release builds is built and checked, including the notes, the signatures and
every check of the files that needs no published release. `sign-dry-run` makes
a minisign key pair in the run (`minisign -G -W` under `$RUNNER_TEMP`), signs
every file with it through the same `scripts/minisign.mjs` a release uses,
checks every signature against that pair's public key and uploads exactly the
twenty-four files as the run's `release-files` artifact, kept for seven days.
The run's summary lists each with its SHA-256 and gives the run's public key.
`homebrew-dry-run` and `winget-dry-run` build the formula and the winget
manifests from the run's SHA-256 list, as a release would, preview or not.
The formula goes through `brew style` and `brew audit --strict` in a tap of
its own on the runner, trusted with `brew trust` as a user's is, then `brew install` and `brew test` on macOS arm64 and
Linux x64, with the run's own file put in Homebrew's download cache so the
formula installed is the very text a release would push. The manifests go
through `winget validate`. They are kept for seven days as the
`homebrew-formula` and `winget-manifests` artifacts. A dry run never reads the
`release` environment or its secrets, and makes no attestation. The tag-exists gate does not stop a dry run, so
the version on the branch may be one already released; one that is not tagged
must still be greater than every tagged one. A run on any branch but `main` is
a dry run, and the version job's `dry_run` output is what every publishing job
and step gates on. `ci.yml` and `floti.yml` skip those branches, as they skip
`main`, so a dry run is checked once.

    git push origin HEAD:release-dry-run/<name>

## Looking at the Mac build

Nobody working on Helm has a Mac in front of them, so the
**macOS build and screenshots** workflow
(`.github/workflows/macos-screenshots.yml`) builds and photographs it on
`macos-latest`. It runs on a push to any branch but `main` that changes how the
Mac build is made or photographed - `electron-builder.yml`, `build/`,
`packages/desktop/package.json`, `verify-mac-artifact.mjs`, the `afterPack`
hook and the spawn-helper script it shares with the postinstall, the screenshot run,
the E2E launch and world it uses, or the workflow itself - so a branch shows
its dmg and zip build before it merges rather than at release time. It can
also be started by hand from the Actions tab, or with
`gh workflow run macos-screenshots.yml --ref <branch>` once the file is on
`main`.

It runs `pnpm dist:mac` and `verify-mac-artifact.mjs` over the dmg and the zip
exactly as `release-mac` does, then launches the built app through the E2E
tests' Playwright launch into a world with the fake claude, and photographs
every destination on the rail and a session terminal in the light and the dark
theme. The pictures are the run's `helm-macos-screenshots` artifact: each view
as the page and, on macOS, as the window with its traffic lights. It is no
check in the gate and nothing waits on it. `pnpm screenshots` runs the same
capture locally on any OS, into `reports/screenshots`.

## floti update

`floti update` replaces the floti that runs it with its channel's newest
release, and the desktop app where it is installed and older, once the app
has quit (`crates/host/src/update`). A person runs it; nothing runs it on a
timer.

**The manifest.** It reads `https://floti.dev/update.json` and its signature
`update.json.minisig`:

```json
{"schema": 1,
 "channels": {"stable": {"version": "2.6.0",
                         "url": "<release download folder>/",
                         "floti":   {"<os>-<cpu>": {"file": "...", "sha256": "..."}},
                         "desktop": {"<os>-<cpu>": {"file": "...", "sha256": "..."}}}}}
```

Targets are named as floti's release files are: `windows-x64`,
`windows-arm64`, `macos-arm64`, `linux-x64`, `linux-arm64`. Fields it does
not know are ignored; a schema other than 1 is refused. The folder must be
`https` and end in `/`, and each file a plain file name, so a manifest can
name nothing outside its release folder. Only `https` is fetched, redirects
included; a test build alone also fetches `http` from this computer, where
its tests serve a release.

**The channel** is `stable` until `floti update --channel <name>` chooses
another. The choice is remembered in `update-channel.json` in Floti's data
folder (`%LOCALAPPDATA%\Floti`, `~/Library/Application Support/Floti`,
`~/.local/share/floti`), once the manifest has been checked and has that
channel; a channel it does not have is an error that names it and the ones
it has.

**The checks.** Each file `F` downloads from `<url>F`, with its signature
from `<url>F.minisig`, which is fetched first. A signature must verify with
the minisign public key built into the binary
(`crates/host/src/update/minisign.pub`, embedded when floti is built), and
its trusted comment must be exactly `floti update.json` for the manifest and
`floti <version> F` for a release file, so a signature cannot be moved to
another file or version. Only minisign's pre-hashed signatures (its default)
are accepted. A release file is then checked against its SHA-256 in the
manifest. Both checks are made on the file as it lies on disk. A release
build has no switch, variable or flag that changes the key; only a test
build (the crate's testkit) reads a test's key and manifest address from a
marker in its data folder.

**What it replaces.** floti, when the channel's version is newer than the
running one; a version older than or equal to it is never installed, and a
floti whose own version is not a version updates nothing. A floti inside the
desktop app's install (`resources\floti.exe` on Windows, inside `Helm.app`
on macOS, on the AppImage's mount on Linux) is never replaced on its own:
the app's installer brings its own, and `floti update` says so and names
floti installed on its own where it is there. The desktop app, when it is
installed and its own version is older than the channel's or cannot be told,
so an app behind an up-to-date floti is still updated. With nothing to
replace, it says floti is up to date and changes nothing.

**Asking first.** On Windows the app's installer stops this computer's host
before it replaces anything (`installer.nsh`), which ends the sessions it
runs. So before it downloads the app, `floti update` asks the host, as the
host itself, how many sessions it runs (as `floti update-stop --ask` does).
With one or more it says how many end and asks `[y/N]` on the terminal, no
by default; no leaves the app as it is and updates floti alone. With no
terminal on standard input it leaves the app and says why in one line, with
what to run. The installer is started out of any job `floti update` is in,
so it finishes even when stopping the host ends the session that ran it.
This question stands until an update hands sessions through the keeper.

**The order.** Nothing is replaced until everything has passed: the
manifest, this computer's floti (written beside the running one under a
name of its own) and the app's file (kept in `update` in the data folder). A
failed check stops the update with one line on standard error naming the
file and what failed and that nothing was updated, removes what was
downloaded, and leaves the old floti working. Then floti is replaced, then
the app: while it runs, `floti update` says it is waiting for it to quit and
looks again each second. What an earlier update left beside floti
(`floti.old-<pid>.exe`, `.floti-<version>.<pid>`) or beside the app (a new
or old copy, `.Helm.app.floti-update-<pid>` or `.Helm.app.floti-old-<pid>`)
is removed at the start of the next, once the process that left it is known
to have ended.

**Where things are replaced** (`crates/host/src/platform/installs`):

| | floti | the desktop app | its version |
|---|---|---|---|
| Windows | The running `floti.exe` is renamed to `floti.old.exe` and the new one moved into its name, as `install.ps1` does; the next update removes the old one. | The per-user NSIS install, `%LOCALAPPDATA%\Programs\Helm` (`Helm.exe` beside `Uninstall Helm.exe`), updated by running the release's setup.exe with `/S`. | `DisplayVersion` of the per-user uninstall entry whose `UninstallString` is that install's uninstaller. |
| macOS | Written beside, made executable, renamed over. | `Helm.app` in `~/Applications`, else `/Applications`: the release's zip is unpacked beside it with `ditto -x -k --noqtn` and swapped in by rename. | `CFBundleShortVersionString` in its `Info.plist`. |
| Linux | Written beside, made executable, renamed over. | The AppImage at `~/Applications/Helm.AppImage` (AppImage's own tools look there, and it is outside the data folder `floti uninstall` keeps), where the desktop's first install puts it: written beside, made executable, renamed over. | Unknown, so it is always updated. |

A release with no desktop file for this computer updates floti alone and
says so. No downloaded file carries a quarantine flag or a
download mark: floti's own process writes them, which neither Gatekeeper's
quarantine nor Windows' mark of the web applies to, and `--noqtn` keeps
`ditto` from carrying one over from the archive.

## Updates: an explicit check, not an updater

`update:check` asks the GitHub releases API for the newest tag, compares it
against `app.getVersion()` and returns a URL. It downloads nothing and executes
nothing; upgrading is something the user does from the releases page.

It happens two ways and no others, and never on a timer. The app asks on its own
at launch, at most once a day (`UPDATE_CHECK_EVERY_MS`), when `updateCheck` is
on; and a person asks by pressing **Check now** in Settings → Updates, which
keeps no throttle of its own and works with `updateCheck` off. A deliberate act
that silently did nothing would be worse than no button, and the bound that
earns the app the right to ask by itself is the launch throttle - which a manual
check therefore does not stamp. Either way a newer release is reported beside
the version in the status bar. Offline is an expected answer, reported as "could
not ask", with the reason, rather than silently as "up to date".

There is deliberately no electron-updater. Three reasons, any one of which would
be enough:

1. **The build is unsigned.** electron-updater's NSIS path downloads a
   replacement installer and runs it. An unsigned replacement puts a SmartScreen
   prompt in front of *every* update, and an update mechanism whose happy path
   is a scary dialog teaches people to dismiss scary dialogs.
2. **It could only cover half the users.** The portable exe has no install
   location to replace, so electron-updater applies to the NSIS build alone.
3. **The app hosts long-lived sessions.** A background updater that restarts
   the local host ends somebody's `claude` session, possibly mid-turn. An
   update stops the host only once the user has agreed to the count of
   sessions it ends (see what the installer does, above).

Code-signing the build would retire reason 1. It would not touch 2 or 3.

## Network posture

**Helm contacts nothing on its own initiative except the update check.
Everything else on the network happens because you asked for it: the browser
pane fetches the page you navigate to, a plugin you added reaches the hosts its
manifest names, and a host you added on another computer is reached through
your own ssh, or the command you gave.**

There is no telemetry, no crash reporting, no fonts and no CDN. With
`updateCheck` off, Helm asks nothing by itself at all; the one remaining route
is a person pressing Check now, so nothing leaves the machine unasked for.

**Helm listens only on this machine.** Its windows reach the local host over
the host's local socket, which only your OS account can open. The local host
listens on `127.0.0.1` for its sessions' agent tools, behind a token minted
for each session and revoked when it ends, and holds no port while no tool
family is on. Helm's own process listens on `127.0.0.1` only for a port you
forwarded from a session on another computer, and closes it when that session
ends.

The tools are the browser pane's (`helm-browser`), the session-awareness tools
(`helm-sessions`), the child-session tools (`floti-agents`) and each
plugin's that offers them, served as named MCP
servers on routes of one endpoint (`crates/host/src/agent_tools`). Every
request carries a bearer token minted for one session and revoked when that
session ends; a request with a non-loopback `Origin` is refused before its
token is compared; and each built-in set has its own setting (`browserMcp`,
`sessionMcp`, `sessionAgents`) that removes it from every launch. The host has no browser, so
it hands each browser call to the attached Helm window, whose main process
answers it in the pane. A forwarded port is opened only when the user chooses
Open in browser on a port a session on a host on another computer listens on:
main binds `127.0.0.1`, on the same number when it is free and another free
one when not, and carries each connection over its own link to that host,
which connects only to its own loopback and only for a port that session's
process tree listens on. Worth knowing for a packaged build in particular: an
installer that a firewall prompt follows is a bad first impression, and a
loopback bind produces no prompt on Windows.

**Plugins reach the hosts their manifests name.** A plugin page has no network
of its own (`connect-src 'none'`); `helm.fetch` sends its requests from the
local host, only to an origin the manifest lists, following redirects one
hop at a time so that every hop is checked against the same list. A plugin's
background page may fetch on its own schedule, which is the plugin's doing
rather than Helm's - the user added it - and is why the sentence above says
so. A plugin never holds a secret: it writes `{{key}}`, and the host fills in
a value the user typed into Settings > Secrets, only for an origin and a
plugin the user allowed. Each value is sealed by this OS's store for the user
(DPAPI on Windows, the Keychain on macOS, the Secret Service on Linux), and
where none answers it refuses to store anything rather than fall back to
plaintext. The programs (`exec`) and the service a plugin declares are
ordinary processes with the user's rights, and their network is not confined;
Settings > Plugins names them per plugin, and turning the plugin off ends
them. A service listens on `127.0.0.1` on a port Helm gives it and is reached
with a token minted for the run - a listener of the plugin's, not Helm's, so
the inbound paragraph above still describes the app.

## The update check needs the repository to be public

`update.ts` sends `User-Agent` and `Accept` and nothing else - no token, because
Helm handles no GitHub credential. GitHub answers an unauthenticated request for a
**private** repository's releases with `404`, so while the repository is private
the check reports *"could not ask"* however current the release actually is.

This is a coupling that breaks silently: **making the repository private turns
the update check off**, and nothing in the app says so beyond the error string.
The same fact is why release artefacts cannot be handed to anyone who is not a
collaborator - a private release page is not a download link. It holds for a
draft too, whatever the repository's visibility, which is why a preview is
published as a pre-release rather than left a draft: the release workflow
fetches one desktop file and one floti file of every version with no token,
and fails unless both answer 200.

## Install-testing it

A check and a tool, and the split is the point: one is safe to run on the
machine you work on, and one is not. The unsafe one is not a group of any suite
and no `--only=` reaches it.

```bash
pnpm packaging-check --only=package   # the artefacts and the portable exe
pnpm verify:installer --yes           # installs and UNINSTALLS Helm here
```

`package` is in the default `pnpm packaging-check`. It builds the artefacts if
they are missing, checks the two native modules are unpacked beside the asar,
then copies the portable exe into a path with spaces, runs `--selftest` out of
it as the ordinary user, and checks the data landed beside the exe and that
`%APPDATA%\Helm` gained nothing. (That last read is dropped, and says so in the
report, when an installed Helm is running - it writes to `%APPDATA%\Helm` too,
so the comparison would be about two processes.)

**`installer` is opt-in and is the only destructive thing in this repository.**
It installs the NSIS package silently, runs `--selftest` out of the installed
app, checks it reports `installed` mode with `%APPDATA%\Helm` as its data
directory and that nothing landed beside the exe, then uninstalls and checks
what is gone and what is kept - and **puts nothing back**. It cannot be
isolated into its own data directory, because it is *about* where an installer
puts things.

It refuses outright if Helm is running from the install directory
(`--replace-running` overrides that, which is what a release machine says on
purpose). Helm hosts Claude Code sessions, so terminating it is not like
terminating an editor with everything saved - it ends the work inside them, and
it has done exactly that once, from a sweep of "every check".

So it is not part of the release gate, and neither is `packaging-check`: both
run only when the owner asks for them, a release that **changes packaging** -
electron-builder, `electron-builder.yml`, the native modules, `dist-win.mjs`, or
anything about where files land - included, and this one only on a machine
where nobody is working. The default run prints a `PKG-2` line recording that it did not,
so a green run is never mistaken for one that covered the installer, and CI
runs `verify-artifact.mjs` over both exes on every publish regardless.

The selftest itself (SPEC 8.2) is a SQLite WAL roundtrip, an interactive pwsh
through ConPTY, renderer-synthesized keystrokes, a resize verified inside the
shell, and the real `claude` TUI reaching its version banner. It spawns a real
session.

## License

MIT. `LICENSE` at the repository root.
