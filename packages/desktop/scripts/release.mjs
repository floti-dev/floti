// What a release is, for .github/workflows/release.yml: which version it may
// be, and exactly which files it carries.
//
//   node release.mjs gate <version> [--dry-run] < tags
//     `tags` is `git ls-remote --tags origin 'refs/tags/v*'`. Prints the
//     decision as JSON: the version, its tag, whether it is a preview, whether
//     the build runs, and why. Refuses (exit 1) a version that is not
//     <major>.<minor>.<patch> with an optional pre-release part, and one that
//     is not tagged yet but is not greater, by semver, than the highest version
//     already tagged. A version already tagged is not built, except in a dry
//     run, which builds everything and publishes nothing.
//
//   node release.mjs names <version> [--unsigned]
//     prints every file a release of <version> carries, one per line, sorted:
//     each signed file and its `.minisig` signature beside it. With
//     --unsigned, only the signed files.
//
//   node release.mjs check <folder> <version> [--unsigned]
//     refuses a folder that does not hold exactly those files, and prints a
//     Markdown table of each with its size and SHA-256 for the step summary.
//
//   node release.mjs desktop-sums <folder> <version>
//     writes `Floti-<version>-SHA256SUMS` into <folder>: the SHA-256 of each of
//     the five desktop files there, in sha256sum's format, which `floti add
//     desktop` checks a download against. The app carries floti's own list,
//     so that list cannot hold the app; this one is the desktop files'.

import { createHash } from 'node:crypto'
import { createReadStream, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { pipeline } from 'node:stream/promises'
import { fileURLToPath } from 'node:url'
import { formatSums, releaseFileNames } from './floti-release.mjs'

// Semver 2.0.0 without build metadata: a `+` part is ignored in precedence, so
// two versions differing only there would be two tags no comparison can order.
const NUMBER = '(0|[1-9]\\d*)'
const IDENTIFIER = '(?:0|[1-9]\\d*|\\d*[A-Za-z-][0-9A-Za-z-]*)'
const VERSION = new RegExp(`^${NUMBER}\\.${NUMBER}\\.${NUMBER}(?:-(${IDENTIFIER}(?:\\.${IDENTIFIER})*))?$`)

/**
 * `{ major, minor, patch, pre }` for a version, with `pre` its pre-release
 * identifiers (empty for a release), or null when it is not one.
 */
export function parseVersion(text) {
  const match = VERSION.exec(text)
  if (match === null) return null
  return {
    major: Number(match[1]),
    minor: Number(match[2]),
    patch: Number(match[3]),
    pre: match[4] === undefined ? [] : match[4].split('.')
  }
}

const isNumeric = (identifier) => /^\d+$/.test(identifier)

/** Semver precedence: negative when `a` is lower, positive when higher, 0 when equal. Both must parse. */
export function compareVersions(a, b) {
  const left = parseVersion(a)
  const right = parseVersion(b)
  if (left === null || right === null) throw new Error(`cannot compare ${a} with ${b}: both must be versions`)
  for (const part of ['major', 'minor', 'patch']) {
    if (left[part] !== right[part]) return left[part] - right[part]
  }
  // A version with a pre-release part is lower than the same version without one.
  if (left.pre.length === 0 || right.pre.length === 0) return right.pre.length - left.pre.length
  for (let i = 0; i < Math.min(left.pre.length, right.pre.length); i++) {
    const x = left.pre[i]
    const y = right.pre[i]
    if (x === y) continue
    if (isNumeric(x) && isNumeric(y)) return Number(x) - Number(y)
    // Numeric identifiers are lower than alphanumeric ones.
    if (isNumeric(x)) return -1
    if (isNumeric(y)) return 1
    return x < y ? -1 : 1
  }
  return left.pre.length - right.pre.length
}

/**
 * The versions `git ls-remote --tags` lists as `v<version>` tags, and the tags
 * it lists that are not versions. Peeled lines (`^{}`) are the same tags again.
 */
export function taggedVersions(lsRemote) {
  const versions = []
  const others = []
  for (const line of lsRemote.split('\n')) {
    const ref = line.trim().split(/\s+/)[1]
    if (ref === undefined || !ref.startsWith('refs/tags/') || ref.endsWith('^{}')) continue
    const tag = ref.slice('refs/tags/'.length)
    if (tag.startsWith('v') && parseVersion(tag.slice(1)) !== null) versions.push(tag.slice(1))
    else others.push(tag)
  }
  return { versions, others }
}

/** The highest of `versions` by semver, or null when there are none. */
export function highestVersion(versions) {
  return versions.reduce((best, each) => (best === null || compareVersions(each, best) > 0 ? each : best), null)
}

/**
 * Whether a push builds `version` and what it would publish. `release` is
 * whether the build runs: false only when the version is already tagged and
 * this is not a dry run. Throws, saying why, for a version the release must
 * refuse.
 */
export function gate({ version, lsRemote, dryRun }) {
  if (parseVersion(version) === null) {
    throw new Error(`version '${version}' is not <major>.<minor>.<patch> with an optional -<pre-release> part`)
  }
  const tag = `v${version}`
  const preview = parseVersion(version).pre.length > 0
  const { versions, others } = taggedVersions(lsRemote)
  const highest = highestVersion(versions)
  const decision = { version, tag, preview, dryRun, highest, ignored: others }

  if (versions.includes(version)) {
    if (!dryRun) return { ...decision, release: false, reason: `${tag} is already tagged; bump the version to cut a new one.` }
    return { ...decision, release: true, reason: `${tag} is already tagged, so a release would not be cut; a dry run builds it anyway.` }
  }
  if (highest !== null && compareVersions(version, highest) <= 0) {
    throw new Error(`version ${version} is not greater than v${highest}, the highest version already tagged; releases only move forward`)
  }
  const after = highest === null ? 'the first tagged version' : `after v${highest}`
  return { ...decision, release: true, reason: `${tag} is not tagged yet, ${after}.` }
}

/** The desktop files a release of `version` carries. */
export function desktopFileNames(version) {
  return [
    `Floti-${version}-portable.exe`,
    `Floti-${version}-setup.exe`,
    `Floti-${version}-arm64.dmg`,
    `Floti-${version}-arm64-mac.zip`,
    `Floti-${version}-linux-x64.AppImage`
  ]
}

/** `Floti-<version>-SHA256SUMS`: the SHA-256 of each desktop file, signed like every file. */
export function desktopSumsName(version) {
  return `Floti-${version}-SHA256SUMS`
}

/**
 * Writes `Floti-<version>-SHA256SUMS` into `folder`, listing each of the five
 * desktop files there, sorted, and returns its path. A desktop file missing
 * from `folder` is an error.
 */
export async function writeDesktopSums(folder, version) {
  const entries = []
  for (const name of [...desktopFileNames(version)].sort()) {
    entries.push({ name, sha256: await sha256Of(join(folder, name)) })
  }
  const file = join(folder, desktopSumsName(version))
  writeFileSync(file, formatSums(entries))
  return file
}

/** The generated protocol types, as one archive. */
export function protocolFileName(version) {
  return `floti-protocol-${version}.tar.gz`
}

/** A file's minisign signature, beside it. */
export function signatureName(name) {
  return `${name}.minisig`
}

/**
 * The files of a release of `version` that are signed, sorted as jq and
 * `LC_ALL=C sort` sort them: the desktop files and their SHA-256 list, the
 * protocol types, the floti files and their SHA-256 list.
 */
export function signedFileNames(version) {
  return [
    ...desktopFileNames(version),
    desktopSumsName(version),
    protocolFileName(version),
    ...releaseFileNames(version)
  ].sort()
}

/**
 * Every file a release of `version` carries, sorted as jq and `LC_ALL=C sort`
 * sort them: each signed file and its signature. With `signed: false`, the
 * signed files alone, as they are before signing.
 */
export function releaseAssetNames(version, { signed = true } = {}) {
  const names = signedFileNames(version)
  return signed ? names.flatMap((name) => [name, signatureName(name)]).sort() : names
}

/** Why a folder's file names are not exactly a release's files, or null when they are. */
export function assetsProblem(names, version, { signed = true } = {}) {
  const expected = releaseAssetNames(version, { signed })
  const have = new Set(names)
  const missing = expected.filter((name) => !have.has(name))
  const extra = [...have].filter((name) => !expected.includes(name)).sort()
  const problems = []
  if (missing.length > 0) problems.push(`missing ${missing.join(', ')}`)
  if (extra.length > 0) problems.push(`not a release file: ${extra.join(', ')}`)
  return problems.length > 0 ? problems.join('; ') : null
}

/** A Markdown table of each file with its size and SHA-256. */
export function summaryTable(rows) {
  const lines = ['| File | Size | SHA-256 |', '|---|---:|---|']
  for (const { name, size, sha256 } of rows) lines.push(`| \`${name}\` | ${String(size)} | \`${sha256}\` |`)
  return `${lines.join('\n')}\n`
}

async function sha256Of(file) {
  const hash = createHash('sha256')
  await pipeline(createReadStream(file), hash)
  return hash.digest('hex')
}

async function check(folder, version, { signed }) {
  const names = readdirSync(folder)
  const problem = assetsProblem(names, version, { signed })
  if (problem !== null) {
    throw new Error(`${folder} is not the files a release of ${version} carries${signed ? '' : ' before signing'}: ${problem}`)
  }
  const rows = []
  for (const name of releaseAssetNames(version, { signed })) {
    const file = join(folder, name)
    rows.push({ name, size: statSync(file).size, sha256: await sha256Of(file) })
  }
  return summaryTable(rows)
}

async function main(args) {
  const [command, ...given] = args
  // `--unsigned` is a flag of `names` and `check` only; `gate` sees its arguments as given.
  const unsigned = (command === 'names' || command === 'check') && given.at(-1) === '--unsigned'
  const rest = unsigned ? given.slice(0, -1) : given
  const signed = !unsigned
  if (command === 'gate' && (rest.length === 1 || (rest.length === 2 && rest[1] === '--dry-run'))) {
    const decision = gate({ version: rest[0], lsRemote: readFileSync(0, 'utf8'), dryRun: rest[1] === '--dry-run' })
    console.log(JSON.stringify(decision))
  } else if (command === 'names' && rest.length === 1) {
    for (const name of releaseAssetNames(rest[0], { signed })) console.log(name)
  } else if (command === 'check' && rest.length === 2) {
    process.stdout.write(await check(rest[0], rest[1], { signed }))
  } else if (command === 'desktop-sums' && given.length === 2) {
    console.log(await writeDesktopSums(given[0], given[1]))
  } else {
    console.error(
      'usage: node release.mjs gate <version> [--dry-run] < tags | names <version> [--unsigned] | check <folder> <version> [--unsigned] | desktop-sums <folder> <version>'
    )
    return 2
  }
  return 0
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    process.exitCode = await main(process.argv.slice(2))
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error))
    process.exitCode = 1
  }
}
