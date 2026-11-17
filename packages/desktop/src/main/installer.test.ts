import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { flotiDataDirFor } from './dataFolder'
import { clientFolder } from './host/launcher'

/**
 * The Windows installer's include (`build/installer.nsh`) against the
 * electron-builder config it is built with: the app ID, the folder it installs
 * in, and the host it asks to stop. Read as text; nothing is built.
 */

const HERE = import.meta.dirname
const BUILD = join(HERE, '..', '..', 'build')
const nsh = readFileSync(join(BUILD, 'installer.nsh'), 'utf8')
const config = readFileSync(join(HERE, '..', '..', 'electron-builder.yml'), 'utf8')
/** The include without its comments. */
const code = nsh
  .split('\n')
  .filter((line) => !line.trim().startsWith(';'))
  .join('\n')

describe('the Windows installer', () => {
  it('is built for the app ID dev.floti.desktop', () => {
    expect(config).toMatch(/^appId: dev\.floti\.desktop$/m)
    expect(config).toMatch(/^productName: Floti$/m)
  })

  it("asks a host running from inside the install to stop, with the installed desktop's own client folder", () => {
    // The installer's define, with $LOCALAPPDATA standing for the folder it names.
    const folder = /!define FLOTI_CLIENT_FOLDER "([^"]+)"/.exec(nsh)?.[1]
    const local = 'C:\\Users\\Some One\\AppData\\Local'
    const dataDir = flotiDataDirFor('win32', { LOCALAPPDATA: local }, 'C:\\Users\\Some One')
    expect(folder?.replace('$LOCALAPPDATA', local)).toBe(clientFolder({ dataDir } as Parameters<typeof clientFolder>[0], 'win32'))
    expect(code).toContain('!insertmacro FLOTI_STOP_INSTALLED_HOST')
    expect(code).toContain('FileOpen $3 "$INSTDIR\\resources\\floti.exe" a')
    expect(code).toContain('update-stop "${FLOTI_CLIENT_FOLDER}" --ask')
    expect(code).toContain('update-stop "${FLOTI_CLIENT_FOLDER}" --stop')
    // The one install it acts on is this one: no other folder, no other uninstaller.
    expect(code).not.toMatch(/ExecWait|DeleteRegKey|Uninstall [^"]*\.exe/)
  })

  it('finds a running app by its path alone, never by an image name the host shares but for case', () => {
    expect(code).toContain('!insertmacro FLOTI_CHECK_APP_RUNNING')
    expect(code).not.toMatch(/!insertmacro _CHECK_APP_RUNNING\b/)
    expect(code).not.toMatch(/\b(taskkill|tasklist)\b|IMAGENAME|\/IM /)
    expect(code).toContain("$$_.Path.StartsWith('$INSTDIR\\', 'CurrentCultureIgnoreCase')")
  })

  it("installs beside floti's own folder, as the host looks for it", () => {
    expect(nsh).toContain('!define FLOTI_INSTALL_FOLDER "Floti\\desktop"')
    expect(nsh).toContain('StrCpy $INSTDIR "$LOCALAPPDATA\\Programs\\${FLOTI_INSTALL_FOLDER}"')
    const host = readFileSync(join(HERE, '..', '..', '..', '..', 'crates', 'host', 'src', 'platform', 'installs', 'windows.rs'), 'utf8')
    expect(host).toContain('pub const INSTALL_FOLDER: [&str; 2] = ["Floti", "desktop"];')
  })
})
