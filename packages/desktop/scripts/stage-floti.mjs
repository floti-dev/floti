// Builds the `floti` host in release and copies it to `dist-host/`, where
// electron-builder.yml's `extraResources` picks it up. A packaged Helm runs it
// from `process.resourcesPath` (`flotiBinary` in src/main/host/launcher.ts).
// Beside it goes the pinned Node
// runtime the host runs plugin services on, fetched here, at build time only,
// and checked against its SHA-256 before anything is extracted.
//
// The release workflow builds floti once for each OS and CPU
// (.github/workflows/floti.yml) and hands the desktop's jobs those files:
// FLOTI_PREBUILT names the one for this platform, which is staged in place of
// a local build, and FLOTI_SHA256SUMS the release's SHA-256 list, which must
// list that file with its SHA-256 and is staged beside it as
// `floti-SHA256SUMS` (floti-release.mjs). On Windows FLOTI_PREBUILT_ARM64
// names the release's floti for Windows on arm64, staged beside it as
// `floti-arm64.exe`, which `floti install-self` installs on an arm64
// computer; a release's Windows build, given FLOTI_SHA256SUMS, must have it.
// Without FLOTI_PREBUILT floti is built here, and no list or arm64 copy is
// carried.
//
// Copied rather than referenced in place because Cargo's output folder moves
// with CARGO_TARGET_DIR, and electron-builder's file sets are relative to this
// package.

import { spawnSync } from 'node:child_process'
import { copyFileSync, existsSync, mkdirSync, rmSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { STAGED_ARM64_NAME, STAGED_SUMS_NAME, desktopVersion, stagePrebuilt, stageWindowsArm64 } from './floti-release.mjs'
import { stageNode } from './node-runtime.mjs'

const desktopDir = resolve(import.meta.dirname, '..')
const repoRoot = resolve(desktopDir, '..', '..')
const name = process.platform === 'win32' ? 'floti.exe' : 'floti'
const staged = join(desktopDir, 'dist-host')
const target = process.env['CARGO_TARGET_DIR'] || join(repoRoot, 'target')
const prebuilt = process.env['FLOTI_PREBUILT'] || undefined
const sums = process.env['FLOTI_SHA256SUMS'] || undefined
const arm64 = process.env['FLOTI_PREBUILT_ARM64'] || undefined

if (arm64 !== undefined && (process.platform !== 'win32' || prebuilt === undefined)) {
  console.error("FLOTI_PREBUILT_ARM64 is the Windows desktop's second floti, so it needs Windows and FLOTI_PREBUILT")
  process.exit(1)
}

if (prebuilt !== undefined) {
  try {
    if (process.platform === 'win32' && sums !== undefined && arm64 === undefined) {
      throw new Error("a release's Windows build carries the arm64 floti too, so FLOTI_SHA256SUMS needs FLOTI_PREBUILT_ARM64")
    }
    const version = desktopVersion()
    const result = stagePrebuilt({ prebuilt, sums, stagedDir: staged, version })
    console.log(`Staged the release's ${prebuilt} for packaging as ${join('dist-host', name)}`)
    if (result.sums !== null) console.log(`Staged ${sums} as ${join('dist-host', STAGED_SUMS_NAME)}`)
    if (stageWindowsArm64({ prebuilt: arm64, sums, stagedDir: staged, version }) !== null) {
      console.log(`Staged the release's ${arm64} for packaging as ${join('dist-host', STAGED_ARM64_NAME)}`)
    }
  } catch (error) {
    console.error(`Could not stage the release's floti: ${error instanceof Error ? error.message : String(error)}`)
    process.exit(1)
  }
} else {
  if (sums !== undefined) {
    console.error("FLOTI_SHA256SUMS lists the release's files, so it needs FLOTI_PREBUILT naming the one to carry")
    process.exit(1)
  }
  const build = spawnSync('cargo', ['build', '--release', '--locked', '-p', 'floti-host', '--bin', 'floti'], {
    cwd: repoRoot,
    stdio: 'inherit'
  })
  if (build.error !== undefined) {
    console.error(`Could not run cargo to build the local host: ${build.error.message}`)
    process.exit(1)
  }
  if (build.status !== 0) process.exit(build.status ?? 1)

  const built = join(target, 'release', name)
  if (!existsSync(built)) {
    console.error(`cargo finished but ${built} is not there`)
    process.exit(1)
  }

  mkdirSync(staged, { recursive: true })
  // copyFileSync keeps the source's mode, so the executable bit survives on macOS and Linux.
  copyFileSync(built, join(staged, name))
  // A local build has no release sums or arm64 copy, and what an earlier run staged does not describe it.
  rmSync(join(staged, STAGED_SUMS_NAME), { force: true })
  rmSync(join(staged, STAGED_ARM64_NAME), { force: true })
  console.log(`Staged ${built} for packaging as ${join('dist-host', name)}`)
}

// The Node runtime the host runs plugin services on, staged as `dist-host/node/`
// (node-runtime.mjs). The archive is kept in Cargo's output folder, so a
// second build reuses it once it still matches its pin.
try {
  const node = await stageNode({ cacheDir: join(target, 'node-runtime'), stagedDir: join(staged, 'node') })
  const how = node.reused ? 'reused' : 'downloaded'
  console.log(`Staged Node ${node.pin.version} (${how} ${node.pin.archive}, SHA-256 verified) as ${join('dist-host', 'node')}`)
} catch (error) {
  console.error(`Could not stage the Node runtime: ${error instanceof Error ? error.message : String(error)}`)
  process.exit(1)
}
