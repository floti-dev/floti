// Types for third-party-notices.mjs, so the notices' closures, fallbacks and text can be tested from TypeScript.

/** A package whose notices a download carries: a Rust crate or an npm package. */
export interface NoticePackage {
  name: string
  version: string
  /** Its license, as its manifest names it: an SPDX expression, or Cargo's older `A/B`. */
  license: string | null
  repository: string | null
  authors: string[]
  /** The folder its files are in. */
  dir: string
  /** Files its manifest names as its license (Cargo's `license-file`), relative to `dir`. */
  named: string[]
}

export interface LicenseFile {
  /** Its path inside the package, with forward slashes. */
  name: string
  text: string
}

export interface NoticeText {
  text: string
  /** Where a text that is not the package's own came from, or null for its own. */
  how: string | null
}

export interface ResolvedNotice {
  pkg: NoticePackage
  texts: NoticeText[]
}

export interface MissingNotice {
  pkg: NoticePackage
  why: string
}

/** One submodule of rusty_v8 whose license files the vendored v8 notices hold. */
export interface V8Library {
  name: string
  /** Its path in rusty_v8's repository. */
  path: string
  /** The file to carry when its own folder holds none. */
  fallback?: { url: string; ref: string; file: string }
}

/** Where a repository's files are read from. */
export type RepositoryHost =
  | { kind: 'github'; owner: string; repo: string; url: string }
  | { kind: 'gitiles'; base: string; url: string }

/** Cargo's `cargo metadata --format-version 1` output, as far as the notices read it. */
export interface CargoMetadata {
  packages: {
    id: string
    name: string
    version: string
    source: string | null
    license?: string | null
    license_file?: string | null
    repository?: string | null
    authors?: string[]
    manifest_path: string
  }[]
  resolve: {
    nodes: {
      id: string
      deps: { name: string; pkg: string; dep_kinds: { kind: string | null; target?: string | null }[] }[]
    }[]
  }
}

export const VENDORED_DIR: string
export const APP_NOTICES_NAME: string
export const LICENSE_FILE: RegExp
export const FLOTI_TITLE: string
export const DESKTOP_TITLE: string
export const VENDORED: Readonly<Record<string, { file: (version: string) => string; how: string }>>
export const V8_LIBRARIES: readonly V8Library[]

export function isLicenseFileName(name: string): boolean
export function normalizeText(text: string): string
export function licenseFilesIn(dir: string, named?: string[], options?: { nested?: boolean }): LicenseFile[]
export function normalizeRepository(url: string | null | undefined): string | null
export function licenseChoices(expression: string | null | undefined): string[] | null
export function mitText(holders: string): string
export function copyrightHolders(pkg: Pick<NoticePackage, 'name' | 'authors'>): string
export function v8VendoredName(version: string): string
export function crateClosure(metadata: CargoMetadata, root?: string): NoticePackage[]
export function cargoMetadata(repoRoot?: string): CargoMetadata
export function npmClosure(desktopDir?: string): NoticePackage[]
export function repositoryApacheText(repoRoot?: string): string
export function resolveNotices(
  packages: NoticePackage[],
  options: { vendoredDir?: string; apacheText: string }
): { resolved: ResolvedNotice[]; missing: MissingNotice[] }
export function missingError(missing: MissingNotice[]): Error
export function renderNotices(options: { title: string; intro: string; resolved: ResolvedNotice[] }): string
export function flotiNotices(options?: { metadata?: CargoMetadata; vendoredDir?: string; apacheText?: string }): string
export function desktopNotices(options: { desktopDir?: string; flotiText: string; vendoredDir?: string; apacheText?: string }): string
export function noticesHold(appText: string, flotiText: string): boolean
export function lockedVersions(lockText: string, name: string): string[]
export function parseGitmodules(text: string): Record<string, string>
export function repositoryHost(url: string): RepositoryHost
export function vendorV8(version: string, options?: { get?: (url: string) => Promise<string> }): Promise<string>
