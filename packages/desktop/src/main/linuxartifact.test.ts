import { chmodSync, mkdirSync, writeFileSync } from 'node:fs'
import { mkdtemp } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import { describe, expect, inject, it } from 'vitest'
import { RUN_ROOT } from '../../../../test/run-root'
import { APPIMAGE_NAME, GLIBC_FLOOR, appProblems, glibcNeeded } from '../../scripts/verify-linux-artifact.mjs'

/**
 * How the Linux AppImage check reads an extracted AppImage
 * (scripts/verify-linux-artifact.mjs). Extracting one needs Linux and the
 * release workflow does that; what is checked inside is plain files.
 */

/** A 64-bit little-endian ELF header for `machine` (0x3e is x64, 0xb7 arm64), with no program headers. */
function elf(machine: number): Uint8Array {
  const bytes = new Uint8Array(64)
  const view = new DataView(bytes.buffer)
  bytes.set([0x7f, 0x45, 0x4c, 0x46, 2, 1, 1])
  view.setUint16(18, machine, true)
  return bytes
}

/**
 * An x64 ELF file whose version-needs section (`.gnu.version_r`) asks
 * libc.so.6 for `versions`, as a dynamically linked module's does.
 */
function elfNeeding(versions: string[]): Uint8Array {
  const bytes = new Uint8Array(0x600)
  bytes.set(elf(0x3e))
  const view = new DataView(bytes.buffer)
  const encoder = new TextEncoder()
  // The string table: an empty name, the library, then each version.
  const strings = 0x100
  const names = ['libc.so.6', ...versions]
  const offsets: number[] = []
  let at = 1
  for (const name of names) {
    offsets.push(at)
    bytes.set(encoder.encode(name), strings + at)
    at += name.length + 1
  }
  // One Verneed entry for libc.so.6, then a Vernaux per version.
  const needs = 0x200
  view.setUint16(needs, 1, true)
  view.setUint16(needs + 2, versions.length, true)
  view.setUint32(needs + 4, offsets[0]!, true)
  view.setUint32(needs + 8, 16, true)
  versions.forEach((_, i) => {
    const aux = needs + 16 + i * 16
    view.setUint32(aux + 8, offsets[i + 1]!, true)
    view.setUint32(aux + 12, i === versions.length - 1 ? 0 : 16, true)
  })
  // Section headers: the null one, the string table, the version needs.
  const shoff = 0x400
  view.setBigUint64(0x28, BigInt(shoff), true)
  view.setUint16(0x3a, 64, true)
  view.setUint16(0x3c, 3, true)
  const header = (index: number, type: number, offset: number, size: number, link: number, info: number): void => {
    const h = shoff + index * 64
    view.setUint32(h + 4, type, true)
    view.setBigUint64(h + 24, BigInt(offset), true)
    view.setBigUint64(h + 32, BigInt(size), true)
    view.setUint32(h + 40, link, true)
    view.setUint32(h + 44, info, true)
  }
  header(1, 3, strings, at, 0, 0)
  header(2, 0x6ffffffe, needs, 16 + versions.length * 16, 1, 1)
  return bytes
}

const X64 = elf(0x3e)
const FLOTI = new Uint8Array([...X64, 1, 2, 3])
const SUMS = new TextEncoder().encode(`${'a'.repeat(64)}  floti-3.0.0-linux-x64\n`)

/** A `squashfs-root` as electron-builder lays a good AppImage out, with `change` applied over it. */
async function extracted(change: Record<string, Uint8Array | null> = {}): Promise<string> {
  const root = await mkdtemp(join(inject(RUN_ROOT), 'appimage root '))
  const files: Record<string, Uint8Array | null> = {
    helm: X64,
    'helm.desktop': new TextEncoder().encode('[Desktop Entry]\nName=Helm\nExec=AppRun %U\nType=Application\n'),
    'resources/app.asar': new Uint8Array([0]),
    'resources/icon.png': new Uint8Array([0]),
    'resources/install/install.sh': new Uint8Array([0]),
    'resources/app.asar.unpacked/node_modules/better-sqlite3/prebuilds/linux-x64.node': X64,
    'resources/app.asar.unpacked/node_modules/node-pty/build/Release/pty.node': X64,
    'resources/app.asar.unpacked/node_modules/@koromix/koffi-linux-x64/package.json': new Uint8Array([0]),
    'resources/floti': FLOTI,
    'resources/floti-SHA256SUMS': SUMS,
    'resources/node/bin/node': X64,
    'resources/node/LICENSE': new Uint8Array([0]),
    ...change
  }
  for (const [path, bytes] of Object.entries(files)) {
    if (bytes === null) continue
    const file = join(root, path)
    mkdirSync(dirname(file), { recursive: true })
    writeFileSync(file, bytes)
    chmodSync(file, 0o755)
  }
  return root
}

const check = (root: string, modes = false) => appProblems(root, { floti: FLOTI, sums: SUMS, origin: 'Helm.AppImage', modes })

describe('the Linux AppImage check', () => {
  it('passes an AppImage laid out as the release builds it', async () => {
    expect(check(await extracted())).toEqual([])
  })

  it('names the AppImage file as the release does', () => {
    expect(APPIMAGE_NAME.test('Helm-3.0.0-linux-x64.AppImage')).toBe(true)
    expect(APPIMAGE_NAME.test('Helm-3.0.0-preview.1-linux-x64.AppImage')).toBe(true)
    expect(APPIMAGE_NAME.test('Helm-3.0.0-x86_64.AppImage')).toBe(false)
    expect(APPIMAGE_NAME.test('Helm-3.0.0-linux-arm64.AppImage')).toBe(false)
  })

  it('refuses a floti or a SHA-256 list that is not the one it was given', async () => {
    const root = await extracted({ 'resources/floti': X64, 'resources/floti-SHA256SUMS': new Uint8Array([0]) })
    expect(check(root)).toEqual([
      'Helm.AppImage: resources/floti is not the floti it was given, byte for byte, so it is not the one the release lists',
      'Helm.AppImage: resources/floti-SHA256SUMS is not the SHA-256 list it was given'
    ])
  })

  it('refuses native modules left inside the asar, or built for another CPU', async () => {
    const root = await extracted({
      'resources/app.asar.unpacked/node_modules/node-pty/build/Release/pty.node': null,
      'resources/app.asar.unpacked/node_modules/@koromix/koffi-linux-x64/package.json': null,
      'resources/app.asar.unpacked/node_modules/better-sqlite3/prebuilds/linux-x64.node': elf(0xb7)
    })
    expect(check(root)).toEqual([
      'Helm.AppImage: app.asar.unpacked/node_modules/better-sqlite3/prebuilds/linux-x64.node is built for linux arm64, not linux x64',
      'Helm.AppImage: app.asar.unpacked/node_modules/node-pty/build/Release/pty.node is missing',
      'Helm.AppImage: app.asar.unpacked/node_modules/@koromix/koffi-linux-x64 is missing'
    ])
  })

  it('refuses an app missing its executable, floti, Node or the files beside them', async () => {
    const root = await extracted({
      helm: null,
      'resources/floti': null,
      'resources/floti-SHA256SUMS': null,
      'resources/node/bin/node': new Uint8Array([1, 2, 3]),
      'resources/install/install.sh': null
    })
    expect(check(root)).toEqual([
      "Helm.AppImage: helm, the app's executable, is missing",
      'Helm.AppImage: resources/install/install.sh is missing',
      'Helm.AppImage: resources/floti is missing, so sessions cannot run in the local host',
      "Helm.AppImage: resources/floti-SHA256SUMS is missing, so Settings > Hosts can check no host's floti",
      'Helm.AppImage: resources/node/bin/node is built for no OS this reads, not linux x64'
    ])
  })

  it('reads the newest glibc an ELF file needs from its version needs', () => {
    expect(GLIBC_FLOOR).toBe('2.34')
    expect(glibcNeeded(elfNeeding(['GLIBC_2.2.5', 'GLIBC_2.34', 'GLIBC_2.17', 'GLIBC_PRIVATE']))).toBe('2.34')
    expect(glibcNeeded(elfNeeding(['GLIBC_2.9', 'GLIBC_2.10']))).toBe('2.10')
    expect(glibcNeeded(elfNeeding(['GLIBC_2.42']))).toBe('2.42')
    // A static file needs none, and a file that is not ELF says nothing.
    expect(glibcNeeded(X64)).toBeNull()
    expect(glibcNeeded(new Uint8Array([1, 2, 3]))).toBeNull()
  })

  it('refuses any ELF file in it that needs a glibc newer than the floor', async () => {
    const root = await extracted({
      'resources/app.asar.unpacked/node_modules/node-pty/build/Release/pty.node': elfNeeding(['GLIBC_2.14', 'GLIBC_2.42']),
      'resources/app.asar.unpacked/node_modules/better-sqlite3/prebuilds/linux-x64.node': elfNeeding(['GLIBC_2.34']),
      'libffmpeg.so': elfNeeding(['GLIBC_2.35'])
    })
    expect(check(root)).toEqual([
      'Helm.AppImage: libffmpeg.so needs glibc 2.35, newer than 2.34, so it does not load on an older Linux',
      'Helm.AppImage: resources/app.asar.unpacked/node_modules/node-pty/build/Release/pty.node needs glibc 2.42, newer than 2.34, so it does not load on an older Linux'
    ])
  })

  it('refuses a menu entry that starts Helm without the sandbox, or none at all', async () => {
    const unsandboxed = await extracted({ 'helm.desktop': new TextEncoder().encode('[Desktop Entry]\nExec=AppRun --no-sandbox %U\n') })
    expect(check(unsandboxed)).toEqual(['Helm.AppImage: helm.desktop starts Helm with --no-sandbox (Exec=AppRun --no-sandbox %U)'])
    expect(check(await extracted({ 'helm.desktop': null }))).toEqual(['Helm.AppImage: helm.desktop, the menu entry, is missing'])
  })

  it.skipIf(process.platform === 'win32')('refuses a floti, Node or executable without its execute bit', async () => {
    const root = await extracted()
    for (const path of ['helm', 'resources/floti', 'resources/node/bin/node']) chmodSync(join(root, path), 0o644)
    expect(check(root, true)).toEqual([
      'Helm.AppImage: helm is not executable',
      'Helm.AppImage: resources/floti is not executable',
      'Helm.AppImage: resources/node/bin/node is not executable'
    ])
  })
})
