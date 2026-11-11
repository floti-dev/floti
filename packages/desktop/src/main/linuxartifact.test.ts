import { chmodSync, mkdirSync, writeFileSync } from 'node:fs'
import { mkdtemp } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import { describe, expect, inject, it } from 'vitest'
import { RUN_ROOT } from '../../../../test/run-root'
import { APPIMAGE_NAME, appProblems } from '../../scripts/verify-linux-artifact.mjs'

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

const X64 = elf(0x3e)
const FLOTI = new Uint8Array([...X64, 1, 2, 3])
const SUMS = new TextEncoder().encode(`${'a'.repeat(64)}  floti-3.0.0-linux-x64\n`)

/** A `squashfs-root` as electron-builder lays a good AppImage out, with `change` applied over it. */
async function extracted(change: Record<string, Uint8Array | null> = {}): Promise<string> {
  const root = await mkdtemp(join(inject(RUN_ROOT), 'appimage root '))
  const files: Record<string, Uint8Array | null> = {
    helm: X64,
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
