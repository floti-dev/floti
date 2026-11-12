import { describe, expect, it } from 'vitest'
import { DEFAULT_SETTINGS } from '@helm/core'
import {
  APPARMOR_PROFILE_FILE,
  APPARMOR_USERNS_SYSCTL,
  appArmorCommand,
  appArmorProfile,
  browserAvailability,
  readAppArmorRestricts,
  withBrowserAvailability
} from './browser-sandbox'

/**
 * Whether the browser pane runs (main/browser-sandbox.ts): never in a Linux
 * Helm started without Chromium's sandbox, and the command that gives an
 * AppImage the sandbox back.
 */

const APPIMAGE = '/home/me/Apps/Helm-3.0.0-linux-x64.AppImage'
const LINUX = { platform: 'linux' as const, env: { APPIMAGE }, appArmorRestricts: true }

describe('whether the browser pane runs', () => {
  it('runs wherever Chromium has its sandbox', () => {
    expect(browserAvailability({ ...LINUX, noSandbox: false })).toEqual({ available: true })
    expect(browserAvailability({ ...LINUX, platform: 'win32', noSandbox: false })).toEqual({ available: true })
  })

  it('is off in a Linux Helm started with --no-sandbox, with the command for its AppImage', () => {
    expect(browserAvailability({ ...LINUX, noSandbox: true })).toEqual({ available: false, command: appArmorCommand(APPIMAGE) })
  })

  it('is off with no command where AppArmor is not what refused the sandbox, or Helm is not an AppImage', () => {
    expect(browserAvailability({ ...LINUX, noSandbox: true, appArmorRestricts: false })).toEqual({ available: false, command: null })
    expect(browserAvailability({ ...LINUX, noSandbox: true, appArmorRestricts: null })).toEqual({ available: false, command: null })
    expect(browserAvailability({ ...LINUX, noSandbox: true, env: {} })).toEqual({ available: false, command: null })
  })

  it('turns the browser tools off with the pane, and leaves every other setting as it is', () => {
    const settings = { ...DEFAULT_SETTINGS, browserMcp: true }
    expect(withBrowserAvailability(settings, { available: true })).toBe(settings)
    expect(withBrowserAvailability(settings, { available: false, command: null })).toEqual({ ...settings, browserMcp: false })
  })

  it('reads whether AppArmor restricts user namespaces from the kernel, and says so when it cannot', () => {
    const asked: string[] = []
    expect(readAppArmorRestricts((file) => (asked.push(file), '1\n'))).toBe(true)
    expect(readAppArmorRestricts(() => '0\n')).toBe(false)
    expect(
      readAppArmorRestricts(() => {
        throw new Error('ENOENT')
      })
    ).toBeNull()
    expect(asked).toEqual([APPARMOR_USERNS_SYSCTL])
  })
})

describe('the command that gives an AppImage the sandbox', () => {
  it('writes a profile letting every Helm AppImage in its folder make user namespaces, with one sudo', () => {
    expect(appArmorProfile('/home/me/Apps/Helm-*.AppImage')).toEqual([
      'abi <abi/4.0>,',
      'include <tunables/global>',
      'profile helm-appimage "/home/me/Apps/Helm-*.AppImage" flags=(unconfined) {',
      '  userns,',
      '}'
    ])
    expect(appArmorCommand(APPIMAGE)).toBe(
      `printf '%s\\n' 'abi <abi/4.0>,' 'include <tunables/global>' 'profile helm-appimage "/home/me/Apps/Helm-*.AppImage" flags=(unconfined) {' '  userns,' '}' | sudo sh -c 'cat > ${APPARMOR_PROFILE_FILE} && apparmor_parser -r ${APPARMOR_PROFILE_FILE}'`
    )
    expect(appArmorCommand(APPIMAGE)!.match(/sudo/g)).toHaveLength(1)
  })

  it('names a renamed AppImage by its own path, and quotes a folder with a quote in it', () => {
    expect(appArmorCommand('/opt/apps/helm.AppImage')).toContain(`'profile helm-appimage "/opt/apps/helm.AppImage" flags=(unconfined) {'`)
    expect(appArmorCommand("/home/o'neil/Helm-3.0.0-linux-x64.AppImage")).toContain(
      `'profile helm-appimage "/home/o'\\''neil/Helm-*.AppImage" flags=(unconfined) {'`
    )
  })

  it('writes none for a path a profile cannot hold safely', () => {
    for (const path of ['Helm.AppImage', '/home/me/"x"/Helm.AppImage', '/home/me/[apps]/Helm.AppImage', '/home/me/a*b/Helm.AppImage']) {
      expect(appArmorCommand(path), path).toBeNull()
    }
  })
})
