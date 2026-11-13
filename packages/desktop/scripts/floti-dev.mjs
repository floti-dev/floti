// What a stable release copies to floti.dev, for .github/workflows/release.yml:
// the install scripts `curl -fsSL https://floti.dev/install.sh | sh` and
// `irm https://floti.dev/install.ps1 | iex` run, and update.json, the manifest
// `floti update` reads (crates/host/src/update/manifest.rs).
//
// The scripts are scripts/floti.dev/install.sh.template and install.ps1.template
// with the release's version, its download folder and each floti's SHA-256
// from the release's floti-<version>-SHA256SUMS written in, and the install
// function of scripts/install/install.sh or install.ps1, the one the desktop
// sends a computer it installs floti on, carried inside. Each is one function
// called on its last line, so a script cut off halfway runs nothing.
//
// update.json names one channel, `stable`; a preview writes none (decision
// I13). Its floti files are the SHA-256 list's, and its desktop files the
// setup exe (windows-x64), the Mac zip (macos-arm64) and the AppImage
// (linux-x64), each hashed here.
//
//   node floti-dev.mjs scripts <sums> <version> <owner/repo> <folder> [--release-url <url>]
//     writes install.sh and install.ps1 into <folder>. They download from the
//     release's own folder, or from <url>: a dry run gives its local server,
//     http on this computer. A published script has no switch of its own.
//
//   node floti-dev.mjs manifest <release folder> <version> <owner/repo> <file> [--as-stable]
//     writes update.json for the release whose files are in <release folder>.
//     With --as-stable, a preview's files are named as a stable release of
//     the same number would name them, for a dry run to check that form.
//
//   node floti-dev.mjs check-scripts <folder> <sums> <version> <owner/repo>
//     refuses install.sh and install.ps1 in <folder> unless each is, byte for
//     byte, the script this commit writes from <sums>.
//
//   node floti-dev.mjs check-manifest <update.json> <sums> <version> <owner/repo>
//     refuses a manifest that does not name <version> alone, on the stable
//     channel, with exactly the floti files and SHA-256s <sums> gives.
//
// `floti-dev` runs both checks with the SHA-256 list the release published,
// after its signature is checked against the committed public key.

import { createHash } from 'node:crypto'
import { createReadStream, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { pipeline } from 'node:stream/promises'
import { fileURLToPath } from 'node:url'
import { parseArgs } from 'node:util'
import { RELEASE_TARGETS, sumsFileName } from './floti-release.mjs'
import { downloadUrl, releaseFiles } from './package-managers.mjs'
import { parseVersion } from './release.mjs'

const REPO_ROOT = resolve(import.meta.dirname, '../../..')

/** The manifest schema `floti update` reads (`SCHEMA` in manifest.rs). */
export const MANIFEST_SCHEMA = 1

/** The one channel a release writes. */
export const STABLE_CHANNEL = 'stable'

/** The desktop file `floti update` installs on each target that has one. */
export const DESKTOP_FILES = Object.freeze({
  'windows-x64': (version) => `Floti-${version}-setup.exe`,
  'macos-arm64': (version) => `Floti-${version}-arm64-mac.zip`,
  'linux-x64': (version) => `Floti-${version}-linux-x64.AppImage`
})

/** Each install script: its template, the install script it carries, and the targets it installs. */
export const INSTALL_SCRIPTS = Object.freeze([
  {
    name: 'install.sh',
    template: join(REPO_ROOT, 'scripts', 'floti.dev', 'install.sh.template'),
    install: join(REPO_ROOT, 'scripts', 'install', 'install.sh'),
    targets: RELEASE_TARGETS.filter((target) => target.os !== 'windows').map((target) => target.name)
  },
  {
    name: 'install.ps1',
    template: join(REPO_ROOT, 'scripts', 'floti.dev', 'install.ps1.template'),
    install: join(REPO_ROOT, 'scripts', 'install', 'install.ps1'),
    targets: RELEASE_TARGETS.filter((target) => target.os === 'windows').map((target) => target.name)
  }
])

const SHA256 = /^[0-9a-f]{64}$/

/**
 * Why `url` cannot be the folder a script downloads from, or null when it
 * can: https, or http to this computer for a dry run's local server, with
 * nothing a single-quoted shell or PowerShell string would read as its end.
 */
export function releaseUrlProblem(url) {
  if (!/^[A-Za-z0-9:/._~%-]+$/.test(url)) return `'${url}' holds a character a release folder's address never does`
  let parsed
  try {
    parsed = new URL(url)
  } catch {
    return `'${url}' is not an address`
  }
  const loopback = parsed.hostname === '127.0.0.1' || parsed.hostname === 'localhost'
  if (parsed.protocol !== 'https:' && !(parsed.protocol === 'http:' && loopback)) {
    return `'${url}' is neither https nor http to this computer`
  }
  if (parsed.search !== '' || parsed.hash !== '') return `'${url}' has a query or a fragment`
  return null
}

/** The release's download folder, without its trailing slash, as the scripts join `<folder>/<file>`. */
export function releaseFolder(repo, version) {
  return downloadUrl(repo, version, '').replace(/\/$/, '')
}

/**
 * The install function of an install script: everything from its first line
 * that is not a comment to the line before the last, which calls it. Refuses
 * a script that is not one function called on its last line.
 */
export function installFunction(text) {
  const lines = text.replace(/\r\n/g, '\n').trimEnd().split('\n')
  const start = lines.findIndex((line) => line.trim() !== '' && !line.startsWith('#'))
  const body = lines.slice(start, -1).join('\n').trimEnd().split('\n')
  const name = /^(?:function ([A-Za-z-]+) \{|([a-z_]+)\(\) \{)$/.exec(body[0] ?? '')
  const called = name !== null && (lines.at(-1) ?? '').includes(name[1] ?? name[2] ?? '')
  if (start === -1 || body.length < 2 || !called || body.at(-1) !== '}') {
    throw new Error('an install script must be one function, from its first line that is not a comment, called on its last line')
  }
  return body.join('\n')
}

/**
 * `template` with each placeholder filled: `{{install_function}}` on a line
 * of its own, indented into the function around it, `{{version}}`,
 * `{{release_url}}` and `{{sha256 <target>}}`. Refuses a placeholder it does
 * not know, a target it has no SHA-256 for, and a result that is not ASCII:
 * PowerShell decodes a download without a charset as Latin-1.
 */
export function fillTemplate(template, { installFunction: install, version, releaseUrl, sha256 }) {
  const indented = install
    .split('\n')
    .map((line) => (line === '' ? '' : `  ${line}`))
    .join('\n')
  const text = template.replace(/\r\n/g, '\n').replace(/\{\{([^{}]*)\}\}/g, (whole, name) => {
    if (name === 'install_function') return indented
    if (name === 'version') return version
    if (name === 'release_url') return releaseUrl
    const target = /^sha256 ([a-z0-9-]+)$/.exec(name)?.[1]
    if (target === undefined) throw new Error(`the template has a placeholder no release fills: ${whole}`)
    const sum = sha256.get(target)
    if (sum === undefined) throw new Error(`the template asks for the SHA-256 of ${target}, which the release does not list`)
    return sum
  })
  if (/[^\t\n -~]/.test(text)) throw new Error('an install script must be printable ASCII, tabs and newlines')
  return text
}

/**
 * floti.dev's install scripts for floti `version`, as `{ name, text }`, from
 * the release's SHA-256 list `sums`, downloading from `releaseUrl` (the
 * release's own folder unless a dry run gives its server). `read` reads a
 * template or install script by path.
 */
export function installScripts({ sums, version, repo, releaseUrl = releaseFolder(repo, version), read = (path) => readFileSync(path, 'utf8') }) {
  const problem = releaseUrlProblem(releaseUrl)
  if (problem !== null) throw new Error(problem)
  const folder = releaseUrl.replace(/\/$/, '')
  return INSTALL_SCRIPTS.map((script) => {
    const files = releaseFiles(sums, version, repo, script.targets)
    const sha256 = new Map([...files].map(([target, file]) => [target, file.sha256]))
    const text = fillTemplate(read(script.template), {
      installFunction: installFunction(read(script.install)),
      version,
      releaseUrl: folder,
      sha256
    })
    const missing = script.targets.filter((target) => !text.includes(sha256.get(target)))
    if (missing.length > 0) throw new Error(`${script.name}'s template installs no floti for ${missing.join(', ')}`)
    return { name: script.name, text }
  })
}

/**
 * update.json for the stable release `version`, downloading from `url` (the
 * release's folder, ending in `/`): `floti` and `desktop` map each target to
 * `{ file, sha256 }`. Refuses a preview, which writes no manifest, and a
 * file entry `floti update` would refuse.
 */
export function updateManifest({ version, url, floti, desktop }) {
  const parsed = parseVersion(version)
  if (parsed === null) throw new Error(`'${version}' is not a release version`)
  if (parsed.pre.length > 0) throw new Error(`${version} is a preview, and a preview writes no update manifest`)
  if (!url.startsWith('https://') || !url.endsWith('/')) throw new Error(`'${url}' is not an https folder ending in /`)
  const entries = (files) =>
    Object.fromEntries(
      RELEASE_TARGETS.filter((target) => files.has(target.name)).map((target) => {
        const { file, sha256 } = files.get(target.name)
        if (!/^[A-Za-z0-9_+-][A-Za-z0-9._+-]*$/.test(file)) throw new Error(`'${file}' is not a plain file name`)
        if (!SHA256.test(sha256)) throw new Error(`${file}'s SHA-256 is not 64 lowercase hex digits`)
        return [target.name, { file, sha256 }]
      })
    )
  return {
    schema: MANIFEST_SCHEMA,
    channels: { [STABLE_CHANNEL]: { version, url, floti: entries(floti), desktop: entries(desktop) } }
  }
}

/** A manifest as the release writes it: two-space JSON and a final newline. */
export function manifestText(manifest) {
  return `${JSON.stringify(manifest, null, 2)}\n`
}

/** `<major>.<minor>.<patch>` of `version`, without its pre-release part. */
export function stableVersion(version) {
  const parsed = parseVersion(version)
  if (parsed === null) throw new Error(`'${version}' is not a release version`)
  return `${parsed.major}.${parsed.minor}.${parsed.patch}`
}

/**
 * Why the install scripts in `scripts` (name to text) are not, byte for byte,
 * the ones this commit writes for floti `version` from the SHA-256 list
 * `sums`: one line per script that differs or is missing, none when both
 * match. `floti-dev` gives it the list the release published, after its
 * signature is checked, so a script carrying any other SHA-256 is refused.
 */
export function scriptsProblems({ scripts, sums, version, repo, read }) {
  const problems = []
  for (const { name, text } of installScripts({ sums, version, repo, ...(read === undefined ? {} : { read }) })) {
    const found = scripts.get(name)
    if (found === undefined) problems.push(`${name} is missing`)
    else if (found !== text) problems.push(`${name} is not the script ${version}'s signed SHA-256 list makes`)
  }
  return problems
}

/**
 * Why the manifest `text` does not name floti `version` alone, on the stable
 * channel, at the release's own folder, with exactly the floti files and
 * SHA-256s the list `sums` gives: one line per difference, none when it
 * does. Its desktop files are not in that list; its signature covers them.
 */
export function manifestProblems({ text, sums, version, repo }) {
  let manifest
  try {
    manifest = JSON.parse(text)
  } catch (error) {
    return [`update.json is not JSON: ${error instanceof Error ? error.message : String(error)}`]
  }
  const problems = []
  if (manifest?.schema !== MANIFEST_SCHEMA) problems.push(`update.json is schema ${String(manifest?.schema)}, not ${MANIFEST_SCHEMA}`)
  const channels = Object.keys(manifest?.channels ?? {})
  if (channels.length !== 1 || channels[0] !== STABLE_CHANNEL) problems.push(`update.json names the channels [${channels.join(', ')}], not ${STABLE_CHANNEL} alone`)
  const stable = manifest?.channels?.[STABLE_CHANNEL] ?? {}
  if (stable.version !== version) problems.push(`update.json names version ${String(stable.version)}, not ${version}`)
  const url = downloadUrl(repo, version, '')
  if (stable.url !== url) problems.push(`update.json downloads from ${String(stable.url)}, not ${url}`)
  const expected = releaseFiles(sums, version, repo, RELEASE_TARGETS.map((target) => target.name))
  const floti = stable.floti ?? {}
  for (const [target, { name, sha256 }] of expected) {
    const entry = floti[target]
    if (entry?.file !== name || entry?.sha256 !== sha256) problems.push(`update.json's floti for ${target} is not ${name} with the SHA-256 the list gives`)
  }
  for (const target of Object.keys(floti)) {
    if (!expected.has(target)) problems.push(`update.json names a floti for ${target}, which the list does not`)
  }
  return problems
}

async function sha256OfFile(file) {
  const hash = createHash('sha256')
  await pipeline(createReadStream(file), hash)
  return hash.digest('hex')
}

/**
 * The manifest of the release whose files are in `folder`: floti from its
 * SHA-256 list, the desktop files hashed. With `asStable`, a preview's files
 * renamed as a stable release of the same number names them.
 */
export async function releaseManifest({ folder, version, repo, asStable = false }) {
  const sums = readFileSync(join(folder, sumsFileName(version)), 'utf8')
  const all = RELEASE_TARGETS.map((target) => target.name)
  const named = asStable ? stableVersion(version) : version
  const rename = (file) => (asStable ? file.replace(`-${version}`, `-${named}`) : file)
  const floti = new Map([...releaseFiles(sums, version, repo, all)].map(([target, file]) => [target, { file: rename(file.name), sha256: file.sha256 }]))
  const desktop = new Map()
  for (const [target, nameFor] of Object.entries(DESKTOP_FILES)) {
    const file = nameFor(version)
    desktop.set(target, { file: rename(file), sha256: await sha256OfFile(join(folder, file)) })
  }
  return updateManifest({ version: named, url: downloadUrl(repo, named, ''), floti, desktop })
}

async function main(args) {
  const [command, ...rest] = args
  const { values, positionals } = parseArgs({
    args: rest,
    options: { 'release-url': { type: 'string' }, 'as-stable': { type: 'boolean' } },
    allowPositionals: true
  })
  if (command === 'scripts' && positionals.length === 4 && values['as-stable'] === undefined) {
    const [sumsFile, version, repo, folder] = positionals
    const scripts = installScripts({
      sums: readFileSync(sumsFile, 'utf8'),
      version,
      repo,
      ...(values['release-url'] === undefined ? {} : { releaseUrl: values['release-url'] })
    })
    mkdirSync(folder, { recursive: true })
    for (const { name, text } of scripts) {
      writeFileSync(join(folder, name), text)
      console.log(`${name}: floti ${version}, from ${values['release-url'] ?? releaseFolder(repo, version)}`)
    }
    return 0
  }
  if (command === 'manifest' && positionals.length === 4 && values['release-url'] === undefined) {
    const [folder, version, repo, file] = positionals
    const manifest = await releaseManifest({ folder, version, repo, asStable: values['as-stable'] === true })
    writeFileSync(file, manifestText(manifest))
    process.stdout.write(manifestText(manifest))
    return 0
  }
  const checked = command === 'check-scripts' || command === 'check-manifest'
  if (checked && positionals.length === 4 && values['release-url'] === undefined && values['as-stable'] === undefined) {
    const [path, sumsFile, version, repo] = positionals
    const sums = readFileSync(sumsFile, 'utf8')
    const problems =
      command === 'check-scripts'
        ? scriptsProblems({
            scripts: new Map(
              INSTALL_SCRIPTS.filter(({ name }) => existsSync(join(path, name))).map(({ name }) => [name, readFileSync(join(path, name), 'utf8')])
            ),
            sums,
            version,
            repo
          })
        : manifestProblems({ text: readFileSync(path, 'utf8'), sums, version, repo })
    if (problems.length > 0) {
      for (const problem of problems) console.error(process.env.GITHUB_ACTIONS === 'true' ? `::error::${problem}` : problem)
      return 1
    }
    console.log(command === 'check-scripts' ? `install.sh and install.ps1 are ${version}'s, from ${sumsFile}` : `update.json names ${version}'s floti files, from ${sumsFile}`)
    return 0
  }
  console.error(
    'usage: node floti-dev.mjs scripts <sums> <version> <owner/repo> <folder> [--release-url <url>] | manifest <release folder> <version> <owner/repo> <file> [--as-stable] | check-scripts <folder> <sums> <version> <owner/repo> | check-manifest <update.json> <sums> <version> <owner/repo>'
  )
  return 2
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    process.exitCode = await main(process.argv.slice(2))
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error))
    process.exitCode = 1
  }
}
