import { readFileSync } from 'node:fs'
import { posix } from 'node:path'
import type { AppSettings } from '@helm/core'
import type { BrowserAvailability } from '../shared/ipc'

/**
 * Whether the browser pane may run in this Helm.
 *
 * Every renderer runs inside Chromium's OS sandbox, and the browser pane is
 * the one that loads arbitrary sites. Started without the sandbox, a renderer
 * exploit would be code running as the user, holding the host's socket and so
 * every session. On Linux an AppImage's AppRun adds `--no-sandbox` by itself
 * whenever unprivileged user namespaces are refused (Ubuntu 24.04's AppArmor
 * restriction does that), so Helm never degrades silently: with the switch
 * present, no web page is opened and no browser tool is served, and the
 * Browser tab says how to give the sandbox back. Everything else keeps
 * working.
 *
 * Decided once, at start: the switch cannot change while Helm runs.
 */

/** The file the profile is written to, and the profile's name. */
export const APPARMOR_PROFILE = 'helm-appimage'
export const APPARMOR_PROFILE_FILE = `/etc/apparmor.d/${APPARMOR_PROFILE}`

/** Where the kernel says whether AppArmor restricts unprivileged user namespaces. */
export const APPARMOR_USERNS_SYSCTL = '/proc/sys/kernel/apparmor_restrict_unprivileged_userns'

/** What the decision is made from. */
export interface SandboxFacts {
  platform: NodeJS.Platform
  /** Whether Electron was started with `--no-sandbox`. */
  noSandbox: boolean
  env: NodeJS.ProcessEnv
  /** Whether AppArmor restricts unprivileged user namespaces here; null when it could not be read. */
  appArmorRestricts: boolean | null
}

/** The decision: available, or off with the command that fixes it where Helm can write one. */
export function browserAvailability(facts: SandboxFacts): BrowserAvailability {
  if (facts.platform !== 'linux' || !facts.noSandbox) return { available: true }
  const appImage = facts.env['APPIMAGE']
  const command = facts.appArmorRestricts === true && appImage !== undefined ? appArmorCommand(appImage) : null
  return { available: false, command }
}

/** The settings as the browser's own code must read them: with the pane off, the agent tools are off too. */
export function withBrowserAvailability(settings: AppSettings, availability: BrowserAvailability): AppSettings {
  return availability.available || !settings.browserMcp ? settings : { ...settings, browserMcp: false }
}

/**
 * The AppArmor profile that lets AppImages at `pattern` create user
 * namespaces, which is what Chromium's sandbox needs. It confines nothing
 * else (`unconfined`), the way Ubuntu's own profiles for browsers do, and
 * whatever the AppImage starts inherits it, AppRun's probe included.
 */
export function appArmorProfile(pattern: string): string[] {
  return [
    'abi <abi/4.0>,',
    'include <tunables/global>',
    `profile ${APPARMOR_PROFILE} "${pattern}" flags=(unconfined) {`,
    '  userns,',
    '}'
  ]
}

/** A string as one shell word, in single quotes. */
function shellWord(text: string): string {
  return `'${text.replaceAll("'", "'\\''")}'`
}

// Characters that are AppArmor glob or quoting syntax, or that a line holds badly.
const UNSAFE_IN_PROFILE = /["*?[\]{}^\\\n\r]/

/**
 * The one command that installs the profile for this AppImage and loads it:
 * a single `sudo`. It covers every Helm AppImage in the same folder
 * (`Helm-*.AppImage`), so the next version keeps the sandbox; a renamed file
 * is covered by its own name. Null when the path cannot be written into a
 * profile safely.
 */
export function appArmorCommand(appImage: string): string | null {
  if (!posix.isAbsolute(appImage)) return null
  const folder = posix.dirname(appImage)
  const name = posix.basename(appImage)
  if (UNSAFE_IN_PROFILE.test(folder) || UNSAFE_IN_PROFILE.test(name)) return null
  const pattern = /^Helm-.+\.AppImage$/.test(name) ? posix.join(folder, 'Helm-*.AppImage') : appImage
  const lines = appArmorProfile(pattern).map(shellWord).join(' ')
  const install = `cat > ${APPARMOR_PROFILE_FILE} && apparmor_parser -r ${APPARMOR_PROFILE_FILE}`
  return `printf '%s\\n' ${lines} | sudo sh -c ${shellWord(install)}`
}

/** Whether AppArmor restricts unprivileged user namespaces, read from the kernel; null when it cannot be read. */
export function readAppArmorRestricts(read: (file: string) => string = (file) => readFileSync(file, 'utf8')): boolean | null {
  try {
    return read(APPARMOR_USERNS_SYSCTL).trim() === '1'
  } catch {
    return null
  }
}
