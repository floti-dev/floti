// Asserts that the Linux AppImage is what a person on Linux needs it to be -
// by extracting the file they download, not by looking at electron-builder's
// `linux-unpacked/` scratch directory.
//
// For the app inside it:
//
// - its executable (`helm`) is an x64 Linux program, the only architecture the
//   release builds;
// - its native modules are unpacked beside the asar: better-sqlite3's prebuild,
//   node-pty's addon (compiled by `pnpm install`, since node-pty ships no Linux
//   prebuild) and koffi's Linux package. A module inside the archive cannot be
//   loaded;
// - its `floti` is the one it was given, byte for byte, and executable: in a
//   release that is the release's file, so the app's floti is the one the
//   release's SHA-256 list gives, and `floti-SHA256SUMS` beside it is that list;
// - the pinned Node the local host runs plugin services on is there and
//   executable, with Node's LICENSE, and so are the install scripts Settings >
//   Hosts sends and the window icon;
// - no ELF file anywhere in it needs a glibc newer than GLIBC_FLOOR. A native
//   module compiled on a newer system than that is one the app cannot load
//   on an older one, and Helm then dies at start; the release builds it in an
//   old enough container, and this is what proves it did;
// - its menu entry (`floti-desktop.desktop`) starts it without `--no-sandbox`, so
//   launching it from a desktop integration keeps Chromium's sandbox.
//
//   node scripts/verify-linux-artifact.mjs <AppImage> [--floti <file>] [--sums <file>]
//
// Without --floti and --sums it compares against what stage-floti.mjs staged
// in dist-host/. Extracting needs Linux: the AppImage's own runtime does it
// (`--appimage-extract`), with no FUSE. Every failure is reported, not just
// the first.

import { spawnSync } from 'node:child_process'
import { chmodSync, closeSync, existsSync, mkdtempSync, openSync, readdirSync, readFileSync, readSync, rmSync, statSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { basename, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { binaryFacts } from './floti-release.mjs'

const desktopDir = resolve(import.meta.dirname, '..')

/** The AppImage's name: `Floti-<version>-linux-x64.AppImage`. */
export const APPIMAGE_NAME = /^Floti-.+-linux-x64\.AppImage$/

/** Native files that must exist under `resources/app.asar.unpacked`, and which of them are x64 Linux code. */
export const UNPACKED = [
  { path: 'node_modules/better-sqlite3/prebuilds/linux-x64.node', elf: true },
  { path: 'node_modules/node-pty/build/Release/pty.node', elf: true },
  { path: 'node_modules/@koromix/koffi-linux-x64', elf: false }
]

/**
 * The newest glibc any file in the AppImage may need: better-sqlite3's Linux
 * prebuild already needs 2.34, so that is the oldest Linux Helm runs on
 * (Ubuntu 22.04, Debian 12, Fedora 35, RHEL 9).
 */
export const GLIBC_FLOOR = '2.34'

const SHT_GNU_VERNEED = 0x6ffffffe

/** `[major, minor, patch]` of a `GLIBC_x.y[.z]` version name, or null for any other name. */
function glibcParts(name) {
  const match = /^GLIBC_(\d+)\.(\d+)(?:\.(\d+))?$/.exec(name)
  return match === null ? null : [Number(match[1]), Number(match[2]), Number(match[3] ?? 0)]
}

function compareParts(a, b) {
  for (let i = 0; i < 3; i++) if (a[i] !== b[i]) return a[i] - b[i]
  return 0
}

/**
 * The newest `GLIBC_x.y` version a 64-bit little-endian ELF file needs, as
 * its version-needs section (`.gnu.version_r`) lists them, or null when it
 * needs none (a static file, or one that is not ELF).
 *
 * @param {Uint8Array} bytes
 */
export function glibcNeeded(bytes) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  const at = (offset, size) => offset >= 0 && offset + size <= bytes.byteLength
  if (!at(0, 64) || bytes[0] !== 0x7f || bytes[1] !== 0x45 || bytes[2] !== 0x4c || bytes[3] !== 0x46) return null
  if (bytes[4] !== 2 || bytes[5] !== 1) return null
  const shoff = Number(view.getBigUint64(0x28, true))
  const shentsize = view.getUint16(0x3a, true)
  const shnum = view.getUint16(0x3c, true)
  const section = (index) => {
    const header = shoff + index * shentsize
    if (!at(header, 64)) return null
    return {
      type: view.getUint32(header + 4, true),
      offset: Number(view.getBigUint64(header + 24, true)),
      size: Number(view.getBigUint64(header + 32, true)),
      link: view.getUint32(header + 40, true),
      info: view.getUint32(header + 44, true)
    }
  }
  const text = (strings, offset) => {
    let end = strings.offset + offset
    while (end < strings.offset + strings.size && at(end, 1) && bytes[end] !== 0) end++
    return new TextDecoder().decode(bytes.subarray(strings.offset + offset, end))
  }
  let newest = null
  for (let i = 0; i < shnum; i++) {
    const needs = section(i)
    if (needs === null || needs.type !== SHT_GNU_VERNEED) continue
    const strings = section(needs.link)
    if (strings === null) continue
    let entry = needs.offset
    for (let n = 0; n < needs.info && at(entry, 16); n++) {
      let aux = entry + view.getUint32(entry + 8, true)
      for (let k = 0; k < view.getUint16(entry + 2, true) && at(aux, 16); k++) {
        const name = text(strings, view.getUint32(aux + 8, true))
        const parts = glibcParts(name)
        if (parts !== null && (newest === null || compareParts(parts, glibcParts(newest)) > 0)) newest = name
        const next = view.getUint32(aux + 12, true)
        if (next === 0) break
        aux += next
      }
      const next = view.getUint32(entry + 12, true)
      if (next === 0) break
      entry += next
    }
  }
  return newest === null ? null : newest.slice('GLIBC_'.length)
}

/** Every regular file under `root` that starts with the ELF magic, relative to `root`. Links are not followed. */
function elfFiles(root) {
  const found = []
  const magic = Buffer.alloc(4)
  const walk = (dir, prefix) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const full = join(dir, entry.name)
      const relative = prefix === '' ? entry.name : `${prefix}/${entry.name}`
      if (entry.isDirectory()) walk(full, relative)
      else if (entry.isFile()) {
        const fd = openSync(full, 'r')
        try {
          if (readSync(fd, magic, 0, 4, 0) === 4 && magic.equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46]))) found.push(relative)
        } finally {
          closeSync(fd)
        }
      }
    }
  }
  walk(root, '')
  return found.sort()
}

/** Files the app carries in `resources` beside the asar. */
export const RESOURCES = ['app.asar', 'icon.png', 'install/install.sh', 'node/LICENSE']

/**
 * Every problem with an extracted AppImage (`squashfs-root`), each prefixed
 * with `origin`. `floti` and `sums` are the bytes its floti and its
 * `floti-SHA256SUMS` must be. Execute bits are checked only where the file
 * system has them (`modes`).
 */
export function appProblems(root, { floti, sums, origin, modes = process.platform !== 'win32' }) {
  const problems = []
  const fail = (message) => problems.push(`${origin}: ${message}`)
  const executable = (file, what) => {
    if (modes && (statSync(file).mode & 0o111) === 0) fail(`${what} is not executable`)
  }
  const x64Linux = (file, what) => {
    const facts = binaryFacts(readFileSync(file))
    if (facts.os !== 'linux' || facts.cpu !== 'x64') {
      fail(`${what} is built for ${facts.os === null ? 'no OS this reads' : `${facts.os} ${String(facts.cpu)}`}, not linux x64`)
    }
  }

  const program = join(root, 'floti-desktop')
  if (!existsSync(program)) fail("helm, the app's executable, is missing")
  else {
    x64Linux(program, 'floti-desktop')
    executable(program, 'floti-desktop')
  }

  const resources = join(root, 'resources')
  for (const path of RESOURCES) {
    if (!existsSync(join(resources, path))) fail(`resources/${path} is missing`)
  }
  for (const { path, elf } of UNPACKED) {
    const full = join(resources, 'app.asar.unpacked', path)
    if (!existsSync(full)) fail(`app.asar.unpacked/${path} is missing`)
    else if (elf) x64Linux(full, `app.asar.unpacked/${path}`)
  }

  // Where `flotiBinary` looks for the local host in a packaged build.
  const carried = join(resources, 'floti')
  if (!existsSync(carried)) fail('resources/floti is missing, so sessions cannot run in the local host')
  else {
    executable(carried, 'resources/floti')
    if (!readFileSync(carried).equals(floti)) fail('resources/floti is not the floti it was given, byte for byte, so it is not the one the release lists')
  }
  const carriedSums = join(resources, 'floti-SHA256SUMS')
  if (!existsSync(carriedSums)) fail("resources/floti-SHA256SUMS is missing, so Settings > Hosts can check no host's floti")
  else if (!readFileSync(carriedSums).equals(sums)) fail('resources/floti-SHA256SUMS is not the SHA-256 list it was given')

  const floor = glibcParts(`GLIBC_${GLIBC_FLOOR}`)
  for (const { path, needed } of glibcByFile(root)) {
    if (compareParts(glibcParts(`GLIBC_${needed}`), floor) > 0) {
      fail(`${path} needs glibc ${needed}, newer than ${GLIBC_FLOOR}, so it does not load on an older Linux`)
    }
  }

  const entry = join(root, 'floti-desktop.desktop')
  if (!existsSync(entry)) fail('floti-desktop.desktop, the menu entry, is missing')
  else {
    const exec = /^Exec=(.*)$/m.exec(readFileSync(entry, 'utf8'))
    if (exec === null) fail('floti-desktop.desktop has no Exec line')
    else if (/(^|\s)--no-sandbox(\s|$)/.test(exec[1])) fail(`floti-desktop.desktop starts Floti with --no-sandbox (Exec=${exec[1]})`)
  }

  const node = join(resources, 'node', 'bin', 'node')
  if (!existsSync(node)) fail('resources/node/bin/node is missing, so no plugin service can run in the local host')
  else {
    x64Linux(node, 'resources/node/bin/node')
    executable(node, 'resources/node/bin/node')
  }
  return problems
}

/** Each ELF file under `root` that needs some glibc, with the newest it needs, newest first. */
export function glibcByFile(root) {
  return elfFiles(root)
    .flatMap((path) => {
      const needed = glibcNeeded(readFileSync(join(root, path)))
      return needed === null ? [] : [{ path, needed }]
    })
    .sort((a, b) => compareParts(glibcParts(`GLIBC_${b.needed}`), glibcParts(`GLIBC_${a.needed}`)) || a.path.localeCompare(b.path))
}

/** Extracts an AppImage with its own runtime into a fresh folder and returns `squashfs-root` in it. */
function extract(appImage, into) {
  chmodSync(appImage, 0o755)
  const run = spawnSync(appImage, ['--appimage-extract'], { cwd: into, encoding: 'utf8', stdio: ['ignore', 'ignore', 'pipe'] })
  if (run.error !== undefined) throw new Error(`could not run ${basename(appImage)} --appimage-extract: ${run.error.message}`)
  if (run.status !== 0) throw new Error(`${basename(appImage)} --appimage-extract failed (exit ${String(run.status)}): ${run.stderr.trim()}`)
  const root = join(into, 'squashfs-root')
  if (!existsSync(root)) throw new Error(`${basename(appImage)} --appimage-extract made no squashfs-root`)
  return root
}

function option(args, name, fallback) {
  const at = args.indexOf(name)
  return at === -1 ? fallback : args[at + 1]
}

function main(args) {
  const appImage = args.find((arg, i) => !arg.startsWith('--') && !['--floti', '--sums'].includes(args[i - 1] ?? ''))
  if (appImage === undefined) {
    console.error('usage: node verify-linux-artifact.mjs <AppImage> [--floti <file>] [--sums <file>]')
    return 2
  }
  if (process.platform !== 'linux') {
    console.error('Extracting an AppImage needs Linux.')
    return 2
  }
  const file = resolve(appImage)
  const floti = option(args, '--floti', join(desktopDir, 'dist-host', 'floti'))
  const sums = option(args, '--sums', join(desktopDir, 'dist-host', 'floti-SHA256SUMS'))
  const problems = []
  let newest = null
  if (!APPIMAGE_NAME.test(basename(file))) problems.push(`${basename(file)} is not named Floti-<version>-linux-x64.AppImage`)
  for (const [what, path] of [['the AppImage', file], ['--floti', floti], ['--sums', sums]]) {
    if (!existsSync(path)) problems.push(`${what} ${path} does not exist`)
  }
  if (problems.length === 0) {
    const facts = binaryFacts(readFileSync(file))
    if (facts.os !== 'linux' || facts.cpu !== 'x64') problems.push(`${basename(file)} is not an x64 Linux executable`)
    const work = mkdtempSync(join(tmpdir(), 'helm-appimage-'))
    try {
      const root = extract(file, work)
      problems.push(...appProblems(root, { floti: readFileSync(floti), sums: readFileSync(sums), origin: basename(file) }))
      // Said on success too, so the log shows what the floor was measured against.
      const needs = glibcByFile(root)
      newest = needs.length === 0 ? null : `${String(needs.length)} ELF files need glibc; the newest, ${needs[0].needed}, by ${needs[0].path}`
    } catch (error) {
      problems.push(error instanceof Error ? error.message : String(error))
    } finally {
      rmSync(work, { recursive: true, force: true })
    }
  }

  if (problems.length > 0) {
    for (const problem of problems) console.error(`FAIL ${problem}`)
    return 1
  }
  console.log(`verified: ${basename(file)}; ${newest ?? 'no ELF file in it names a glibc version'}`)
  return 0
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2))
}
