// Types for package-managers.mjs, so the Homebrew formula and the winget manifests can be tested from TypeScript.

export interface PackagedFile {
  /** The release's file name. */
  name: string
  url: string
  /** Lower-case hex. */
  sha256: string
}

export interface ReleaseInput {
  /** The text of the release's floti-<version>-SHA256SUMS. */
  sums: string
  version: string
  /** `<owner>/<repository>`. */
  repo: string
}

export const HOMEBREW_TARGETS: readonly string[]
export const WINGET_TARGETS: readonly string[]
export const WINGET_IDENTIFIER: string
export const WINGET_MANIFEST_VERSION: string
export const DESCRIPTION: string
export function downloadUrl(repo: string, version: string, name: string): string
export function releaseFiles(sumsText: string, version: string, repo: string, targets: readonly string[]): Map<string, PackagedFile>
export function homebrewFormula(input: ReleaseInput): string
export function wingetFolder(version: string): string
export function wingetManifests(input: ReleaseInput): { path: string; text: string }[]
