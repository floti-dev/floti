// The third-party notices every download of Floti carries: the license texts
// of the code from other projects that ships inside it.
//
//   node third-party-notices.mjs floti <out-file>
//     writes floti's notices: every Rust crate `floti-host` links for a target
//     a release carries floti for (RELEASE_TARGETS in floti-release.mjs), as
//     `cargo metadata` resolves them, each with its own license files. The
//     release carries the file as `floti-<version>-THIRD-PARTY-NOTICES.txt`
//     beside the five floti files (.github/workflows/floti.yml writes it).
//
//   node third-party-notices.mjs desktop <out-file> [--floti <floti-notices>]
//     writes the desktop app's: every npm package the app carries or bundles
//     (the production dependencies of packages/desktop, through the workspace
//     packages it bundles), then floti's notices verbatim, since the app
//     carries floti. Without --floti it writes floti's itself, which needs
//     cargo. scripts/stage-floti.mjs runs this for every desktop build, and
//     electron-builder.yml carries the file as `THIRD-PARTY-NOTICES.txt` in
//     the app's resources.
//
//   node third-party-notices.mjs check <app-notices> <floti-notices>
//     refuses (exit 1) an app's notices that do not hold floti's whole.
//
//   node third-party-notices.mjs vendor-v8
//     writes `third-party/v8-<version>.txt` at the repository's root for the
//     v8 crate Cargo.lock names. That crate's package leaves out every license
//     file, and the static library it links into floti, librusty_v8, holds V8
//     and the libraries V8_LIBRARIES names. The file holds their license files
//     at the commits rusty_v8's tag `v<version>` pins, read from GitHub and
//     chromium.googlesource.com with no token. `floti` fails while the file
//     for the v8 version Cargo.lock names is missing, so a v8 update is the
//     change that runs this again.
//
// A package with no license file of its own is given, in order: the vendored
// file VENDORED names for it; the license files of a package in the same set
// from the same repository under the same license; the MIT License text with
// the copyright its manifest names, when its license is a choice that offers
// MIT; this repository's own Apache-2.0 text, when it offers Apache-2.0. A
// package none of those covers fails the command, naming it, so a dependency
// with no notice is a decision for the change that adds it.
//
// The output is plain text with LF line ends, sorted, and the same for the
// same packages: each distinct license text appears once, after the list of
// packages it covers.

import { execFileSync } from 'node:child_process'
import { existsSync, mkdirSync, readFileSync, readdirSync, realpathSync, statSync, writeFileSync } from 'node:fs'
import { basename, dirname, join, relative, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'
import { RELEASE_TARGETS } from './floti-release.mjs'

const DESKTOP_DIR = resolve(import.meta.dirname, '..')
const REPO_ROOT = resolve(DESKTOP_DIR, '..', '..')

/** Where the vendored license files live: `third-party/` at the repository's root. */
export const VENDORED_DIR = join(REPO_ROOT, 'third-party')

/** The name the desktop app carries its notices under, in its resources. */
export const APP_NOTICES_NAME = 'THIRD-PARTY-NOTICES.txt'

/** The start of the name of a file or folder that holds a license or notices. */
export const LICENSE_FILE = /^(licen[cs]e|copying|notice|copyright|unlicense)/i

/**
 * A name LICENSE_FILE matches that is not a license file: source code
 * (`copying-phase.cc` in V8), or an HTML page, which is markup around a text
 * (ICU's `license.html` only says the license moved to LICENSE).
 */
const NOT_A_LICENSE = /\.(html?|c|cc|cpp|cxx|h|hh|hpp|rs|js|mjs|cjs|ts|py|go|java|json|toml|ya?ml|sh|gni?)$/i

/** Whether a file or folder named `name` is a license file, or a folder of them. */
export function isLicenseFileName(name) {
  return LICENSE_FILE.test(name) && !NOT_A_LICENSE.test(name)
}

/**
 * Folders a nested search for license files does not enter: tests, examples,
 * benchmarks and documentation, which no build links, another package's
 * install, and hidden folders.
 */
const NOT_LINKED = /^(tests?|testdata|test-data|fixtures?|examples?|benches|benchmarks?|docs?|node_modules|\..*)$/i

/** How deep a nested search goes below a package's root. */
const NESTED_DEPTH = 6

export const FLOTI_TITLE = 'Third-party notices: floti'
export const DESKTOP_TITLE = 'Third-party notices: Floti'

const RULE = '='.repeat(80)
const THIN_RULE = '-'.repeat(80)

/** Code-unit order, which is the same on every machine and in every locale. */
const compare = (a, b) => (a < b ? -1 : a > b ? 1 : 0)
const byNameVersion = (a, b) => compare(a.name, b.name) || compare(a.version, b.version)

/**
 * A license text as the notices hold it: LF line ends, no byte-order mark, no
 * whitespace at the end of a line or of the text.
 */
export function normalizeText(text) {
  const unmarked = text.charCodeAt(0) === 0xfeff ? text.slice(1) : text
  return unmarked
    .replace(/\r\n?/g, '\n')
    .split('\n')
    .map((line) => line.trimEnd())
    .join('\n')
    .trim()
}

/**
 * The license files of the package in `dir`: each file at its root whose name
 * isLicenseFileName accepts, each file one level inside such a folder (a
 * `LICENSES/` folder), and each of `named` (Cargo's `license-file`), as
 * `{ name, text }` sorted by name, with empty files left out. With `nested`,
 * also each such file in the folders below the root, outside NOT_LINKED ones,
 * to NESTED_DEPTH: a package that vendors another project's code keeps that
 * project's license beside it (ring's `third_party/fiat/LICENSE`).
 */
export function licenseFilesIn(dir, named = [], { nested = false } = {}) {
  const files = new Map()
  const addFile = (path) => {
    const text = normalizeText(readFileSync(path, 'utf8'))
    if (text !== '') files.set(relative(dir, path).split(sep).join('/'), text)
  }
  const add = (path) => {
    if (!existsSync(path)) return
    if (!statSync(path).isDirectory()) return addFile(path)
    for (const name of readdirSync(path).sort(compare)) {
      const inner = join(path, name)
      if (statSync(inner).isFile()) addFile(inner)
    }
  }
  for (const name of readdirSync(dir).sort(compare)) if (isLicenseFileName(name)) add(join(dir, name))
  for (const name of named) add(resolve(dir, name))
  if (nested) {
    const walk = (folder, depth) => {
      for (const entry of readdirSync(folder, { withFileTypes: true }).sort((a, b) => compare(a.name, b.name))) {
        const path = join(folder, entry.name)
        if (entry.isDirectory() && !NOT_LINKED.test(entry.name) && depth < NESTED_DEPTH) walk(path, depth + 1)
        else if (entry.isFile() && isLicenseFileName(entry.name)) addFile(path)
      }
    }
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      if (entry.isDirectory() && !NOT_LINKED.test(entry.name) && !isLicenseFileName(entry.name)) walk(join(dir, entry.name), 1)
    }
  }
  return [...files].sort(([a], [b]) => compare(a, b)).map(([name, text]) => ({ name, text }))
}

/**
 * A repository URL as two packages from one repository both give it, or null
 * for none: https, no `git+`, `.git`, fragment or trailing slash, a GitHub,
 * GitLab or Bitbucket URL cut to its owner and name, an npm `owner/name` or
 * `github:owner/name` shorthand spelled out, lower case.
 */
export function normalizeRepository(url) {
  if (typeof url !== 'string' || url.trim() === '') return null
  let text = url.trim()
  const shorthand = /^(?:(github|gitlab|bitbucket):)?([\w.-]+)\/([\w.-]+)$/i.exec(text)
  if (shorthand !== null) {
    const host = { github: 'github.com', gitlab: 'gitlab.com', bitbucket: 'bitbucket.org' }[(shorthand[1] ?? 'github').toLowerCase()]
    text = `https://${host}/${shorthand[2]}/${shorthand[3]}`
  }
  text = text
    .replace(/^git\+/, '')
    .replace(/^git:\/\//, 'https://')
    .replace(/^ssh:\/\/git@/, 'https://')
    .replace(/^git@([^:]+):/, 'https://$1/')
    .replace(/^http:\/\//, 'https://')
    .replace(/#.*$/, '')
    .replace(/\/+$/, '')
    .replace(/\.git$/, '')
  const hosted = /^https:\/\/(github\.com|gitlab\.com|bitbucket\.org)\/([^/]+)\/([^/]+)/i.exec(text)
  if (hosted !== null) text = `https://${hosted[1]}/${hosted[2]}/${hosted[3]}`
  return text.toLowerCase()
}

/**
 * The licenses an SPDX expression lets a recipient choose between (`A OR B`,
 * or Cargo's older `A/B`), each as written (`X WITH Y` stays one), or null
 * when it names none or combines them with AND, which no single text covers.
 */
export function licenseChoices(expression) {
  if (typeof expression !== 'string' || /\bAND\b/i.test(expression)) return null
  const choices = expression
    .replace(/[()]/g, ' ')
    .split(/\s+OR\s+|\s*\/\s*/i)
    .map((choice) => choice.trim().replace(/\s+/g, ' '))
    .filter((choice) => choice !== '')
  return choices.length > 0 ? choices : null
}

/** The MIT License text with `holders` as its copyright. */
export function mitText(holders) {
  return [
    'MIT License',
    '',
    `Copyright (c) ${holders}`,
    '',
    'Permission is hereby granted, free of charge, to any person obtaining a copy',
    'of this software and associated documentation files (the "Software"), to deal',
    'in the Software without restriction, including without limitation the rights',
    'to use, copy, modify, merge, publish, distribute, sublicense, and/or sell',
    'copies of the Software, and to permit persons to whom the Software is',
    'furnished to do so, subject to the following conditions:',
    '',
    'The above copyright notice and this permission notice shall be included in all',
    'copies or substantial portions of the Software.',
    '',
    'THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR',
    'IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,',
    'FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE',
    'AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER',
    'LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,',
    'OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE',
    'SOFTWARE.'
  ].join('\n')
}

/** Authors as a copyright line names them: their names without e-mail addresses, or "the <name> authors". */
export function copyrightHolders(pkg) {
  const names = pkg.authors.map((author) => author.replace(/\s*<[^>]*>/g, '').replace(/\s*\([^)]*\)/g, '').trim()).filter(Boolean)
  return names.length > 0 ? names.join(', ') : `the ${pkg.name} authors`
}

/** `v8-<version>.txt`: the vendored notices for the v8 crate at `version`. */
export function v8VendoredName(version) {
  return `v8-${version}.txt`
}

/**
 * The crates whose package leaves out license files they need, each with the
 * vendored file that holds them and how that file is written again.
 */
export const VENDORED = Object.freeze({
  v8: { file: (version) => v8VendoredName(version), how: 'node packages/desktop/scripts/third-party-notices.mjs vendor-v8' }
})

/**
 * Every crate the workspace package `root` links, from `cargo metadata`
 * output: its normal dependencies and theirs, never a dev- or build-only one,
 * and no workspace or path package, sorted by name and version. Each is
 * `{ name, version, license, repository, authors, dir, named }`, `named`
 * holding Cargo's `license-file` when it gives one.
 */
export function crateClosure(metadata, root = 'floti-host') {
  const packages = new Map(metadata.packages.map((pkg) => [pkg.id, pkg]))
  const nodes = new Map(metadata.resolve.nodes.map((node) => [node.id, node]))
  const start = metadata.packages.find((pkg) => pkg.name === root && pkg.source === null)
  if (start === undefined) throw new Error(`cargo metadata names no workspace package ${root}`)
  const seen = new Set()
  const stack = [start.id]
  while (stack.length > 0) {
    const id = stack.pop()
    if (seen.has(id)) continue
    seen.add(id)
    const node = nodes.get(id)
    if (node === undefined) throw new Error(`cargo metadata resolves no dependencies for ${id}`)
    for (const dep of node.deps) if (dep.dep_kinds.some((kind) => kind.kind === null)) stack.push(dep.pkg)
  }
  return [...seen]
    .map((id) => packages.get(id))
    .filter((pkg) => pkg.source !== null)
    .map((pkg) => ({
      name: pkg.name,
      version: pkg.version,
      license: pkg.license ?? null,
      repository: pkg.repository ?? null,
      authors: pkg.authors ?? [],
      dir: dirname(pkg.manifest_path),
      named: typeof pkg.license_file === 'string' ? [pkg.license_file] : []
    }))
    .sort(byNameVersion)
}

/** `cargo metadata` for the workspace at `repoRoot`, filtered to the targets a release carries floti for. */
export function cargoMetadata(repoRoot = REPO_ROOT) {
  const platforms = RELEASE_TARGETS.flatMap((target) => ['--filter-platform', target.triple])
  const output = execFileSync('cargo', ['metadata', '--format-version', '1', '--locked', ...platforms], {
    cwd: repoRoot,
    encoding: 'utf8',
    maxBuffer: 512 * 1024 * 1024,
    stdio: ['ignore', 'pipe', 'inherit']
  })
  return JSON.parse(output)
}

/** A package.json's license as one expression: `license`, its `type`, or the old `licenses` list as a choice. */
function npmLicense(manifest) {
  if (typeof manifest.license === 'string') return manifest.license
  if (typeof manifest.license?.type === 'string') return manifest.license.type
  if (Array.isArray(manifest.licenses)) {
    const types = manifest.licenses.map((each) => (typeof each === 'string' ? each : each?.type)).filter((each) => typeof each === 'string')
    if (types.length > 0) return types.join(' OR ')
  }
  return null
}

function npmRepository(manifest) {
  const repository = typeof manifest.repository === 'string' ? manifest.repository : manifest.repository?.url
  return typeof repository === 'string' ? repository : null
}

function npmAuthors(manifest) {
  const author = typeof manifest.author === 'string' ? manifest.author : manifest.author?.name
  return typeof author === 'string' && author.trim() !== '' ? [author] : []
}

/** The folder of package `name` as Node resolves it from `fromDir`, through each `node_modules` above, or null. */
function resolvePackage(name, fromDir) {
  for (let dir = fromDir; ; dir = dirname(dir)) {
    const manifest = join(dir, 'node_modules', name, 'package.json')
    if (existsSync(manifest)) return realpathSync(dirname(manifest))
    if (dirname(dir) === dir) return null
  }
}

const isWorkspacePackage = (dir) => !dir.split(sep).includes('node_modules')

/**
 * Every npm package the desktop app at `desktopDir` carries or bundles: its
 * `dependencies`, and through each workspace package among them that
 * package's own, each followed through its `dependencies` and the
 * `optionalDependencies` this install has, as Node resolves them. Workspace
 * packages are Floti's own, so they are walked through and not listed. Sorted
 * by name and version, in the shape crateClosure gives. A dependency the
 * install lacks is an error: notices from a broken install would be wrong.
 */
export function npmClosure(desktopDir = DESKTOP_DIR) {
  const found = new Map()
  const visited = new Set()
  const walk = (dir) => {
    if (visited.has(dir)) return
    visited.add(dir)
    const manifest = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'))
    const workspace = isWorkspacePackage(dir)
    if (!workspace) {
      const key = `${manifest.name}@${manifest.version}`
      if (!found.has(key)) {
        found.set(key, {
          name: manifest.name,
          version: manifest.version,
          license: npmLicense(manifest),
          repository: npmRepository(manifest),
          authors: npmAuthors(manifest),
          dir,
          named: []
        })
      }
    }
    const required = Object.keys(manifest.dependencies ?? {})
    const optional = Object.keys(manifest.optionalDependencies ?? {})
    for (const name of [...required, ...optional].sort(compare)) {
      const at = resolvePackage(name, dir)
      if (at === null) {
        if (required.includes(name)) throw new Error(`${manifest.name} depends on ${name}, which is not installed: run pnpm install`)
        continue
      }
      walk(at)
    }
  }
  walk(realpathSync(desktopDir))
  return [...found.values()].sort(byNameVersion)
}

/** The Apache License 2.0 text this repository is licensed under, its own LICENSE. */
export function repositoryApacheText(repoRoot = REPO_ROOT) {
  const text = normalizeText(readFileSync(join(repoRoot, 'LICENSE'), 'utf8'))
  if (!text.startsWith('Apache License')) throw new Error(`${join(repoRoot, 'LICENSE')} is not the Apache License 2.0 text`)
  return text
}

/**
 * Each package's notice texts, `{ pkg, texts: [{ text, how }] }` in the
 * packages' order, `how` saying where a text not from the package itself came
 * from; and every package none of the fallbacks covers, `{ pkg, why }`.
 */
export function resolveNotices(packages, { vendoredDir = VENDORED_DIR, apacheText }) {
  // A vendored crate's file is its notices; its own tree is not searched
  // (the v8 crate's holds 200 MB of V8's sources).
  const own = new Map(packages.map((pkg) => [pkg, licenseFilesIn(pkg.dir, pkg.named, { nested: !Object.hasOwn(VENDORED, pkg.name) })]))
  const resolved = []
  const missing = []
  for (const pkg of packages) {
    const texts = own.get(pkg).map(({ text }) => ({ text, how: null }))
    const vendored = Object.hasOwn(VENDORED, pkg.name) ? VENDORED[pkg.name] : undefined
    if (vendored !== undefined) {
      const file = join(vendoredDir, vendored.file(pkg.version))
      if (!existsSync(file)) {
        missing.push({ pkg, why: `its notices are vendored, and ${relative(REPO_ROOT, file).split(sep).join('/')} is missing: run ${vendored.how}` })
        continue
      }
      texts.push({ text: normalizeText(readFileSync(file, 'utf8')), how: `vendored as ${basename(file)}` })
    }
    if (texts.length === 0) {
      const repository = normalizeRepository(pkg.repository)
      const sibling =
        repository === null
          ? undefined
          : packages.find((other) => other !== pkg && own.get(other).length > 0 && other.license === pkg.license && normalizeRepository(other.repository) === repository)
      if (sibling !== undefined) {
        for (const { text } of own.get(sibling)) texts.push({ text, how: `license file of ${sibling.name} ${sibling.version}, from the same repository` })
      }
    }
    if (texts.length === 0) {
      const choices = licenseChoices(pkg.license) ?? []
      if (choices.includes('MIT')) texts.push({ text: mitText(copyrightHolders(pkg)), how: 'ships no license file: the MIT License, with the copyright its manifest names' })
      else if (choices.includes('Apache-2.0')) texts.push({ text: apacheText, how: 'ships no license file: the Apache License 2.0' })
    }
    if (texts.length === 0) missing.push({ pkg, why: `it ships no license file, and ${pkg.license ?? 'no license'} is neither a choice of MIT nor of Apache-2.0` })
    else resolved.push({ pkg, texts })
  }
  return { resolved, missing }
}

/** An error naming every package with no notice, and why. */
export function missingError(missing) {
  const lines = missing.map(({ pkg, why }) => `  ${pkg.name} ${pkg.version} (${pkg.license ?? 'no license'}): ${why}`)
  return new Error(`no notice for ${missing.length === 1 ? 'one package' : `${missing.length} packages`}:\n${lines.join('\n')}`)
}

/**
 * The notices as text: the title, the introduction, every package with its
 * license, then each distinct license text once, after the packages it
 * covers. Texts that differ only in whitespace are one text, printed as the
 * first package to use it has it.
 */
export function renderNotices({ title, intro, resolved }) {
  const lines = [title, '='.repeat(title.length), '', intro.trim(), '', 'Packages', '--------', '']
  for (const { pkg } of resolved) lines.push(`  ${pkg.name} ${pkg.version} (${pkg.license ?? 'no license named'})`)
  const sections = new Map()
  for (const { pkg, texts } of resolved) {
    for (const { text, how } of texts) {
      const key = text.replace(/\s+/g, ' ')
      if (!sections.has(key)) sections.set(key, { text, users: [] })
      const user = `${pkg.name} ${pkg.version}${how === null ? '' : ` (${how})`}`
      const users = sections.get(key).users
      if (!users.includes(user)) users.push(user)
    }
  }
  lines.push('', '', 'License texts', '-------------')
  for (const { text, users } of sections.values()) {
    lines.push('', RULE, 'The license text below covers:', ...users.map((user) => `  ${user}`), THIN_RULE, '', text)
  }
  return `${lines.join('\n')}\n`
}

/** floti's notices, from `cargo metadata` output. Throws naming every crate with no notice. */
export function flotiNotices({ metadata = cargoMetadata(), vendoredDir = VENDORED_DIR, apacheText = repositoryApacheText() } = {}) {
  const { resolved, missing } = resolveNotices(crateClosure(metadata), { vendoredDir, apacheText })
  if (missing.length > 0) throw missingError(missing)
  return renderNotices({
    title: FLOTI_TITLE,
    intro: [
      "floti, Floti's host program, is built from the Rust crates listed below.",
      'Each license text appears once, after the list of crates it covers.'
    ].join('\n'),
    resolved
  })
}

/**
 * The desktop app's notices: its npm packages', then `flotiText`, floti's, as
 * flotiNotices writes them. Throws naming every package with no notice, and
 * for a `flotiText` that is not floti's notices.
 */
export function desktopNotices({ desktopDir = DESKTOP_DIR, flotiText, vendoredDir = VENDORED_DIR, apacheText = repositoryApacheText() }) {
  if (!flotiText.startsWith(`${FLOTI_TITLE}\n`)) throw new Error(`the floti notices given do not start with "${FLOTI_TITLE}"`)
  const { resolved, missing } = resolveNotices(npmClosure(desktopDir), { vendoredDir, apacheText })
  if (missing.length > 0) throw missingError(missing)
  const own = renderNotices({
    title: DESKTOP_TITLE,
    intro: [
      'Floti carries the npm packages listed below, and floti, its host program,',
      'whose notices follow theirs. Each license text appears once, after the list',
      'of packages it covers.',
      '',
      "Electron's license and Chromium's are LICENSE.electron.txt and",
      "LICENSES.chromium.html, which ship with the app, and Node's is the LICENSE",
      'file in the node folder beside floti.'
    ].join('\n'),
    resolved
  })
  return `${own}\n\n${flotiText.endsWith('\n') ? flotiText : `${flotiText}\n`}`
}

/** Whether an app's notices hold floti's whole. */
export function noticesHold(appText, flotiText) {
  return normalizeLineEnds(appText).includes(normalizeLineEnds(flotiText).trimEnd())
}

const normalizeLineEnds = (text) => text.replace(/\r\n?/g, '\n')

// --- vendor-v8 ---------------------------------------------------------------

/**
 * The libraries librusty_v8 links that need a notice, by their path in
 * rusty_v8's repository, where each is a git submodule. libc++, libc++abi,
 * libunwind and LLVM's libc are linked too and left out: they are
 * Apache-2.0 WITH LLVM-exception, whose exception waives the notice
 * conditions for their compiled object code. `fallback` is the file to carry
 * for a library whose own folder holds none.
 */
export const V8_LIBRARIES = Object.freeze([
  { name: 'V8', path: 'v8' },
  { name: 'ICU', path: 'third_party/icu' },
  { name: 'Abseil', path: 'third_party/abseil-cpp' },
  { name: 'Highway', path: 'third_party/highway/src' },
  { name: 'simdutf', path: 'third_party/simdutf' },
  { name: 'FP16', path: 'third_party/fp16/src' },
  { name: 'fast_float', path: 'third_party/fast_float/src' },
  { name: 'Dragonbox', path: 'third_party/dragonbox/src' },
  {
    name: 'PartitionAlloc',
    path: 'third_party/partition_alloc',
    // PartitionAlloc's sources say they are under the BSD-style license in
    // Chromium's LICENSE file, which its own repository does not hold.
    fallback: { url: 'https://chromium.googlesource.com/chromium/src', ref: 'refs/heads/main', file: 'LICENSE' }
  }
])

const RUSTY_V8 = 'https://github.com/denoland/rusty_v8'

/** The versions of crate `name` a Cargo.lock lists. */
export function lockedVersions(lockText, name) {
  const versions = []
  for (const block of lockText.split(/^\[\[package\]\]$/m)) {
    const named = /^name = "([^"]+)"$/m.exec(block)
    const version = /^version = "([^"]+)"$/m.exec(block)
    if (named?.[1] === name && version !== null) versions.push(version[1])
  }
  return versions
}

/** `{ path: url }` for each submodule a .gitmodules file declares. */
export function parseGitmodules(text) {
  const urls = {}
  let path = null
  let url = null
  const flush = () => {
    if (path !== null && url !== null) urls[path] = url
    path = null
    url = null
  }
  for (const line of text.split(/\r?\n/)) {
    if (/^\s*\[submodule\s/.test(line)) flush()
    const pathMatch = /^\s*path\s*=\s*(.+?)\s*$/.exec(line)
    const urlMatch = /^\s*url\s*=\s*(.+?)\s*$/.exec(line)
    if (pathMatch !== null) path = pathMatch[1]
    if (urlMatch !== null) url = urlMatch[1]
  }
  flush()
  return urls
}

/** Where a repository's files are read from: `{ kind: 'github', owner, repo }` or `{ kind: 'gitiles', base }`. */
export function repositoryHost(url) {
  const trimmed = url.replace(/\.git$/, '').replace(/\/+$/, '')
  const github = /^https:\/\/github\.com\/([^/]+)\/([^/]+)$/.exec(trimmed)
  if (github !== null) return { kind: 'github', owner: github[1], repo: github[2], url: trimmed }
  if (/^https:\/\/[a-z0-9.-]+\.googlesource\.com\//.test(trimmed)) return { kind: 'gitiles', base: trimmed, url: trimmed }
  throw new Error(`cannot read files from ${url}: only GitHub and googlesource repositories are known`)
}

const USER_AGENT = { 'User-Agent': 'floti-third-party-notices' }

/** A gitiles `?format=JSON` answer, which starts with `)]}'` to keep it from running as a script. */
function gitilesJson(text) {
  return JSON.parse(text.replace(/^\)\]\}'\n?/, ''))
}

async function fetchText(url) {
  const response = await fetch(url, { headers: USER_AGENT })
  if (!response.ok) throw new Error(`GET ${url} answered ${response.status}`)
  return response.text()
}

/** The names of the files at the root of `host`'s tree at `ref`. */
async function rootFiles(host, ref, get) {
  if (host.kind === 'github') {
    const listing = JSON.parse(await get(`https://api.github.com/repos/${host.owner}/${host.repo}/contents/?ref=${encodeURIComponent(ref)}`))
    return listing.filter((entry) => entry.type === 'file').map((entry) => entry.name)
  }
  const listing = gitilesJson(await get(`${host.base}/+/${ref}/?format=JSON`))
  return listing.entries.filter((entry) => entry.type === 'blob').map((entry) => entry.name)
}

/** One file of `host`'s tree at `ref`, as text. */
async function fileAt(host, ref, name, get) {
  if (host.kind === 'github') return get(`https://raw.githubusercontent.com/${host.owner}/${host.repo}/${ref}/${name}`)
  return Buffer.from(await get(`${host.base}/+/${ref}/${name}?format=TEXT`), 'base64').toString('utf8')
}

/** The commit a gitiles branch or tag `ref` names now. */
async function gitilesCommit(host, ref, get) {
  const commit = gitilesJson(await get(`${host.base}/+/${ref}?format=JSON`)).commit
  if (typeof commit !== 'string') throw new Error(`${host.base} names no commit for ${ref}`)
  return commit
}

/** The license files at the root of `host`'s tree at `ref`, as `{ name, text }` sorted by name. */
async function licenseFilesAt(host, ref, get) {
  const names = (await rootFiles(host, ref, get)).filter(isLicenseFileName).sort(compare)
  const files = []
  for (const name of names) files.push({ name, text: normalizeText(await fileAt(host, ref, name, get)) })
  return files.filter((file) => file.text !== '')
}

/**
 * The vendored notices for the v8 crate at `version`: rusty_v8's license
 * files at its tag, then each of V8_LIBRARIES' at the commit that tag pins,
 * each headed with the library, its repository and the commit. `get` fetches
 * a URL's body as text.
 */
export async function vendorV8(version, { get = fetchText } = {}) {
  const tag = `v${version}`
  const rusty = repositoryHost(RUSTY_V8)
  const modules = parseGitmodules(await fileAt(rusty, tag, '.gitmodules', get))
  const sections = []
  // Dashed rules, so the sections stay inside the one notice floti's file gives the v8 crate.
  const add = (library, file, url, ref) => sections.push([THIN_RULE, `${library}: ${file.name}`, `${url} at ${ref}`, THIN_RULE, '', file.text, ''].join('\n'))

  const own = await licenseFilesAt(rusty, tag, get)
  if (own.length === 0) throw new Error(`rusty_v8 ${tag} has no license file at its root`)
  for (const file of own) add('rusty_v8', file, rusty.url, tag)

  for (const library of V8_LIBRARIES) {
    const url = modules[library.path]
    if (url === undefined) throw new Error(`rusty_v8 ${tag} declares no submodule at ${library.path} (${library.name})`)
    const entry = JSON.parse(await get(`https://api.github.com/repos/denoland/rusty_v8/contents/${library.path}?ref=${tag}`))
    if (Array.isArray(entry) || typeof entry.sha !== 'string') throw new Error(`${library.path} is not a submodule at rusty_v8 ${tag}`)
    const host = repositoryHost(url)
    const files = await licenseFilesAt(host, entry.sha, get)
    if (files.length > 0) {
      for (const file of files) add(library.name, file, host.url, entry.sha)
    } else if (library.fallback !== undefined) {
      // A branch, read at the commit it names now, which the header records.
      const fallback = repositoryHost(library.fallback.url)
      const commit = await gitilesCommit(fallback, library.fallback.ref, get)
      const text = normalizeText(await fileAt(fallback, commit, library.fallback.file, get))
      add(library.name, { name: `${library.fallback.file} of ${fallback.url}, which its sources name`, text }, fallback.url, commit)
    } else {
      throw new Error(`${library.name} (${host.url} at ${entry.sha}) has no license file at its root`)
    }
  }

  const intro = [
    `librusty_v8 ${version}`,
    '',
    `The v8 crate ${version} links librusty_v8, a static library holding V8 and the`,
    'libraries below, into floti. Its package leaves out every license file, so',
    `these are rusty_v8's and each library's, at the commits rusty_v8 ${tag} pins.`,
    "libc++, libc++abi, libunwind and LLVM's libc are linked too. They are",
    'Apache-2.0 WITH LLVM-exception, whose exception waives the notice conditions',
    'for their compiled object code.',
    ''
  ].join('\n')
  return `${intro}\n${sections.join('\n')}`.replace(/\n+$/, '\n')
}

// --- command line ------------------------------------------------------------

function writeText(file, text) {
  mkdirSync(dirname(resolve(file)), { recursive: true })
  writeFileSync(file, text)
}

async function main(args) {
  const [command, ...rest] = args
  if (command === 'floti' && rest.length === 1) {
    const text = flotiNotices()
    writeText(rest[0], text)
    console.log(`Wrote floti's third-party notices to ${rest[0]} (${String(Buffer.byteLength(text))} bytes)`)
  } else if (command === 'desktop' && (rest.length === 1 || (rest.length === 3 && rest[1] === '--floti'))) {
    const flotiText = rest.length === 3 ? readFileSync(rest[2], 'utf8') : flotiNotices()
    const text = desktopNotices({ flotiText })
    writeText(rest[0], text)
    console.log(`Wrote the desktop app's third-party notices to ${rest[0]} (${String(Buffer.byteLength(text))} bytes)`)
  } else if (command === 'check' && rest.length === 2) {
    if (!noticesHold(readFileSync(rest[0], 'utf8'), readFileSync(rest[1], 'utf8'))) {
      throw new Error(`${rest[0]} does not hold floti's notices, ${rest[1]}`)
    }
    console.log(`${rest[0]} holds floti's notices`)
  } else if (command === 'vendor-v8' && rest.length === 0) {
    const versions = lockedVersions(readFileSync(join(REPO_ROOT, 'Cargo.lock'), 'utf8'), 'v8')
    if (versions.length !== 1) throw new Error(`Cargo.lock lists ${versions.length === 0 ? 'no v8 crate' : `v8 ${versions.join(' and ')}`}; vendor-v8 needs exactly one`)
    const file = join(VENDORED_DIR, v8VendoredName(versions[0]))
    writeText(file, await vendorV8(versions[0]))
    console.log(`Wrote ${relative(REPO_ROOT, file)}`)
  } else {
    console.error(
      'usage: node third-party-notices.mjs floti <out-file> | desktop <out-file> [--floti <floti-notices>] | check <app-notices> <floti-notices> | vendor-v8'
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
