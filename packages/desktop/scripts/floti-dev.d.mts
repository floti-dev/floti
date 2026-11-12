// Types for floti-dev.mjs, so the install scripts and the update manifest can be tested from TypeScript.

export interface FileEntry {
  /** A plain file name in the release's folder. */
  file: string
  /** Lower-case hex. */
  sha256: string
}

export interface UpdateManifest {
  schema: number
  channels: Record<string, { version: string; url: string; floti: Record<string, FileEntry>; desktop: Record<string, FileEntry> }>
}

export interface InstallScript {
  name: string
  template: string
  install: string
  targets: readonly string[]
}

export const MANIFEST_SCHEMA: number
export const STABLE_CHANNEL: string
export const DESKTOP_FILES: Readonly<Record<string, (version: string) => string>>
export const INSTALL_SCRIPTS: readonly InstallScript[]
export function releaseUrlProblem(url: string): string | null
export function releaseFolder(repo: string, version: string): string
export function installFunction(text: string): string
export function fillTemplate(
  template: string,
  input: { installFunction: string; version: string; releaseUrl: string; sha256: ReadonlyMap<string, string> }
): string
export function installScripts(input: {
  sums: string
  version: string
  repo: string
  releaseUrl?: string
  read?: (path: string) => string
}): { name: string; text: string }[]
export function updateManifest(input: {
  version: string
  url: string
  floti: ReadonlyMap<string, FileEntry>
  desktop: ReadonlyMap<string, FileEntry>
}): UpdateManifest
export function manifestText(manifest: UpdateManifest): string
export function stableVersion(version: string): string
export function scriptsProblems(input: {
  scripts: ReadonlyMap<string, string>
  sums: string
  version: string
  repo: string
  read?: (path: string) => string
}): string[]
export function manifestProblems(input: { text: string; sums: string; version: string; repo: string }): string[]
export function releaseManifest(input: { folder: string; version: string; repo: string; asStable?: boolean }): Promise<UpdateManifest>
