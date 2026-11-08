// The agents a `pnpm dev --drive` window runs: the fakes in `test/fake-agents`,
// never a real agent.
//
// A driven window is one an agent clicks through, and a session it starts, a
// restore, a History resume or an agent-started child would otherwise run the
// real `claude` and spend the user's account on it. So each fake is written as
// a program under the dev run's own folder and chosen in the dev database,
// after its copy from the installed app and on every launch. Main sends that
// choice to the host on every connect, before anything can launch, and the
// host starts a chosen program as given, never one it finds by name.
//
// Claude Code's config folder moves with it (`CLAUDE_CONFIG_DIR`), to a folder
// beside the fakes, so what the fake writes (its session records, history and
// transcripts) never lands in the real `~/.claude`. That folder starts with
// the onboarding record a signed-in CLI leaves, which is all Helm reads of a
// sign-in, and is kept across launches so a fake conversation can be resumed;
// `--fresh` starts it over.

import { existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { openStore, writeSettings, type AppSettings } from '@helm/core'
import { FAKE_AGENTS, writeFakeAgent, type FakeBackend } from '../../../test/fake-agents/programs.mjs'

/** The setting that chooses each backend's program in Helm. */
const PROGRAM_SETTING = { 'claude-code': 'claudePath' } as const satisfies Record<FakeBackend, keyof AppSettings>

export interface DriveAgent {
  backend: FakeBackend
  /** The fake's program, chosen in the dev database. */
  program: string
}

export interface DriveAgents {
  agents: DriveAgent[]
  /** Claude Code's config folder for the dev window, beside the fakes. */
  claudeConfigDir: string
  /** What the dev window's environment gets, so its host and sessions get it too. */
  env: Record<string, string>
}

export interface DriveAgentsOptions {
  /** Start the fakes' folder over, as `--fresh` starts the database over. */
  fresh: boolean
  /** The Node each fake runs under; this one by default. */
  node?: string
  platform?: NodeJS.Platform
}

/**
 * Writes every fake under `<root>/fake-agents` and chooses each in
 * `<dataDir>/helm.db`, whatever the copied database chose.
 */
export function prepareDriveAgents(root: string, dataDir: string, options: DriveAgentsOptions): DriveAgents {
  const folder = join(root, 'fake-agents')
  if (options.fresh) rmSync(folder, { recursive: true, force: true })
  const bin = join(folder, 'bin')
  const claudeConfigDir = join(folder, 'claude')
  mkdirSync(bin, { recursive: true })
  mkdirSync(claudeConfigDir, { recursive: true })

  const onboarding = join(claudeConfigDir, '.claude.json')
  if (!existsSync(onboarding)) {
    writeFileSync(onboarding, JSON.stringify({ hasCompletedOnboarding: true, userID: 'helm-dev-drive' }))
  }

  const write = {
    ...(options.node === undefined ? {} : { node: options.node }),
    ...(options.platform === undefined ? {} : { platform: options.platform })
  }
  const backends = Object.keys(FAKE_AGENTS) as FakeBackend[]
  const agents = backends.map((backend) => ({ backend, program: writeFakeAgent(bin, backend, write) }))

  const patch: Partial<AppSettings> = {}
  for (const { backend, program } of agents) patch[PROGRAM_SETTING[backend]] = program
  const store = openStore({ file: join(dataDir, 'helm.db') })
  try {
    writeSettings(store, patch)
  } finally {
    store.close()
  }

  return { agents, claudeConfigDir, env: { CLAUDE_CONFIG_DIR: claudeConfigDir } }
}
