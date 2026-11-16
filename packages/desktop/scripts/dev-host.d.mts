// Types for dev-host.mjs, so the build decision can be tested from TypeScript.

export interface Protocol {
  major: number
  minor: number
}

export interface VersionLine {
  version: string
  protocol: Protocol
}

export interface BuildDecision {
  build: boolean
  reason: string
}

export const BUILD_JOBS: number
export function devHostPath(options: { repoRoot: string; env: Record<string, string | undefined>; platform: string }): string
export function parseVersionLine(text: string): VersionLine | null
export function clientProtocol(generatedText: string): Protocol | null
export function compareVersions(a: string, b: string): number | null
export function decideBuild(options: {
  exists: boolean
  output: string | null
  client: { protocol: Protocol; version: string }
}): BuildDecision
export function hostFiles(path: string): [string, string]
export function asidePath(path: string, stamp: number): string
export function isAside(name: string, base: string): boolean
export function moveAside(options: {
  path: string
  exists: (path: string) => boolean
  rename: (from: string, to: string) => void
  stamp: number
}): string[]
export function sweepAsides(options: { path: string; list: (folder: string) => string[]; remove: (path: string) => void }): number
export function buildEnv(env: Record<string, string | undefined>): Record<string, string | undefined>
export function ensureDevHost(options: {
  repoRoot: string
  env: Record<string, string | undefined>
  platform: string
  read: (path: string) => string
  exists: (path: string) => boolean
  version: (path: string) => string | null
  list: (folder: string) => string[]
  rename: (from: string, to: string) => void
  remove: (path: string) => void
  now: () => number
  build: (env: Record<string, string | undefined>) => number
  say: (line: string) => void
}): { built: boolean; reason: string }
export function ensureDevHostForReal(options?: {
  env?: Record<string, string | undefined>
  platform?: string
}): { built: boolean; reason: string }
