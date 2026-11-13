---
name: procedures
description: Fixed procedures in Helm - adding an app setting, changing the database schema, cutting a release, publishing the plugin SDK, and building the Windows installer. Use before doing any of those.
---

## Adding an app setting

`app_settings` is JSON per key, so there is no migration.

1. The key and its default in `AppSettings` / `DEFAULT_SETTINGS`
   (`core/src/types.ts`).
2. A validator in `SETTING_VALIDATORS` (`core/src/store/settings.ts`). The map
   is keyed by `keyof AppSettings`, so a key without one does not compile. Add
   its valid and invalid cases to the table in `store.test.ts`.
3. A row in the matching group of `ui/src/components/SettingsPane.tsx`, with
   its test.
4. Only if the value drives something outside the database: a branch in the
   `settings:write` handler in `main/ipc.ts`.

Reads are tolerant (unknown keys ignored, bad JSON falls back per key); writes
are strict (a malformed write throws and writes nothing). Internal state such
as `windowBounds` lives in the same table but is not a setting and has no row in
the pane.

## Changing the schema

Edit `core/src/store/schema.ts`, then run `pnpm db:generate`. The generated SQL
is embedded in the bundle, so skipping that step ships code that expects a
column its migrations never create.

## Cutting a release

1. Bump the version in `packages/desktop/package.json`. Merging that to `main`
   tags, builds and publishes. It must be greater, by semver, than every `v*`
   tag, or the release fails. A version with a pre-release part
   (`2.4.1-preview.12`) publishes as a public pre-release, never `latest`.
2. Add a `## <version>` section to `CHANGELOG.md`. The release fails without
   one. Write it for someone deciding whether to download the exe: what changed
   for them, grouped by surface. No commit log, no test or tool names.

Run `pnpm check` and `pnpm test:e2e` locally first; CI does not run the
end-to-end tests. To see the whole release run first, push the branch to
`release-dry-run/<name>`: it builds, signs with a key made for the run and
checks everything, publishes nothing, and leaves the files and their
signatures as the run's `release-files` artifact (docs/PACKAGING.md, "A dry
run"). A real release is signed by the `sign` job with `MINISIGN_SECRET_KEY`
from the `release` environment, and fails there unless every signature
verifies against `crates/host/src/update/minisign.pub`. That environment is a
one-time prerequisite: the owner creates it with a deployment branch policy
admitting `main` alone, then sets the key as its secret, never as a
repository secret (docs/PACKAGING.md, "The release environment"). A stable
release then pushes the Homebrew formula to floti-dev/homebrew-tap, opens
the winget pull request and commits the install scripts and `update.json` to
floti-dev/floti-dev.github.io (floti.dev), after the release is published; a
preview does none of these. Each needs a prerequisite the owner creates once
- the tap and `HOMEBREW_TAP_DEPLOY_KEY`, a winget-pkgs fork and
`WINGET_TOKEN` on a GitHub account kept for this alone, with no access to
floti-dev, and `PAGES_DEPLOY_KEY` for the site - and its job fails naming the
missing one, to be re-run once it exists (docs/PACKAGING.md, "Homebrew and
winget" and "floti.dev"). A dry run builds the formula, the manifests and
floti.dev's files, checks them with Homebrew and winget, runs the install
scripts for real in CI runners, and pushes nothing. Then merge and push. CI runs `pnpm check`
and the build on Windows, macOS and Linux - a failure on any of them blocks
the release - and verifies the exes, the dmg and zip, and the AppImage from
the inside before publishing. `packaging-check` and `verify:installer` are not part of a
release and run only when the owner asks. `verify:installer --yes` installs over
and then uninstalls the Helm on this machine, leaving none behind.

## Publishing the plugin SDK

`@floti-dev/plugin-sdk` goes to npm on its own version, separate from
the app's.

1. Bump the version in `packages/plugin-sdk/package.json`: patch for a fix,
   minor for anything new (a manifest field, a bridge call, a hook), major
   only for a change that breaks a plugin that worked.
2. Run `pnpm check`. `test/package.test.ts` packs the SDK and uses it the way
   an author does, through `npm exec`, so a file missing from `files` fails
   there.
3. Merge to `main`. The `sdk` job in `release.yml` publishes when that version
   is not on npm yet, by trusted publishing, so no npm token exists anywhere.

The app and the SDK are independent: bumping one never publishes the other.
Helm bundles the workspace SDK, so a validator change ships in the next app
release whether or not the SDK was bumped; bump it as well, or authors check
against an older validator than the Helm they run.

## Building the Windows installer

`pnpm dist:win` goes through `scripts/dist-win.mjs`, never straight to
electron-builder: a stale standalone pnpm on PATH makes electron-builder fall
back to the npm collector and ship an exe with no `app.asar.unpacked`, which
dies on its first native module load. The wrapper checks the resolved pnpm.
`pnpm verify:artifact` checks a finished exe from the outside. Full process:
[docs/PACKAGING.md](../../../docs/PACKAGING.md).
