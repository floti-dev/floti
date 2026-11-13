import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

/**
 * The Windows installer's include (`build/installer.nsh`) against the
 * electron-builder config it is built with: the app ID it moves an old install
 * from, and the folder it installs in. Read as text; nothing is built.
 */

const HERE = import.meta.dirname
const BUILD = join(HERE, '..', '..', 'build')
const nsh = readFileSync(join(BUILD, 'installer.nsh'), 'utf8')
const config = readFileSync(join(HERE, '..', '..', 'electron-builder.yml'), 'utf8')

/** electron-builder's namespace for the GUID it derives from an app ID (`ELECTRON_BUILDER_NS_UUID`). */
const ELECTRON_BUILDER_NS = '50e065bc-3134-11e6-9bab-38c9862bdaf3'

/** A name-based UUID (RFC 4122 version 5) of `name` in `namespace`, as electron-builder computes it. */
function uuidV5(name: string, namespace: string): string {
  const hash = createHash('sha1')
    .update(Buffer.from(namespace.replace(/-/g, ''), 'hex'))
    .update(name, 'utf8')
    .digest()
  hash[6] = (hash[6]! & 0x0f) | 0x50
  hash[8] = (hash[8]! & 0x3f) | 0x80
  const hex = hash.subarray(0, 16).toString('hex')
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
}

describe('the Windows installer', () => {
  it('is built for the app ID dev.floti.desktop', () => {
    expect(config).toMatch(/^appId: dev\.floti\.desktop$/m)
    expect(config).toMatch(/^productName: Floti$/m)
  })

  it("moves an old install from the uninstall entry Helm's app ID gave it", () => {
    expect(uuidV5('dev.floti.desktop', ELECTRON_BUILDER_NS)).toBe('189f9946-3aab-5304-9f04-ec8cb9d04196')
    const guid = /!define HELM_APP_GUID "([0-9a-f-]+)"/.exec(nsh)?.[1]
    expect(guid).toBe(uuidV5('dev.coletaylor.helm', ELECTRON_BUILDER_NS))
    expect(nsh).toContain('Uninstall Helm.exe')
    expect(nsh).toContain('"$PLUGINSDIR\\helm-uninstaller.exe" /S /currentuser _?=$helmInstall')
  })

  it("installs beside floti's own folder, as the host looks for it", () => {
    expect(nsh).toContain('!define FLOTI_INSTALL_FOLDER "Floti\\desktop"')
    expect(nsh).toContain('StrCpy $INSTDIR "$LOCALAPPDATA\\Programs\\${FLOTI_INSTALL_FOLDER}"')
    const host = readFileSync(join(HERE, '..', '..', '..', '..', 'crates', 'host', 'src', 'platform', 'installs', 'windows.rs'), 'utf8')
    expect(host).toContain('pub const INSTALL_FOLDER: [&str; 2] = ["Floti", "desktop"];')
  })
})
