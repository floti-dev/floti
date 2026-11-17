// Types for after-extract.mjs, so electron-builder's afterExtract hook can be tested from TypeScript.

/** The part of electron-builder's AfterExtractContext the hook reads. */
export interface AfterExtractContext {
  appOutDir: string
  electronPlatformName: string
}

export const ELECTRON_LICENSES: readonly { from: string; to: string }[]
export function bundleResources(appOutDir: string): string
export default function afterExtract(context: AfterExtractContext): void
