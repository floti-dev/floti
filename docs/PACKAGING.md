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
Windows, `Contents/Resources/floti` on macOS and `resources/floti` on Linux.
The installed app runs none of it there: that copy is only what is put in
place as floti installed on its own (see
[floti in your own folder](#floti-in-your-own-folder)), and the file
Settings > Hosts sends a host of this computer's OS and CPU. The portable exe
runs the copy inside it. A release build carries the release's own floti
instead of building it again (see [floti for every host](#floti-for-every-host)),
with the release's SHA-256 list beside it as `floti-SHA256SUMS`; a local build
carries no list. A release's Windows build also carries the release's floti
for Windows on arm64, as `resources\floti-arm64.exe`, beside the x64
`floti.exe`: `floti install-self`, run by the installer from the x64 one,
installs the arm64 one on a computer whose own CPU is arm64, so Windows on
Arm never gets an emulated floti from the installer. A local build carries
no arm64 copy. The installer and the portable exe unpack their payload with
electron-builder's nsis7z plugin, which is 7-Zip 19.00 and cannot read the
ARM64 filter newer 7-Zip gives ARM64 executables on its own; NSIS leaves a
file it cannot read out of the install without failing it. So
`scripts/dist-win.mjs` packs the payload with BCJ2 throughout
(`ELECTRON_BUILDER_7Z_FILTER`), and `verify-artifact.mjs` fails an artefact
with any file packed with a filter 19.00 cannot read.

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
and the list (`FLOTI_SHA256SUMS`), and the Windows job the Windows arm64 file
as well (`FLOTI_PREBUILT_ARM64`), which a Windows build given the list must
have. It refuses a file whose header is another platform's or whose SHA-256
the list does not give, stages the arm64 file as `dist-host/floti-arm64.exe`,
and stages the list as
`dist-host/floti-SHA256SUMS`, which electron-builder carries as
`resources\floti-SHA256SUMS` on Windows, `Contents/Resources/floti-SHA256SUMS`
on macOS and `resources/floti-SHA256SUMS` on Linux. The floti inside the app is
therefore byte for byte the one the release lists, and `release-windows`
compares both Windows copies with the release's files. On macOS that holds only
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

Settings > Hosts installs floti on another computer over the transport the
user gave it, and only after the user says yes to an offer that names the
computer's OS and CPU, the floti found there, and what will be done where. The app carries `scripts/install/install.sh` and `install.ps1` as
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

An added host whose floti is older than this desktop's gets the same Update
floti, in Settings > Hosts, on its sidebar header and on its Sessions page
row, whether or not this desktop can still talk to it (`checkUpdate` in
`packages/desktop/src/main/host/remote-install.ts`). Where the floti there
has `floti update` (protocol 3.1 and later, `hasFlotiUpdate`), the update is
that rather than an install, so it stops nothing first, and Update floti
starts it at once with no question: the row shows it running, its first line
naming the floti it replaces, the version it installs where the host's own
check found it (else the newest on its channel), and the sessions it ends
where it ends some (`flotiUpdateWorkingLine`). Only an install, which ends
the sessions there, says first what it does, Update from this desktop's
floti after an up-to-date outcome included.

- A connected host that keeps floti up to date (`update:state` names no
  reason it does not) and whose own check found a newer floti on its
  channel is asked over its connection with `update:install`, and the offer
  names that version. It runs `floti update --from-desktop`, which on a
  computer that also has the desktop app closes that app (keeping its
  sessions), replaces it, relaunches the host once and opens the app again.
  The row shows each step from `update:state`. `installFailed` is a
  failure. The update went well only once the host's link connects again on
  a newer floti; a host that comes back on the same one, or an update that
  ends its run while the connection outlives a grace, is said as not seen
  to replace floti.
- Any other host whose floti has it runs `floti update --floti-only` over
  the same login, its output shown as it comes. It leaves the desktop app
  there to that computer's own host, and never waits for it. Its status 4
  says floti was up to date on its channel: no failure, and the row offers
  an install from this desktop's floti instead.
- A floti without it, or a connected host that runs a floti it does not keep
  up to date or whose channel has nothing newer, is updated by the install
  above.

No wait on a `floti update` is unbounded: the row's Stop waiting, or ten
minutes (`UPDATE_PATIENCE_MS`), ends it, saying the update may still be
running there.

`floti update` relaunches the host through its session keeper. For an update
to this desktop's own version, the check first asks the host what a restart
would mean (`restartAsk` through a bridge of its own, as Restart host to
update asks this PC's), and the update's first line says how many sessions
end when the keeper does not speak the link this desktop's floti takes back.
For any other version, whose keeper link the desktop cannot know, it says
nothing of them.

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
likely keeps in git. The installed app is the exception: its host keeps its
own config folder, `~/.config/helm` where that is there, else
`~/.config/floti`, so its templates are in `templates` there, as a host started at
sign-in reads them. One rule, four run modes, and no second override
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
| templates | `~/.config/helm/templates` where Helm's folder is there, else `~/.config/floti/templates`; `helm-data/templates` when portable | the same, without portable | the same, without portable |

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
- **It puts floti where the install script does, and runs the same setup.**
  Once the app's files are in place, `build/installer.nsh` (`nsis.include`)
  runs `floti install-self` from the installed `resources\floti.exe`: floti
  and its Node go to `%LOCALAPPDATA%\Programs\Floti\bin`, that folder goes on
  the user's PATH, and `floti setup --no-desktop` runs from there
  ([floti in your own folder](#floti-in-your-own-folder) has each step). A
  failure there does not fail the install; it is in the details log, and
  Helm tries again when it starts.
- **It stops no host, and ends no session.** The host and its session keeper
  run from `%LOCALAPPDATA%\Programs\Floti\bin`, outside the install folder,
  so neither electron-builder's own check, which force-stops every process
  run from inside the install folder, nor the previous version's
  uninstaller, which does the same, reaches them. A running `floti.exe`
  there is renamed aside rather than replaced, and keeps running until
  Helm's Restart host to update hands its sessions to the new host through
  the keeper.
- **Once, it stops a host from inside the old install, after saying what
  that ends.** A Helm from before this ran its host from
  `resources\floti.exe` inside the install folder, and the previous
  version's uninstaller, which runs before any of this version's files are
  written, force-stops it and its keeper whatever this installer does. So
  `customCheckAppRunning` first checks whether that file is in use (it cannot
  be opened for writing while any process runs it), and only then runs the
  floti.exe **this installer carries**: it embeds `dist-host\floti.exe`, the
  same file it installs, and writes it to its own temporary folder, so an
  installed floti.exe that predates `update-stop` is asked too.
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
  check: no host answering (status 3), a client folder that has pinned no
  host yet (status 4) or a host it cannot talk to. Then electron-builder's
  own check runs, which the macro still inserts, with the `getProcessInfo.nsh`
  include and `Var pid` that defining the macro drops. A silent install
  (`/S`) answers OK, as that check does. The uninstaller runs the default
  check alone. The decision is tested in `crates/host` (`update_stop.rs` and
  `tests/update_stop.rs`), `install-self` in `tests/install_self.rs`;
  nothing in the suite runs an installer. The release's dry run runs it for
  real on Windows runners ([A dry run](#a-dry-run)).

### floti in your own folder

The installed app's floti is floti installed on its own, where the install
scripts put it: `%LOCALAPPDATA%\Programs\Floti\bin\floti.exe` on Windows,
`~/.local/bin/floti` on macOS and Linux. So a computer has one floti however
Floti came to it, the host started at sign-in is that one, and
`ssh <computer> floti version` finds it. `floti install-self`
(`crates/host/src/install_self.rs`), run from the floti the app carries,
puts it there:

1. The floti build for the computer's own CPU is copied beside the one there
   under a name of its own, then put in its place. On Windows on Arm that is
   `floti-arm64.exe` beside the running floti (the release's
   `floti-windows-arm64.exe`, which the installer carries as
   `resources\floti-arm64.exe` beside its x64 floti); the CPU is
   `IsWow64Process2`'s native machine, so an x64 floti emulated there still
   picks the arm64 build. Anywhere else, and where that file is not there,
   it is the floti that runs install-self. On Windows a running `floti.exe`
   there is renamed aside (`floti.old-<pid>-<n>.exe`) and keeps running; on
   macOS and Linux the new file is renamed over the old, which leaves a
   running process its own copy. A copy there that is the chosen build byte
   for byte is left as it is, so a native arm64 floti of the same version
   that `install.ps1` put there stays, and copies put aside earlier are
   removed once nothing runs them.
2. The Node runtime beside the carried floti goes, file by file the same
   way, into `node` beside the installed one, where the host looks for it. A
   `node` there that is not a folder (a file or a link of the user's own) is
   left as it is, and said so; plugin services of a host started from there
   then have no Node.
3. On Windows, when the folder is not on the user's PATH, it is appended to
   their `Path` value as written, kept an expandable string, and Explorer is
   told the environment changed, so a terminal or an ssh login started after
   it finds `floti`. On macOS and Linux floti's line goes at the end of the
   startup file the user's shell reads, as [floti setup](#floti-setup)
   adds it after the install script, and it says which file.
4. `floti setup --no-desktop` runs from the installed floti, which registers
   it to start the host at sign-in and starts it, and neither asks about nor
   installs the desktop app, which is there already. A host already running
   keeps running.

Who runs it:

- **Windows:** the NSIS installer, after the app's files are in place.
- **Every OS:** the installed app when it starts, before it reaches the local
  host (`src/main/host/local-floti.ts`), when the floti there is missing,
  does not say its version, or is older than the app. The dmg and the
  AppImage have no installer, so on macOS and Linux this is how it gets
  there; on Windows it puts back one that was removed. A floti there that is
  as new as the app or newer, such as one `floti update` replaced, is left as
  it is. When it cannot be put there the app runs the floti it carries, and
  its log says why.
- **The portable exe** installs nothing and runs the floti inside it, with
  its data beside it, as before.

The host's config folder is `~/.config/helm` on a computer where that is
there, which Helm set up before the rename and where its agents, templates
and plugins are, and `~/.config/floti` elsewhere; it does not change while
Floti runs there, and the one-time move to `~/.config/floti`, which the rename makes, is
what switches a computer over. The installed app gives its
host no config folder of its own, so the host it starts and the host started
at sign-in read the same one.

What it adds outside the install folder, which uninstalling the app leaves
and `floti uninstall` removes ([below](#floti-uninstall)), the host's data
only with `--purge`:

| What | Where |
|---|---|
| floti, copies of it put aside, its Node, and copies an install stopped part way left | `%LOCALAPPDATA%\Programs\Floti\bin\` (`floti.exe`, `floti.old-*.exe`, `node\` with any `node.old-*.exe`, `.floti.exe.floti-install-*`); `~/.local/bin/floti`, `~/.local/bin/node/` and `.floti.floti-install-*` there |
| The PATH entry | on Windows that folder, appended to `Path` in `HKEY_CURRENT_USER\Environment`; on macOS and Linux floti's line, ending `# added by floti`, at the end of the startup file the user's shell reads ([floti setup](#floti-setup)) |
| The start-at-sign-in entry | the logon task "Floti host" and its XML under the data folder's `startup\`; the LaunchAgent `~/Library/LaunchAgents/dev.floti.host.plist`; the user unit `~/.config/systemd/user/floti.service` |
| The host's data | `%LOCALAPPDATA%\Floti`, `~/Library/Application Support/Floti`, `~/.local/share/floti` and `~/.local/state/floti`, kept as Helm's data is |

The host and its session keeper run from that folder, so they outlive the
app's uninstaller, which stops only what runs from inside the app's own
install folder, and keep running until the user stops the host.

## Releasing

**A release is a version bump merged to `main`.** Nothing else: no tag pushed by
hand, no button to press, no artefact built on a developer's machine.

`.github/workflows/release.yml` reads the version out of
`packages/desktop/package.json` and, if `v<version>` is not already a tag on the
remote, runs the checks, builds floti for its five targets, builds the Windows
artefacts on a clean `windows-latest` runner, the macOS ones on `macos-latest`
and the Linux one on `ubuntu-latest`, packs the protocol types, signs every
file, and publishes one GitHub release with twenty-six files attached: the
five desktop files and their SHA-256 list (`Helm-<version>-SHA256SUMS`, which
`floti add desktop` checks a download against), the protocol types, the five
floti files and their SHA-256 list, and a minisign signature beside each of
those thirteen. A stable release
then updates the Homebrew formula and opens the winget pull request ("Homebrew
and winget"), and copies the install scripts and the update manifest to
floti.dev ("floti.dev"). A merge that does
not bump the version runs the checks and the floti builds, skips every build
after them and finishes green.

| Job | Does |
|---|---|
| `version` | The gate (`scripts/release.mjs gate`): the version, its tag, whether it is a preview, whether the build runs and whether this is a dry run. |
| `check`, `floti` | `ci.yml` and `floti.yml`, called. |
| `protocol` | The crate's `generated_files` test, then `floti-protocol-<version>.tar.gz`. |
| `release-windows`, `release-mac`, `release-linux` | One desktop build each, verified from the inside and handed on as a workflow artifact. None publishes anything. |
| `assemble` | Gathers every file into one folder, composes the notes, checks floti's SHA-256s, writes and checks the desktop files' SHA-256 list (`scripts/release.mjs desktop-sums`), checks that the folder holds exactly the thirteen files to sign (`scripts/release.mjs check --unsigned`), and uploads them as `release-unsigned`. Writes floti.dev's install scripts and, for a stable release, `update.json` (`scripts/floti-dev.mjs`) as `floti-dev-unsigned`. |
| `sign` | The only job that reads the signing key, in the `release` environment (a prerequisite: "The release environment"). Signs every file, and `update.json` with the comment `floti update.json`, checks every signature against `crates/host/src/update/minisign.pub`, lists the twenty-six files with their SHA-256s in the run's summary and uploads them as `release-files`, and floti.dev's files as `floti-dev`. Never runs in a dry run. |
| `sign-dry-run` | A dry run's `sign`: the same signing and checks, with a key pair made in the run. Runs only in a dry run. |
| `publish` | The only job with write access. Attests every file in `release-files`, then creates the release from them, publishes it and confirms it. Never runs in a dry run. |
| `homebrew`, `winget` | After `publish`, for a stable release only: push `Formula/floti.rb` to floti-dev/homebrew-tap and open the winget pull request ("Homebrew and winget"). Never run for a preview or in a dry run. |
| `floti-dev` | After `publish`, for a stable release only: commits `install.sh`, `install.ps1`, `update.json` and `update.json.minisig` to floti-dev/floti-dev.github.io ("floti.dev"). Never runs for a preview or in a dry run. |
| `packages-dry-run`, `homebrew-dry-run`, `winget-dry-run` | A dry run's formula (and its stable form) and manifests, built from the run's signature-checked SHA-256 list and checked by Homebrew and winget. Run only in a dry run. |
| `install-dry-run`, `install-dry-run-debian` | floti.dev's install scripts run for real against a local server holding the run's files, on Linux, macOS, Windows and in a Debian container ("A dry run"). Run only in a dry run. |
| `sdk` | The plugin SDK's npm publish, on its own version. Never runs in a dry run. |

- **floti is built once, by `floti.yml`.** Every desktop build needs it, so a
  failed floti build on any target skips the release.
- **Each desktop build is its own job and publishes nothing.** A failed build
  on any OS skips `assemble` and `publish` with it: a version is never
  published with half its downloads. Every runner is a standard GitHub-hosted
  one, and no certificate or paid account is involved: the Mac build is
  ad-hoc signed and the Windows build unsigned. There are four secrets, each
  read by one job: the minisign key by `sign`, the tap's deploy key by
  `homebrew`, the winget token by `winget` and the site's deploy key by
  `floti-dev`.
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
  ref and all twenty-six assets appear together rather than leaving a window where
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
- **The confirmation expects all twenty-six files.** After publishing a
  release, `publish` asks `/releases/latest` - the endpoint the in-app check
  asks - and fails unless it names this tag, is not a draft, and lists exactly
  the files `scripts/release.mjs names` gives, signatures included. A preview
  must be published, a pre-release, carry the same twenty-six and not be what
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
  formula ... from untrusted tap". `brew trust` accepts a tap that is not
  tapped yet, and `brew install` then taps it. This is the line the install
  page gives, and the dry run takes the same order.
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
- **`winget`** makes one commit holding the three manifests under
  `manifests/f/FlotiDev/Floti/<version>/`, on microsoft/winget-pkgs' `master`
  as it is at that moment, puts it on the fork's branch
  `FlotiDev.Floti-<version>`, and opens the pull request, titled "New package"
  the first time and "New version" after. The fork itself is never synced:
  syncing is refused without the `workflow` scope whenever winget-pkgs has
  changed its own workflows, and the token never gets that scope. A version
  already in winget-pkgs is left alone. It uses gh and GitHub's API, not an
  action: winget-releaser and komac update a package winget already has from
  their own reading of the release, so neither can submit the first version,
  and both would write manifests of their own rather than these.

**Both run after `publish`, so a release is already published when either
fails.** While a prerequisite below is missing, its job fails with one line
naming what to create. Create it, then re-run the failed jobs from the run's
page: a re-run pushes the formula only if it changed, moves the winget branch
to a fresh commit and reuses a pull request already open, and does nothing for
winget once the pull request is merged.

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

    # 3. A GitHub account kept for this alone: a member of no organisation and a
    #    collaborator on no repository, floti-dev's least of all. Signed in as
    #    that account, fork winget-pkgs into it (a personal fork, because an
    #    organisation's cannot let winget's maintainers edit the pull request)
    #    and make a classic token with the public_repo scope at
    #    https://github.com/settings/tokens/new. Then, as the owner of
    #    floti-dev/floti, set the token as the secret.
    gh repo fork microsoft/winget-pkgs --clone=false
    gh secret set WINGET_TOKEN --env release --repo floti-dev/floti

**The winget account must have no access to floti-dev.** A classic token's
`public_repo` scope is not limited to the fork: it can push to, and open or
merge pull requests in, every public repository its account can write to, and
change that account's own public repositories' settings. On an account that
can push to floti-dev/floti, whose `main` is what a release builds and signs,
a leaked `WINGET_TOKEN` would be as bad as a leaked signing key. On an account
that holds nothing but the fork it can do no more than push branches to that
fork and open pull requests as that account. Fine-grained tokens cannot open
a pull request in a repository their account does not own, so a classic
token it is.

The job finds the fork as `<the token's account>/winget-pkgs`. The first pull
request asks that account to accept Microsoft's contributor licence agreement,
once, by commenting `@microsoft-github-policy-service agree` on it; winget's
reviewers merge it after that. `gh secret list --env release --repo
floti-dev/floti` lists `HOMEBREW_TAP_DEPLOY_KEY` and `WINGET_TOKEN` beside
`MINISIGN_SECRET_KEY`, and `gh secret list --repo floti-dev/floti` lists none
of them.

### floti.dev

floti.dev is floti-dev/floti-dev.github.io, served by GitHub Pages from its
`main` over HTTPS. A stable release puts four files at its root:

| File | What it is |
|---|---|
| `install.sh` | `curl -fsSL https://floti.dev/install.sh \| sh` on macOS, Linux and WSL. |
| `install.ps1` | `irm https://floti.dev/install.ps1 \| iex` on Windows. |
| `update.json` | The manifest `floti update` reads ("floti update"), naming the `stable` channel alone. |
| `update.json.minisig` | Its signature, with the trusted comment `floti update.json`, by the project's key. |

**The install scripts** are `scripts/floti.dev/install.sh.template` and
`install.ps1.template`, filled in by `scripts/floti-dev.mjs scripts` in
`assemble`: the release's version, its download folder
(`releases/download/v<version>`) and each floti's SHA-256 from the release's
`floti-<version>-SHA256SUMS`. Each carries the install function of
`scripts/install/install.sh` or `install.ps1`, the one the desktop sends a
computer over SSH ("Installing floti on a host's computer"), so the steps are
the same code: floti goes into `~/.local/bin` or
`%LOCALAPPDATA%\Programs\Floti\bin` under a temporary name, is checked against
its SHA-256, and only then moved into place. A script:

1. picks the floti for the computer's OS and CPU: macOS arm64 (a shell under
   Rosetta counts as arm64), Linux x64 and arm64, WSL included, and Windows
   x64 and arm64, read from the system's own record of its CPU, so an x64
   PowerShell on an arm64 computer gets arm64. Any other pair is refused in
   one line naming it;
2. downloads it from the release folder and installs it, as above;
3. on Windows, appends the folder to the user's own `Path` as written, kept
   an expandable string, and tells Explorer the environment changed, as `floti
   install-self` does, and puts it on the session's `Path` too. On macOS and
   Linux the script changes no startup file itself: `floti setup`, next,
   adds floti's line to the one the user's shell reads and says which
   ([floti setup](#floti-setup));
4. runs `floti setup`, with the terminal as its input where there is one,
   never the rest of the download, so setup asks there whether to install the
   desktop app as well ([floti setup](#floti-setup)). Its lines and, from
   `install.sh`, its exit status are the script's.

Each script is one function called on its last line, so a script cut off
halfway runs nothing; the PowerShell one never exits, since `iex` runs it in
the user's own session. Both are printable ASCII, because Windows PowerShell
reads a download that names no charset as Latin-1. Neither has a switch: the
dry run makes copies that download from its local server through the same
template, with `--release-url`, which only http to this computer or https
passes.

**update.json** comes from `scripts/floti-dev.mjs manifest`, also in
`assemble`: the floti targets from the release's SHA-256 list, and the
desktop files `floti update` installs, hashed there: `windows-x64` the setup
exe, `macos-arm64` the Mac zip, `linux-x64` the AppImage. A preview writes
none (decision I13), so a channel other than `stable` appears only once it
is added there. `sign` signs it through `scripts/minisign.mjs` with the
comment `floti update.json` and checks it against the committed public key.
`crates/host/src/update/fixtures/update.json` is the manifest the script's
unit test writes for 3.0.0, compared byte for byte, and the host's
`manifest.rs` tests read that same file for every target.

**`floti-dev`** runs after `publish`, so the scripts never name a file that
is not downloadable yet. It fetches the SHA-256 list the release published,
with no token, and checks its signature against the committed key (the
`published-sums` action `homebrew` and `winget` use), then checks the four
files: exactly those four; each script, byte for byte, the one
`scripts/floti-dev.mjs check-scripts` writes from that list, so it carries
exactly the published SHA-256s; `update.json`'s signature against the
committed key, and (`check-manifest`) that it names this version alone, on
the stable channel, at the release's folder, with that list's floti files
and SHA-256s. A dry run makes the same checks in `packages-dry-run`, with its
own list and key. Then it clones the site over HTTPS, commits the four files
as `floti <version>: the install scripts and update.json`, one commit, and
pushes it with the deploy key, one push, with GitHub's host keys from its
API, `BatchMode=yes` and the key in an owner-only file removed when the step
ends. Files that already say this release are left alone. A push refused
because the site's `main` moved on since the clone is reported as that, and
a re-run clones again. That push to the site's `main` is what publishes
them; every other change there is the owner's, on a branch they merge.

Like `homebrew`, it fails after the release is published while its
prerequisite is missing, with one line naming it, and a re-run once it exists
finishes the work. `sign` keeps the four files as the run's `floti-dev`
artifact for 90 days, so the re-run can come long after the release. The
owner creates it once, in the `release` environment, never as a repository
secret:

    # The site's deploy key, with write access; the private half is the secret.
    ssh-keygen -t ed25519 -N "" -C "floti release" -f floti-dev-pages-key
    gh repo deploy-key add floti-dev-pages-key.pub --repo floti-dev/floti-dev.github.io \
      --allow-write --title "floti release workflow"
    gh secret set PAGES_DEPLOY_KEY --env release --repo floti-dev/floti <floti-dev-pages-key
    rm floti-dev-pages-key floti-dev-pages-key.pub

The site's Pages settings serve `main` from its root with HTTPS enforced, on
the domain `CNAME` names.

### A dry run

A push to a branch named `release-dry-run/<name>` runs the whole workflow and
publishes nothing: no tag, no release, no attestation, no npm, no tap push,
no winget pull request and nothing on floti.dev. Everything a
release builds is built and checked, including the notes, the signatures and
every check of the files that needs no published release. `sign-dry-run` makes
a minisign key pair in the run (`minisign -G -W` under `$RUNNER_TEMP`), signs
every file with it through the same `scripts/minisign.mjs` a release uses,
checks every signature against that pair's public key and uploads exactly the
twenty-six files as the run's `release-files` artifact, kept for seven days.
The run's summary lists each with its SHA-256 and gives the run's public key.
`packages-dry-run` builds the formula and the winget manifests from the run's
SHA-256 list as a release would, preview or not, after checking the list's
signature with the same `published-sums` action, against the public key of
the run's own pair. Beside them it builds the formula a stable release of the
same number would push, from the same list with the files renamed, so the
stable form is checked even when the branch's version is a preview.
`homebrew-dry-run` takes the documented install order on macOS arm64 and
Linux x64: `brew trust` on a tap not tapped yet, then that tap, made on the
runner, then `brew style` and `brew audit --strict` on both formulas, then
`brew install` and `brew test`, with the run's own file put in Homebrew's
download cache so the formula installed is the very text a release would
push.
`winget-dry-run` runs `winget validate` on the manifests. They are kept for
seven days as the `homebrew-formula` and `winget-manifests` artifacts.
floti.dev's four files are written from the run's own files as a stable
release writes them, `update.json` in the stable form when the branch's
version is a preview and signed with the run's key, and kept for seven days
as the `floti-dev` artifact. The install scripts run for real there and
nowhere else: `install-dry-run` on `ubuntu-latest`, `ubuntu-24.04-arm`,
`macos-latest`, `windows-latest` and `windows-11-arm`, so each CPU detection
runs on x64 and on arm64, and `install-dry-run-debian` as root in a `debian`
container, each serving the run's floti files and copies of the scripts made
to download from `http://127.0.0.1:8765` (`scripts/floti.dev/try-install.sh`
and `try-install.ps1`). Each cuts its script off at ten places, the last
just before its last line, and runs every piece with a home or
`LOCALAPPDATA` of its own, which must stay empty, with no floti file asked
for (and on Windows the user's `Path` unchanged). Then it runs the whole
script as a person does, `curl -fsSL <url>/install.sh | sh` or `irm
<url>/install.ps1 | iex` in Windows PowerShell, and checks that floti is
where the script put it, that the PowerShell session `iex` ran in went on
after it (a line it prints after `iex` must appear), that `floti version` there names the run's version,
OS and CPU, that on Windows its folder is on the user's `Path`, and that the
script reached `floti setup`, whatever setup made of the runner (root in the
container is refused by setup, which is the line it must reach). On macOS
and Linux `floti` must then be on `PATH`: for a shell floti writes a startup
file of, setup must have named the file it put floti's line in, or said
nothing where that shell's files had the folder already, and a new login
shell of the user's own kind (`$SHELL -l`), started with `env -i` and a
`PATH` of `/usr/bin:/bin` so that it has nothing of the job's, must find
that floti through its startup files alone; for a shell floti knows no file
of, or no `$SHELL`, setup must have said so; as root setup must have said
nothing of `PATH`. Two runs must show the line itself, so `try-install.sh`
gets `adds-path-line` there, which refuses "had the folder already": on
`macos-latest`, run with `SHELL=/bin/zsh` (the runner's own is bash), setup
adds the line to `~/.zshenv`, which the runner has none of; the container's
ordinary user starts with no startup file, so setup makes its `~/.profile`.
The Ubuntu runners have `~/.local/bin` on `PATH` already, through
`/etc/environment` and `~/.profile`, and show only that it is found.

Then `scripts/floti.dev/check-setup.sh` or `check-setup.ps1` checks what
setup left, each missing thing failing the job with one line naming it:
`floti version` answers from the installed path with the run's version, and

| Where | What setup must have left |
|---|---|
| `ubuntu-latest`, `ubuntu-24.04-arm` | `~/.config/systemd/user/floti.service` running that floti, enabled and active in the user's systemd |
| `macos-latest` | `~/Library/LaunchAgents/dev.floti.host.plist` running that floti, loaded and running in `gui/<uid>` |
| `windows-latest`, `windows-11-arm` | the logon task "Floti host" running that `floti.exe host` at sign-in, running, with a host running from its folder and the folder on the user's `Path` |
| `debian` as root | nothing: no unit and no start-on-connect marker, as a refused setup leaves |
| `debian` as an ordinary user, with `WSL_DISTRO_NAME` set | the start-on-connect marker in the data folder and no unit, as in a WSL distro where no systemd runs |

On both Windows runners the run's `Helm-<version>-setup.exe` then runs with
`/S`, per user, as `floti add desktop` runs it: in `install-dry-run` over the
install script's floti, and in `installer-dry-run` alone on a clean runner,
where nothing of Floti's may be there first
(`scripts/floti.dev/try-installer.ps1`). Each must leave, in
`%LOCALAPPDATA%\Programs\Floti\bin`, the release's
`floti-<version>-<cpu>.exe` byte for byte, the floti for that runner's CPU,
and no process running from the app's own folder, so the floti the app
carries is not the running host; and `check-setup.ps1` must pass as it does
after `install.ps1`. Those same checks after either way in are what "runs
the same setup" means: the same floti on the same `Path`, started by the
same logon task. `floti doctor` is not the check, because on a runner it
reports Claude Code missing, which says nothing about setup.

Last in each of those jobs, on every runner and in both container runs,
`scripts/floti.dev/try-uninstall.sh` or `try-uninstall.ps1` runs
[`floti uninstall`](#floti-uninstall) three times, each of which must exit 0:

1. `floti uninstall`, from the installed floti. floti and its `node`, the
   sign-in entry (the unit, the LaunchAgent and what launchd has loaded, the
   logon task, or the start-on-connect marker), the desktop app where an
   installer put it there (`Helm.exe` and the Start menu's `Helm.lnk`) and,
   on Windows, floti's folder on the user's `Path` must be gone, with the
   lines that say so; on macOS and Linux no startup file may hold floti's
   line, and where the install added one, uninstall must say it removed
   it. Where a host ran it must say it stopped it, and no
   host may run after it: no process from floti's folder on Windows, no
   `dev.floti.host` loaded in launchd, no active `floti.service`, and no
   `<floti> host` process where `pgrep` is there to ask. The data folder,
   where setup made one, must stay (`data: kept`). On Windows the
   `floti.exe` it ran from goes once it exits, and `Programs\Floti` after
   it, which is waited for up to a minute; a run that says the copy is left
   for a later sweep fails. Lingering and the desktop app's settings are
   left as uninstall leaves them, and not checked.
2. `floti uninstall --purge`, and the data folder must be gone.
3. `floti uninstall` again, which must change nothing: no line says
   anything was removed, stopped or closed, and the sign-in entry, the
   desktop app and the data each say `not found`.

floti removes itself in the first run, so the other two run a copy of it
made beforehand outside its install place, which removes what the install
put there and leaves itself, as a floti from elsewhere does: the third
run's last line is `floti: left as it is`.

`try-install.sh`, `try-install.ps1`, `try-installer.ps1`,
`try-uninstall.ps1`, `try-uninstall.sh` and `try-add-desktop.sh` change the
computer they run on, so each refuses in one line, before doing anything,
unless `GITHUB_ACTIONS` is `true`.

`floti add desktop` cannot run in a dry run: it fetches from the releases
address built into floti and checks the desktop files' list with the key
built into it, which refuses a list signed with the run's key. So
`add-desktop` runs only after `publish`, on a published release, preview or
stable, on `windows-latest`, `macos-latest` and `ubuntu-latest`
(`scripts/floti.dev/try-add-desktop.sh`): it downloads the release's floti
for the runner without a token, checks it against the run's SHA-256 list,
runs `floti add desktop` and checks that it exited 0 and the app is where
add puts it with its launcher entry (the Start menu's `Helm.lnk` and the
uninstaller; `~/Applications/Helm.app`; `~/Applications/Helm.AppImage`, its
`helm.desktop` and icon). On Windows and macOS the app's install ran
`floti install-self`, so `check-setup` runs there too. The Linux runner has
no display, and add asks only whether `DISPLAY` is set, so the job sets one
that is not there; the AppImage is never started. It publishes and pushes
nothing and reads no secret.

A dry run never reads the
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

## floti setup

`floti setup` (`crates/host/src/setup.rs`) is what the install scripts run,
and what `floti install-self` runs from the floti it puts in place. It checks
Claude Code and says what to run when it is missing or outside the tested
range, makes the data folder and the host key, registers the host to start
at sign-in with the OS's per-user mechanism (on a Linux server with
lingering on; in WSL without systemd it marks the host to start when a
client connects) and starts it, then says how that ended in one line and
exits with that outcome's status.

**floti's folder on PATH.** Before the host, when the floti setup runs from
is the one in the user's own folder (`~/.local/bin/floti`,
`%LOCALAPPDATA%\Programs\Floti\bin\floti.exe`) and that folder is not on
the user's `PATH`, setup puts it there and says so in one line, so every way
in, `curl | sh`, `irm | iex`, the desktop's installer and the desktop's
install over ssh, ends with `floti` found by the next terminal or login. A
floti anywhere else, such as a build run from its own folder or Homebrew's,
changes nothing, and neither does a setup refused as root or a Windows
service account. The folder is the one the user's home names
(`Standalone::binary`), even where the OS gives setup's own path with its
links resolved (a home under `/var/home` reached as `/home`), so the line
is the one `floti uninstall` looks for. On Windows the folder is appended to
the user's `Path` value. On macOS and Linux it is the line `floti doctor`
gives (`platform/user_path`), in the file doctor names:

| `$SHELL` | The file | The line |
|---|---|---|
| bash | the first of `~/.bash_profile`, `~/.bash_login` and `~/.profile` there, else `~/.profile` | `export PATH="$HOME/.local/bin:$PATH" # added by floti` |
| zsh | `~/.zshenv` | the same |
| fish | `~/.config/fish/config.fish` | `fish_add_path -g "$HOME/.local/bin" # added by floti` |
| sh, dash, ash, ksh, mksh | `~/.profile` | the same as bash |
| not set, or any other (tcsh, nushell) | none | none: setup says it knows no startup file of that shell, and to add the folder there |

"Not on `PATH`" is doctor's reading: the folder is in none of the folders a
login shell starts with and no `PATH` line in a file that shell reads names
it, so a `~/.profile` that adds `~/.local/bin` already, as Ubuntu's and
Debian's do, gets nothing. The line is appended, after a line break when
the file does not end in one, and nothing else in the file changes; a file
that is a link is appended to through it, and keeps its permissions; the
file is made only where it is not there (with `~/.config/fish` for fish);
and a file with floti's line already keeps it once. A read-only file, or a
folder the file cannot be made in, is refused and left as it was. It says
`Put <folder> on your PATH in <file>, for terminals and logins started from
now on.`, except where only a login reads that file (bash's and sh's) and a
new terminal is not a login, as on a Linux desktop, whose terminals keep
the `PATH` the session started with: there it says `for logins started from
now on, and for terminals once you sign in again.` A file that could not be
changed is one line with doctor's line to run instead. Neither changes
setup's exit status.

**The desktop app** comes last, once the host is set up, and not at all when
setting it up failed, so a desktop install that fails leaves a working host
and says so (`crates/host/src/setup/desktop.rs`). It installs only through
`floti add desktop` ([below](#floti-add-desktop)), and nothing it does
changes setup's exit status, which stays the host's.

| Run | What setup does about the desktop app |
|---|---|
| `--desktop` | Runs `floti add desktop` without asking, which refuses a session with no display, and says an installed app is there, as it always does. In a WSL distro with a display it installs, since it was asked for. |
| `--no-desktop` | Looks at nothing and says nothing. |
| Neither, the app installed | Says where it is, in `floti add desktop`'s line, and asks nothing. |
| Neither, the app not offered | Asks nothing and says nothing. The app is not offered with no display (a server; an ssh login to Windows, which is session 0; a Mac nobody is signed in to at its screen, where launchd has no GUI domain for the user), after setup itself found nobody signed in (the host starts at the next sign-in, on any OS), or in a WSL distro, whose desktop is Windows' own. |
| Neither, nowhere to ask | Asks nothing and installs nothing; one line names `floti add desktop`. Setup asks only when standard input and standard output are both terminals and it runs in the terminal's foreground, so `floti setup > log`, the desktop's own ssh and `curl ... \| sh &` get the line. |
| Neither, a terminal and a display | Asks once, `Install the Helm desktop app as well? [Y/n]`: Enter or `y` runs `floti add desktop`, `n` or the end of the input says one line naming it, anything else is asked again. |

`--desktop` and `--no-desktop` together are a usage error, and either goes
with `--linger` or `--no-linger`. A desktop install that fails says
`floti add desktop`'s own line on standard error, followed by "The Floti host
is set up all the same." `floti install-self` always passes `--no-desktop`:
the app that runs it is there already, whether the Windows installer, the
app at start, `floti update` after a desktop update or `floti add desktop`
on macOS, and the last two show what install-self printed, so it says
nothing about the app.

How each way in reaches the question: `install.sh` under `curl | sh` gives
setup the terminal (`/dev/tty`) as its input, so a person at a terminal is
asked and a CI runner or a script with no terminal gets the one line, as
does the script run as a background job, which is not in the terminal's
foreground and would be stopped reading it;
`install.ps1` under `irm | iex` runs setup in the user's own PowerShell
session, whose console is its input. The desktop's install on another
computer runs setup over `ssh -o BatchMode=yes` with no terminal, so it asks
nothing there and installs no desktop app.

## floti add desktop

`floti add desktop` is the one way the binary installs the desktop app
(`crates/host/src/add.rs`); `floti update` then keeps it up to date. It
installs the app of the release that is floti's own version, for this OS and
CPU, per user and with no admin rights, where the desktop's installer puts
it:

| | Where | Its launcher entry |
|---|---|---|
| Windows | `%LOCALAPPDATA%\Programs\Helm`, by running the release's `Helm-<version>-setup.exe` with `/S`, which runs `floti install-self` itself. | The Start menu entry the installer makes. |
| macOS | `~/Applications/Helm.app`, unpacked from `Helm-<version>-arm64-mac.zip` with `ditto -x -k --noqtn` beside it and renamed into place; then `floti install-self` runs from the floti inside it. | `~/Applications` is the Applications entry. |
| Linux | `~/Applications/Helm.AppImage`, from `Helm-<version>-linux-x64.AppImage`, written beside, made executable and renamed into place. | `applications/helm.desktop` and `icons/hicolor/256x256/apps/helm.png` under `$XDG_DATA_HOME` (`~/.local/share`); the icon is the AppImage's own, built into floti. |

**Which release.** The release that is floti's own version, preview or
stable, at `https://github.com/floti-dev/floti/releases/download/v<version>/`;
`update.json` plays no part. Its desktop files are listed with their
SHA-256s in `Helm-<version>-SHA256SUMS` ("Releasing"): floti's own list
cannot hold the app, because the app carries it. A version with no published
release (a development build), and a release made before the list existed,
each says so in one line and installs nothing. A release with no desktop file
for this computer (Windows on Arm, Linux on Arm) says so too.

**The checks.** The list's signature is fetched first and must verify with
the key built into floti, with the trusted comment `floti <version>
Helm-<version>-SHA256SUMS`; a signature by another key is refused before the
list is fetched. Then the desktop file downloads into `add` in Floti's data
folder and is checked against its own signature and its SHA-256 in the list
before anything runs, as `floti update` checks a file ("floti update", "The
checks"). A list or file that fails a check, or a list that does not name the
file, stops it: the line on standard error names the file and which check it
failed, the download is removed, and nothing is installed. The folder is
removed once the app is installed, or once the install failed.

**Nothing is quarantined.** floti's own process writes the download, which
neither Gatekeeper's quarantine nor Windows' mark of the web applies to, and
`--noqtn` keeps `ditto` from carrying a flag over from the archive, so the
ad-hoc signed Mac app opens without Open Anyway.

**One floti.** The app carries this release's floti, so `install-self` from
it leaves floti installed on its own as it is, and puts the Node beside it
in place. The app, which runs `install-self` at start only when the floti
there is missing or older ("floti in your own folder"), then uses that floti
and the host already running; nothing stops or restarts the host.

**What it refuses.** It holds `floti update`'s run file (`update-run.json`)
from start to end, so a second `floti add desktop`, or one while `floti
update` runs, refuses in one line and two never write the same download; it
writes no ended record, since a host follows updates by it. Where the app is
already installed it says where, with its version where that can be told,
and changes nothing but a menu entry floti makes that is missing (Linux).
Where something that is not an install is at the install's place (a
`Helm.app` with no `Info.plist`, a `Programs\Helm` with no uninstaller, a
folder named `Helm.AppImage`) it refuses in one line and leaves it as it
is. In a session with
no display it refuses in one sentence saying why: on Linux when neither
`DISPLAY` nor `WAYLAND_DISPLAY` is set, on Windows in session 0, where ssh
logins and services run, and on a Mac when nobody is signed in at its
screen (launchd has no GUI domain for the user). `floti add` with no
name, or a name it does not know, lists what it can add, which is `desktop`
alone.

Progress goes to standard output, one line a step; the status is 0 when the
app was installed or already there and 1 otherwise. Its tests use fake
release files served from a loopback port and fake folders
(`crates/host/tests/add.rs`, `src/add/tests.rs`); a test build alone reads
`FLOTI_DESKTOP_DIR` for the folder the app goes in, `FLOTI_TEST_DISPLAY`
for whether there is a display, and the releases' address and the key from
the marker in its data folder, as `floti update`'s tests do.

## floti update

`floti update` replaces the floti that runs it with its channel's newest
release, has this computer's host start it in its place, and updates the
desktop app where it is installed and older, once the app has quit
(`crates/host/src/update`). The host runs the same checks by itself for
floti alone (**The host's own update**, below), and runs `floti update
--from-desktop` when the desktop's Update asks it to (**Updates in Helm**).
`floti update --floti-only` replaces floti alone and never waits for the
app, which this computer's host then offers its own update; it exits 4
rather than 0 when floti is up to date. Another computer's desktop runs it
over the login ("Installing floti on a host's computer").

**The manifest.** It reads `https://floti.dev/update.json` and its signature
`update.json.minisig`, which each stable release writes and copies there
("floti.dev"):

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

**It asks nothing.** An update ends no session. Installing the app stops no
host on any OS: the host runs from floti installed on its own, outside the
app's install, and the Windows installer renames a running copy aside rather
than stopping it (`installer.nsh`). Replacing floti ends nothing either: the
host restarts on it through its session keeper (**The handover**, below).
The installer is started out of any job `floti update` is in, so it finishes
whatever ends the session that ran it.

**The order.** Nothing is replaced until everything has passed: the
manifest, this computer's floti (written beside the running one under a
name of its own) and the app's file (kept in `update` in the data folder). A
failed check stops the update with one line on standard error naming the
file and what failed and that nothing was updated, removes what was
downloaded, and leaves the old floti working. Then floti is replaced, and
the host on this computer, where one runs, is asked as the host itself to
start the new floti in its place (`host:stop` with reason `relaunch`); `floti
update` says it is, or why it could not ask. Then the app: while it runs,
`floti update` says it is waiting for it to quit and looks again each
second. Then, on macOS, it runs `floti install-self` from
the new app's floti (`Contents/Resources/floti`), so the Node beside floti
installed on its own is the new app's; the Windows installer runs it
itself, and the AppImage's floti is inside its image, so its app runs it
when it next starts. What an earlier update or install left beside floti
(copies put aside, `floti.old-<pid>-<n>.exe`, removed once nothing runs
them; downloads, `.floti-<version>.<pid>`) or beside the app (a new or old
copy, `.Helm.app.floti-update-<pid>` or `.Helm.app.floti-old-<pid>`) is
removed at the start of the next, the downloads and copies of the app once
the process that left them is known to have ended.

**Where things are replaced** (`crates/host/src/platform/installs`):

| | floti | the desktop app | its version |
|---|---|---|---|
| Windows | The running `floti.exe` is renamed aside (`floti.old-<pid>-<n>.exe`) and the new one moved into its name, as `install.ps1` and `floti install-self` do (`platform/standalone`); the old copy is removed at once when nothing runs it, else by the next update or install. | The per-user NSIS install, `%LOCALAPPDATA%\Programs\Helm` (`Helm.exe` beside `Uninstall Helm.exe`), updated by running the release's setup.exe with `/S`. | `DisplayVersion` of the per-user uninstall entry whose `UninstallString` is that install's uninstaller. |
| macOS | Written beside, made executable, stripped of any quarantine flag, renamed over. | `Helm.app` in `~/Applications`, else `/Applications`: the release's zip is unpacked beside it with `ditto -x -k --noqtn` and swapped in by rename. | `CFBundleShortVersionString` in its `Info.plist`. |
| Linux | Written beside, made executable, renamed over. | The AppImage at `~/Applications/Helm.AppImage` (AppImage's own tools look there, and it is outside the data folder `floti uninstall` keeps), where `floti add desktop` puts it: written beside, made executable, renamed over. | Unknown, so it is always updated. |

A release with no desktop file for this computer updates floti alone and
says so. No downloaded file carries a quarantine flag or a
download mark: floti's own process writes them, which neither Gatekeeper's
quarantine nor Windows' mark of the web applies to, and `--noqtn` keeps
`ditto` from carrying one over from the archive.

### The host's own update

A host keeps floti up to date by itself (`crates/host/src/update/background.rs`)
while its `updateCheck` setting is on: on by default, kept in `floti.db` and
shown in Settings → Updates as "Update floti by itself". Only a host running
floti installed on its own does it; one run from a development build, from
inside the desktop app, or with `FLOTI_CONFIG_READ_ONLY` set (every
development run and check) never looks.

It checks at most once a day: at its start when no check has reached the
manifest within the day, then a day after the last one that did, kept in
`update-checked.json` in the data folder so a restarted host does not look
again. A check that could not reach the release (no network, a download cut
off, a disk that refused the file) is tried again an hour later; one the
release itself failed (a signature, a checksum, a manifest without this
computer's build) waits the day. Turning the setting on checks at once when
the day has passed; turning it off stops the next check. Each check is `floti
update`'s for floti alone: the same manifest, the same checks, the same
replacement through `platform/standalone`. The desktop app is left to `floti
update`. Each line the check says goes to the host's log.

### The handover

Once a newer floti is in place, the host relaunches on it without waiting for
any session to end (`host:stop` with reason `relaunch`, which `floti update`
sends too). It lets go of its session keeper (`Keeper::let_go`), as a crash
would, so every session and plain terminal runs on there, releases its data
folder and socket, and then starts the floti at the path it was started from
in its place, with the arguments it was started with. The old host hands
over itself, in the user's own session; nothing else starts the new one, and
it is never started by ssh or as a service:

- On macOS and Linux the old host replaces its own process image (`exec`),
  so the new host keeps its process id: the LaunchAgent and the systemd user
  unit go on tracking it, and a desktop that started it goes on holding it.
- On Windows there is no `exec`. The old host starts the new floti alone, as
  it starts the session keeper (outside its job, with none of its handles),
  with `--after <pid>`, and exits; the new host waits until that process has
  ended before it opens anything.

The new host takes the keeper back when it speaks the keeper's link version,
and every session keeps its tab. One that does not ends the keeper with what
it holds, and those sessions come back through the restore offer in their
tabs, as after any keeper that was lost. A client sees the connection end
without a stop, keeps each tab's screen dimmed, and reconnects.

## floti uninstall

`floti uninstall` (`crates/host/src/uninstall.rs`) takes off the computer
what installing Floti put there, either way in: the install scripts, or the
desktop installer through `floti install-self`. It is the explicit act, so it
asks nothing. `floti uninstall --purge` removes the data too.

It goes in this order, so a run that stops part way leaves nothing
half-working, and the next run carries on where it stopped:

1. **The desktop app's processes, if they run, are ended one by one**, each
   alone and never as a tree. A desktop that started the host would start
   another as soon as the host stops, and the host it started is its child:
   ending the app's tree would kill that host, its keeper and every session
   uncounted. Left running, the host is what step 2 stops. A process of the
   app's this user cannot end (another user's, or one run as administrator)
   is left, and one line says how many.
2. **The host stops for good** (`host:stop` with reason `stop`, as the host
   itself, which `update-stop.rs` holds): every session and plain terminal
   ends, each session recorded as stopped so none is offered back, and the
   session keeper ends with it. One line says how many sessions it ended.
   With no host answering, a keeper a host left running is ended with what
   it holds. The sessions' history stays in the data folder.
3. **The start-at-sign-in entry** (`platform/startup/register.rs`,
   `unregister`): the logon task "Floti host" (`schtasks /Delete`, and its
   XML under the data folder), the LaunchAgent `dev.floti.host`
   (`launchctl bootout` when loaded, then its property list), or the user
   unit `floti.service` (`systemctl --user disable --now`, its file, then
   `daemon-reload`; with no user manager answering, the unit and its
   `default.target.wants` link go by hand). In a WSL distro without systemd,
   the record that has `floti connect` start the host goes instead.
   **Lingering is left on**: it is a per-user setting other services may rely
   on, so a Linux computer with it on gets one line saying so and that
   `loginctl disable-linger` turns it off.
4. **The desktop app** (`platform/installs`, `removal`): on Windows by its own
   NSIS uninstaller, `"Uninstall Helm.exe" /S _?=<install folder>`, run in
   place so the wait is for the uninstall itself (without `_?=` NSIS copies
   itself to `%TEMP%` and returns at once); it removes the app, its Start menu
   entry and its uninstall registry entry, and what it leaves of its folder
   goes after it. A file there that Windows or an antivirus still holds just
   after the uninstaller exits (refused with access denied or a sharing
   violation) is tried again for up to five seconds, with a pause that
   doubles, before the run fails on it (`platform/held_files`, which
   `floti install-self` and `floti update` rename through too). A run whose
   `Helm.exe` is still there afterwards fails.
   On macOS the `.app`; on Linux the AppImage, `applications/helm.desktop`
   and the icon. **The desktop app's own settings stay**: they are the app's,
   where the app keeps them, which the host never learns, and its uninstaller
   leaves them too. One line says so, with `--purge` as well.
5. **The PATH entry** (`platform/user_path`, `remove`): on Windows each entry
   of the user's `Path` that names floti's folder, as written or once its
   variables are expanded, goes, and the rest stay as written, still an
   expandable string. On macOS and Linux floti's line for its folder, the
   one setup adds, marker and all, goes from every startup file of a shell
   floti knows, whatever the user's shell is now; every other line stays as
   written, byte for byte and in its order, whatever its encoding, a line
   the user wrote naming the same folder included. The file is written over
   in place and cut to its new length, through a link where it is one, so
   it stays the same file, with its owner, permissions, hard links,
   extended attributes and ACLs, in a folder that need not be writable; a
   read-only file is refused and left as it was.
6. **The data, with `--purge`** (below). It comes before floti, so a purge
   that fails leaves the floti that runs it to try again with.
7. **floti and its Node** (`platform/standalone`): the `node` folder beside
   it, copies an install or update put aside or left part way, what an
   install script left (install.ps1's `floti.old.exe`, and a
   `.floti-<version>.<pid>` download, either script's or `floti update`'s,
   whose process has ended, by the rule `floti update` sweeps them with; one
   whose process still runs is left to it), and floti itself. A
   running `floti.exe` can be neither removed nor replaced on Windows, and
   uninstall runs from it, so it is renamed aside (`floti.old-<pid>-<n>.exe`)
   and a hidden Windows PowerShell 5.1, started apart from this process
   (with none of its handles, so a script reading uninstall's output is not
   held open, and outside its job, or inside it where the job does not let
   what it holds break away, which it still outlives), waits for it to exit
   and deletes the
   copy, retrying for up to five minutes while anything still runs it, then
   `Programs\Floti\bin` and `Programs\Floti` once they are empty and not
   junctions. That needs no admin rights (a delete at the next restart does),
   leaves no copy of floti in `%TEMP%` that would have the same problem, and
   waits on Windows' own refusal to delete a file a process runs, the test
   install-self and `floti update` already use; the sweep of copies put aside
   that both make is the fallback. Inside a job that ends every process in
   it when it closes, the line adds that the job may end that PowerShell
   first. Where that PowerShell cannot be started at all, one line says the
   copy is left for that sweep, and the run still exits 0. On macOS and Linux
   the file is unlinked, which the running process survives, and
   `~/.local/bin` stays: it is the user's.

**The data.** Without `--purge` the data folder, the logs and the config
folder stay, each named on one line. With `--purge` they go, and on Linux the
socket's folder under `$XDG_RUNTIME_DIR` too, each unlinking every link it
holds (a launch folder's junctions into repositories) and never walking into
one, then Floti's entry in the OS secret store: the Keychain item on macOS,
the Secret Service item on Linux, and on Windows none, since DPAPI stores
nothing. A folder is purged only when it is Floti's: the one floti uses with
no `FLOTI_*_DIR` override, or one an override names that holds what floti
writes there (the data folder its `host.key` or `floti.db`, the logs
`host.log`, the config folder the `agents` or `templates` folder floti
seeds). The socket's folder, which floti marks with nothing, is left with a
line when an override moved it. Before anything is changed a purge is
refused outright, with status 1, when the home folder is unknown, or when a
folder is an override's with no such mark, holds a `.git`, is the home folder
or holds it, or is, holds or is inside `~/.claude` or `~/.ssh`; nothing
touches an agent's config, `~/.ssh` or a repository.

A floti that is not the one in the place the install scripts use, such as one
Homebrew or winget installed, removes only what it installed: steps 1 to 4
and the data, then one line, `floti: left as it is (installed by <where it
runs from>)`, leaving itself and the PATH to whatever installed it. A `node`
folder in that place is left too: only beside a floti installed on its own
is it Floti's.

| Line on standard output | Means |
|---|---|
| `desktop app: closed, so it starts no host` | It was running and was closed. |
| `desktop app: left 1 process running that this user cannot end, ...` | Another user's, or one run as administrator. |
| `host: stopped, ending 2 sessions` / `host: not running` | The host, or a keeper a host left, was stopped; or nothing answered. |
| `sign-in entry: removed (logon task "Floti host")` / `not found` | The entry, named as the OS calls it. |
| `lingering: left on, ...` | Linux, where it is on. |
| `desktop app: removed (<path>)` / `not found` | |
| `desktop app settings: kept; ...` | After the app is removed, and with `--purge`. |
| `PATH entry: removed (<folder>)` / `not found` | On macOS and Linux, whether a startup file had floti's line. |
| `data: kept (<folder>); floti uninstall --purge removes it` / `removed (<folder>)` / `not found` | And `logs:`, `config:`, with `--purge` `runtime:` where it is apart (or `left as it is`). |
| `secret store: removed Floti's key` / `not found` | `--purge` only. |
| `floti: removed (<path>)` / `not found` | Last. On Windows followed by `; the copy this process runs from goes once it exits`, or by what is left for the next sweep. |
| `floti: left as it is (installed by <path>); ...` | A floti from elsewhere. |

So a second run says `not found` for each and exits 0. Each line starts with
what it is about and a colon, for a script to match. A step that fails stops
the run with one line on standard error, `floti uninstall: <what>: <why>`,
and status 1; a host that cannot be stopped says nothing was removed. Run
from inside one of Floti's own sessions (a `floti` among its parent
processes, the keeper's or the host's), it refuses before changing anything,
since stopping the host would end it. Any other argument is a usage error,
status 2.

Its tests use fake folders and a fake system, never the computer's own
(`crates/host/tests/uninstall.rs`, `src/uninstall/tests.rs`, and the
`unregister`, `removal`, `remove` and `delete_after_exit` tests in each
concern). The install and uninstall run for real only on CI runners, in the
release's dry run ([A dry run](#a-dry-run)).

## Updates in Helm

Helm's own process contacts nothing about updates. It learns of a newer
release from this PC's host, over the window's own connection
(`renderer/src/app/host/updates.ts`):

- **`update:state`** is what the host knows, whole on every change: the
  floti it runs and the channel it follows, the channel's newest version and
  when the last check that read the manifest found it (kept in
  `update-checked.json` beside the time, so a restarted host still knows),
  why the last check failed, whether one runs, and where an update it runs
  has got to. The window calls a release newer when the channel's newest is
  strictly ahead of its own version; the status bar offers it beside the
  version and opens Settings → Updates.
- **Check now** is `update:check`: the host checks at once, whatever
  `updateCheck` says and however recently it last did, and answers once the
  check has ended. It keeps no throttle. A check that finds a newer floti
  puts it in place and relaunches the host, as the background check does.
  Offline is an expected answer, said with its reason, never as "up to date".
- **Update** is `update:install`: the host starts `floti update
  --from-desktop` apart from itself, so it outlives the host and the app and
  holds nothing of the host's open. Every update keeps the run file
  `update-run.json` in the data folder while it runs: made with
  `create_new`, it is the lock that makes a second update (a terminal's, the
  background check, another Update) refuse, and it holds the update's
  process and its last line, marked while it waits for the app. When the
  update ends it writes how in `update-ended.json` and removes the run file.
  The host follows the two every second and tells each step on
  `update:state`, and a host that starts while an update runs (relaunched,
  or after a crash) follows it too, so an Update pressed meanwhile starts
  nothing and Check now rests. When the run file says the update waits for
  the app, Helm's main process quits, asking nothing and keeping every
  session on every host running (`followHostUpdate`, `quitKeeping`). The
  update asks the host to relaunch only once the app is replaced, then opens
  the app again (replaced, or as it was when replacing it failed); on Linux
  with the display and session-bus variables the app ran with, read from its
  own process, since a host started by the systemd user unit has none. An
  update that fails says its last line in Settings → Updates until the next
  one starts.

The setting that turns the automatic check off is the host's
(`HostSettings.updateCheck`): the host is the one that asks. A host that does
not update says why on `update:state` and checks nothing: one whose config
folder is another host's (`FLOTI_CONFIG_READ_ONLY`: `pnpm dev` and every
check), and one that runs any floti but the one installed on its own. The
pane says so rather than showing an error, and Check now and Update rest.

There is deliberately no electron-updater: the build is unsigned, so its NSIS
path would put a SmartScreen prompt in front of every update, and the portable
exe has no install location for it to replace. `floti update` is the one
updater, and its signature check is the one to get right.

## Network posture

**The host's update check, at most once a day to floti.dev and to the
GitHub release it names, is the only request Helm makes on its own
initiative; the app itself contacts nothing.
Everything else on the network happens because you asked for it: Check
now and Update ask the host to check or to update, the browser pane
fetches the page you navigate to, a plugin you added reaches the hosts
its manifest names, and a host you added on another computer is reached
through your own ssh, or the command you gave.**

There is no telemetry, no crash reporting, no fonts and no CDN. With
`updateCheck` off, nothing asks anything by itself; the one remaining route is
a person pressing Check now or Update, or running `floti update` or
`floti add desktop`, so nothing leaves the machine unasked for.

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

## Updates need the repository to be public

floti fetches its release files from the GitHub release folder `update.json`
names with no token, because Floti handles no GitHub credential. GitHub
answers an unauthenticated request for a **private** repository's release
files with `404`, so while the repository is private every update stops on
its first download, however current the release actually is.

This is a coupling that breaks silently: **making the repository private
turns updates off**, the host's own and the one Update starts, and nothing in
the app says so beyond the failure line.
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
