// Types for release.mjs, so the version gate and the release's file list can be tested from TypeScript.

export interface ParsedVersion {
  major: number
  minor: number
  patch: number
  /** The pre-release identifiers; empty for a release. */
  pre: string[]
}

export interface GateDecision {
  version: string
  tag: string
  preview: boolean
  dryRun: boolean
  /** Whether the build runs. */
  release: boolean
  /** The highest version already tagged, or null when none is. */
  highest: string | null
  /** Tags that are not `v<version>`, which the gate ignores. */
  ignored: string[]
  reason: string
}

export function parseVersion(text: string): ParsedVersion | null
export function compareVersions(a: string, b: string): number
export function taggedVersions(lsRemote: string): { versions: string[]; others: string[] }
export function highestVersion(versions: string[]): string | null
export function gate(options: { version: string; lsRemote: string; dryRun: boolean }): GateDecision
export function desktopFileNames(version: string): string[]
export function desktopSumsName(version: string): string
export function writeDesktopSums(folder: string, version: string): Promise<string>
export function protocolFileName(version: string): string
export function signatureName(name: string): string
export function signedFileNames(version: string): string[]
export function releaseAssetNames(version: string, options?: { signed?: boolean }): string[]
export function assetsProblem(names: string[], version: string, options?: { signed?: boolean }): string | null
export function summaryTable(rows: { name: string; size: number; sha256: string }[]): string
