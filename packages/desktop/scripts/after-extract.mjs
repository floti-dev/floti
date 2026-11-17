// electron-builder's afterExtract hook (electron-builder.yml). It runs once
// Electron's download is unpacked into the output folder and before the app is
// made out of it.
//
// For macOS it keeps Electron's license and Chromium's in the app. Electron's
// download holds them beside Electron.app, as LICENSE and
// LICENSES.chromium.html, and a Windows or Linux build keeps them beside the
// app as LICENSE.electron.txt and LICENSES.chromium.html. On macOS
// electron-builder deletes both when it makes the app, which is after this
// hook, so they are copied into the bundle's Resources first, under the names
// the other builds give them, and the app's signature covers them. Every
// other platform it leaves alone.

import { copyFileSync, existsSync, readdirSync } from 'node:fs'
import { join } from 'node:path'

/** Electron's license files, as its download names them, and the names the app carries them under. */
export const ELECTRON_LICENSES = Object.freeze([
  { from: 'LICENSE', to: 'LICENSE.electron.txt' },
  { from: 'LICENSES.chromium.html', to: 'LICENSES.chromium.html' }
])

/** The Resources folder of the one app bundle in `appOutDir`. */
export function bundleResources(appOutDir) {
  const bundles = readdirSync(appOutDir).filter((name) => name.endsWith('.app'))
  if (bundles.length !== 1) {
    throw new Error(`afterExtract: ${appOutDir} holds ${bundles.length === 0 ? 'no app bundle' : `${bundles.length} app bundles`}, not one`)
  }
  return join(appOutDir, bundles[0], 'Contents', 'Resources')
}

export default function afterExtract(context) {
  if (context.electronPlatformName !== 'darwin') return
  const resources = bundleResources(context.appOutDir)
  for (const { from, to } of ELECTRON_LICENSES) {
    const source = join(context.appOutDir, from)
    // A Mac app without them ships Electron and Chromium with no notices: fail the build.
    if (!existsSync(source)) throw new Error(`afterExtract: Electron's download has no ${from} in ${context.appOutDir}`)
    copyFileSync(source, join(resources, to))
    console.log(`  afterExtract: kept Electron's ${from} as ${join('Contents', 'Resources', to)}`)
  }
}
