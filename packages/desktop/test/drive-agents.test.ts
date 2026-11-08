import { execFileSync, execSync } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, relative, isAbsolute } from 'node:path'
import { describe, expect, it } from 'vitest'
import { openStore, readSettings, writeSettings, type AppSettings } from '@helm/core'
import { prepareDriveAgents } from '../scripts/drive-agents'

/**
 * `pnpm dev --drive` chooses the fake agents in its dev database on every
 * launch, whatever the copy of the installed app's database chose, and keeps
 * what the fakes write under its own folder.
 */

/** A dev run's folder, as `isolate('dev')` lays it out, under the test run's root. */
function devRun(): { root: string; dataDir: string } {
  const root = realpathSync.native(mkdtempSync(join(tmpdir(), 'helm drive-')))
  return { root, dataDir: join(root, 'helm-data') }
}

function settingsIn(dataDir: string): AppSettings {
  const store = openStore({ file: join(dataDir, 'helm.db') })
  try {
    return readSettings(store)
  } finally {
    store.close()
  }
}

function under(parent: string, child: string): boolean {
  const rel = relative(parent, child)
  return rel !== '' && !rel.startsWith('..') && !isAbsolute(rel)
}

/** What the program answers to `--version`, run as the host would find it on this OS. */
function versionOf(program: string, configDir: string): string {
  const env: NodeJS.ProcessEnv = { ...process.env, CLAUDE_CONFIG_DIR: configDir }
  delete env['FAKE_AGENT_VERSION']
  return process.platform === 'win32'
    ? execSync(`"${program}" --version`, { encoding: 'utf8', env })
    : execFileSync(program, ['--version'], { encoding: 'utf8', env })
}

describe('the agents a driven dev window runs', () => {
  it('chooses the fake claude over the one the copied database chose, under the run folder', () => {
    const { root, dataDir } = devRun()
    const real = join(root, 'elsewhere', 'claude')
    const copied = openStore({ file: join(dataDir, 'helm.db') })
    try {
      writeSettings(copied, { claudePath: real, updateCheck: false })
    } finally {
      copied.close()
    }

    const drive = prepareDriveAgents(root, dataDir, { fresh: false })

    expect(drive.agents.map((agent) => agent.backend)).toEqual(['claude-code'])
    const [claude] = drive.agents
    if (claude === undefined) throw new Error('no fake claude')
    expect(under(join(root, 'fake-agents'), claude.program)).toBe(true)
    const settings = settingsIn(dataDir)
    expect(settings.claudePath).toBe(claude.program)
    expect(settings.updateCheck).toBe(false)
    expect(versionOf(claude.program, drive.claudeConfigDir)).toBe('2.1.999 (Claude Code)\n')
  })

  it('gives Claude Code a signed-in config folder of its own beside the fakes', () => {
    const { root, dataDir } = devRun()

    const drive = prepareDriveAgents(root, dataDir, { fresh: false })

    expect(under(join(root, 'fake-agents'), drive.claudeConfigDir)).toBe(true)
    expect(drive.env).toEqual({ CLAUDE_CONFIG_DIR: drive.claudeConfigDir })
    expect(JSON.parse(readFileSync(join(drive.claudeConfigDir, '.claude.json'), 'utf8'))).toMatchObject({
      hasCompletedOnboarding: true
    })
  })

  it('chooses the fakes in a first-run database it makes itself', () => {
    const { root, dataDir } = devRun()

    const drive = prepareDriveAgents(root, dataDir, { fresh: true })

    expect(settingsIn(dataDir).claudePath).toBe(drive.agents[0]?.program)
  })

  it('keeps what the fakes wrote across launches, and starts it over when fresh', () => {
    const { root, dataDir } = devRun()
    const first = prepareDriveAgents(root, dataDir, { fresh: false })
    const transcript = join(first.claudeConfigDir, 'projects', 'p', 'conversation.jsonl')
    mkdirSync(join(first.claudeConfigDir, 'projects', 'p'), { recursive: true })
    writeFileSync(transcript, '{}\n')

    const again = prepareDriveAgents(root, dataDir, { fresh: false })
    expect(again.claudeConfigDir).toBe(first.claudeConfigDir)
    expect(existsSync(transcript)).toBe(true)
    expect(settingsIn(dataDir).claudePath).toBe(again.agents[0]?.program)

    prepareDriveAgents(root, dataDir, { fresh: true })
    expect(existsSync(transcript)).toBe(false)
    expect(existsSync(join(first.claudeConfigDir, '.claude.json'))).toBe(true)
  })
})
