// Types for programs.mjs, for the TypeScript that writes the fakes.

export interface FakeAgent {
  /** The program's name as an install gives it. */
  program: string
  /** The script in this folder that stands in for it. */
  script: string
}

export type FakeBackend = 'claude-code'

export const FAKE_AGENTS: Readonly<Record<FakeBackend, Readonly<FakeAgent>>>

export function writeFakeAgent(
  bin: string,
  backend: FakeBackend,
  options?: { node?: string; platform?: NodeJS.Platform }
): string
