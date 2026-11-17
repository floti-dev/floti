/**
 * What Settings and the version banner need to know about the CLI. Structurally identical to
 * the main process's `ClaudeStatus` and declared here rather than imported,
 * because `@helm/ui` may not depend on the Electron package - the shape travels
 * as props like every other piece of data this package renders.
 */
export interface SetupClaudeStatus {
  path: string | null
  source: 'setting' | 'discovered' | null
  version: string | null
  semver: string | null
  tested: boolean
  testedRange: { min: string; max: string }
  configDir: string
  configDirExists: boolean
  auth: 'authenticated' | 'unauthenticated' | 'unknown'
  authSignal: string
  error: string | null
}
