// The host `pnpm dev` connects to, and whether it has to be built first.
//
// An unpackaged app finds its floti where `carriedFloti` (`main/host/launcher.ts`)
// says: `<CARGO_TARGET_DIR or <repo>/target>/debug/floti`. Nothing built it, so a
// checkout whose host was last built before the client moved on (a newer protocol,
// a version bump) opened a window that could not connect. This decides, before the
// window opens, whether that floti is good enough, and builds it when it is not.
//
// The decision is `decideBuild` and has no side effects; `ensureDevHost` carries
// it out through the functions it is handed, so a test drives it with fakes.

import { spawnSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'

/** The most jobs a build here may use (`CLAUDE.local.md`: builds are kept small on this machine). */
export const BUILD_JOBS = 4

/**
 * Where the dev host is, the way `carriedFloti` resolves it for an unpackaged
 * app. `platform` is `process.platform`.
 */
export function devHostPath({ repoRoot, env, platform }) {
  const target = env.CARGO_TARGET_DIR || join(repoRoot, 'target')
  return join(target, 'debug', platform === 'win32' ? 'floti.exe' : 'floti')
}

/**
 * What `floti version` printed: `floti <ver> protocol <M.m> <os> <cpu>`
 * (`version_line` in the host). `null` when it is anything else.
 */
export function parseVersionLine(text) {
  const match = /^floti (\S+) protocol (\d+)\.(\d+)(?: |$)/m.exec(text)
  if (match === null) return null
  return { version: match[1], protocol: { major: Number(match[2]), minor: Number(match[3]) } }
}

/** The protocol the client speaks, read from the generated declarations. `null` when they do not say. */
export function clientProtocol(generatedText) {
  const match = /PROTOCOL_VERSION\s*=\s*\{\s*major:\s*(\d+),\s*minor:\s*(\d+)\s*\}/.exec(generatedText)
  if (match === null) return null
  return { major: Number(match[1]), minor: Number(match[2]) }
}

/** Semver order of two versions, prereleases included: negative when `a` is older. `null` for one that is not semver. */
export function compareVersions(a, b) {
  const parse = (text) => {
    const match = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+.*)?$/.exec(text)
    if (match === null) return null
    return { core: [Number(match[1]), Number(match[2]), Number(match[3])], pre: match[4]?.split('.') ?? [] }
  }
  const left = parse(a)
  const right = parse(b)
  if (left === null || right === null) return null
  for (let i = 0; i < 3; i++) {
    if (left.core[i] !== right.core[i]) return left.core[i] < right.core[i] ? -1 : 1
  }
  // A release is newer than any prerelease of it.
  if (left.pre.length === 0 || right.pre.length === 0) return Math.sign(right.pre.length - left.pre.length)
  for (let i = 0; i < Math.max(left.pre.length, right.pre.length); i++) {
    const x = left.pre[i]
    const y = right.pre[i]
    if (x === undefined) return -1
    if (y === undefined) return 1
    if (x === y) continue
    const xNumber = /^\d+$/.test(x)
    const yNumber = /^\d+$/.test(y)
    if (xNumber && yNumber) return Number(x) < Number(y) ? -1 : 1
    if (xNumber !== yNumber) return xNumber ? -1 : 1
    return x < y ? -1 : 1
  }
  return 0
}

/**
 * Whether the dev host has to be built, and why. `found` is what the floti at
 * the path is there, `output` what `floti version` printed (`null` when it did
 * not run); `client` is `{ protocol, version }`.
 */
export function decideBuild({ exists, output, client }) {
  if (!exists) return { build: true, reason: 'there is no host build yet' }
  const found = output === null ? null : parseVersionLine(output)
  if (found === null) return { build: true, reason: 'its version could not be read' }
  const older = (a, b) => a.major < b.major || (a.major === b.major && a.minor < b.minor)
  if (older(found.protocol, client.protocol)) {
    const text = (p) => `${String(p.major)}.${String(p.minor)}`
    return {
      build: true,
      reason: `it speaks protocol ${text(found.protocol)}, older than the client's ${text(client.protocol)}`
    }
  }
  const order = compareVersions(found.version, client.version)
  if (order !== null && order < 0) {
    return { build: true, reason: `it is floti ${found.version}, older than ${client.version}` }
  }
  return { build: false, reason: `floti ${found.version} protocol ${String(found.protocol.major)}.${String(found.protocol.minor)} is current` }
}

/** The environment a host build runs with: at most `BUILD_JOBS` jobs, fewer when the caller asked for fewer. */
export function buildEnv(env) {
  const asked = Number(env.CARGO_BUILD_JOBS)
  const jobs = Number.isInteger(asked) && asked > 0 ? Math.min(asked, BUILD_JOBS) : BUILD_JOBS
  return { ...env, CARGO_BUILD_JOBS: String(jobs) }
}

/**
 * Builds the host when `decideBuild` says so. Answers `{ built, reason }`;
 * throws with the build's own failure when it fails, so nothing opens a window
 * that cannot connect.
 *
 * `read(path)` answers a file's text, `exists(path)` whether it is there,
 * `version(path)` what `floti version` printed (or `null`), `build(env)` runs the
 * build and answers its exit status, `say(line)` shows a line.
 */
export function ensureDevHost({ repoRoot, env, platform, read, exists, version, build, say }) {
  const protocol = clientProtocol(read(join(repoRoot, 'crates', 'floti-protocol', 'generated', 'floti-protocol.generated.ts')))
  const app = JSON.parse(read(join(repoRoot, 'packages', 'desktop', 'package.json')))
  if (protocol === null) throw new Error("the client's protocol version could not be read from the generated declarations")
  const path = devHostPath({ repoRoot, env, platform })
  const present = exists(path)
  const decision = decideBuild({
    exists: present,
    output: present ? version(path) : null,
    client: { protocol, version: app.version }
  })
  if (!decision.build) {
    say(`host ${path}: ${decision.reason}`)
    return { built: false, reason: decision.reason }
  }
  say(`host ${path}: ${decision.reason}; building it (cargo build -p floti-host --bin floti, ${buildEnv(env).CARGO_BUILD_JOBS} jobs)`)
  const status = build(buildEnv(env))
  if (status !== 0) {
    throw new Error(`the host build failed (cargo exited ${String(status)}), so there is no host for the window to connect to`)
  }
  return { built: true, reason: decision.reason }
}

/** `ensureDevHost` against the real disk, `floti version` and `cargo`. */
export function ensureDevHostForReal({ env = process.env, platform = process.platform } = {}) {
  const repoRoot = resolve(import.meta.dirname, '..', '..', '..')
  return ensureDevHost({
    repoRoot,
    env,
    platform,
    read: (path) => readFileSync(path, 'utf8'),
    exists: existsSync,
    version: (path) => {
      const ran = spawnSync(path, ['version'], { encoding: 'utf8', windowsHide: true, timeout: 20_000 })
      return ran.status === 0 && typeof ran.stdout === 'string' ? ran.stdout : null
    },
    build: (buildEnvironment) =>
      spawnSync('cargo', ['build', '-p', 'floti-host', '--bin', 'floti'], {
        cwd: repoRoot,
        stdio: 'inherit',
        env: buildEnvironment
      }).status ?? 1,
    say: (line) => console.log(line)
  })
}
