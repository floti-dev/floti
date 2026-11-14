import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import type { Page } from '@playwright/test'
import {
  archiveTranscriptFile,
  indexUsageFile,
  openStore,
  renameHistorySession,
  writeSettings
} from '@helm/core'
import { appendPrompts, claudeTree, sessionUuid, transcriptFile } from '../test/history-fixture'
import type { World } from '../test/world'
import type { OwnDataCopyState } from '../src/shared/ipc'
import { expect, openHistory, test as base } from './helm'

/**
 * What only Helm's own database holds - a conversation it archived whose
 * transcript Claude Code has since deleted, the name given to it in History
 * and its usage - is copied into the local host, and reads there as it did
 * from helm.db.
 */

const KEPT = sessionUuid(0x11)
const PROMPT = 'draft the migration'
const REPLY = 'The migration adds two columns to the ledger.'
const NAME = 'Migration plan'

const helmDb = (world: World): string => join(world.dataDir, 'helm.db')

/** The `n`th message of a conversation: its session id with the fourth group numbering the message. */
const messageUuid = (sessionId: string, n: number): string =>
  `${sessionId.slice(0, 19)}${n.toString(16).padStart(4, '0')}${sessionId.slice(23)}`

function archive(world: World, sessionId: string, texts: readonly string[], at: number): void {
  const store = openStore({ file: helmDb(world) })
  try {
    archiveTranscriptFile(store, {
      file: transcriptFile(claudeTree(world.claudeDir), sessionId, world.projects.alpha),
      sessionId,
      tail: {
        rows: texts.map((text, index) => ({
          uuid: messageUuid(sessionId, index + 1),
          sessionId,
          role: index % 2 === 0 ? ('user' as const) : ('assistant' as const),
          at: at + index,
          text
        })),
        bytes: 100,
        reset: false,
        skipped: 0,
        read: 100
      }
    })
  } finally {
    store.close()
  }
}

/**
 * Helm's database as an earlier version left it. The conversation's prompts
 * stay in history.jsonl; its transcript file was never written, as after
 * Claude Code reaped it.
 */
function seedOwnData(world: World): void {
  const at = Date.now() - 60 * 60 * 1000
  appendPrompts(claudeTree(world.claudeDir), [{ sessionId: KEPT, project: world.projects.alpha, display: PROMPT, at }])
  archive(world, KEPT, [PROMPT, REPLY], at)
  const store = openStore({ file: helmDb(world) })
  try {
    renameHistorySession(store, KEPT, NAME)
    indexUsageFile(store, {
      file: transcriptFile(claudeTree(world.claudeDir), KEPT, world.projects.alpha),
      tail: {
        rows: [
          {
            uuid: messageUuid(KEPT, 2),
            at: at + 1,
            model: 'claude-opus-4-1',
            input: 1200,
            output: 800,
            cacheWrite5m: 0,
            cacheWrite1h: 0,
            cacheRead: 0
          }
        ],
        bytes: 100,
        reset: false,
        skipped: 0
      }
    })
    // Helm's profiles and plugins were copied into this host before its own
    // data was: that copy's markers do not stand in for this one's.
    const now = new Date().toISOString()
    writeSettings(store, { profilesCopiedToHostAt: now, pluginsCopiedToHostAt: now })
  } finally {
    store.close()
  }
}

const test = base.extend({
  world: async ({ world }, use) => {
    seedOwnData(world)
    await use(world)
  }
})

const copyState = (window: Page): Promise<OwnDataCopyState> =>
  window.evaluate(
    () =>
      (
        window as unknown as { helm: { invoke: (channel: string) => Promise<OwnDataCopyState> } }
      ).helm.invoke('ownData:state')
  )

/**
 * History names the session as it was named in Helm, says its transcript is
 * gone but the conversation was kept, and Enter reads it from the archive in a
 * tab named the same.
 */
async function showsTheArchivedConversation(window: Page): Promise<void> {
  const palette = await openHistory(window)
  const row = palette.getByRole('option', { name: new RegExp(`^${NAME}`) })
  await expect(row).toBeVisible()
  await row.click()
  await expect(palette.locator('[data-history-preview]')).toContainText(
    'Floti kept the conversation, so it can be read but not resumed.'
  )
  await window.keyboard.press('Enter')
  await expect(palette).toHaveCount(0)
  await expect(window.getByRole('tab', { name: NAME, exact: true })).toBeVisible()
  await expect(window.locator('[data-transcript]')).toContainText(REPLY)
  await expect(window.getByText(REPLY)).toHaveCount(1)
}

test('History and the archived conversation show what only helm.db held, once copied into the host', async ({
  world,
  helm
}) => {
  const before = readFileSync(helmDb(world))
  await expect.poll(async () => (await copyState(helm.window)).state).toBe('finished')

  await showsTheArchivedConversation(helm.window)

  await expect(helm.window.locator('[data-own-data-banner]')).toHaveCount(0)
  // The copy wrote the host's database, never Helm's.
  expect(readFileSync(helmDb(world)).equals(before)).toBe(true)
})
