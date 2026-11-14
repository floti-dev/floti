import { join } from 'node:path'
import { openStore, readArchiveStats, readHistorySessions } from '@helm/core'
import { fakeClaudeLogs, type FakeClaudeLog } from '../test/world'
import { claudeRunIn, expect, openHistory, sessionIdOf, startSession, terminalText, test, typeLine } from './helm'

test('a session ended in Helm is in History without a refresh, its archive opens, it is found by what was said, and it resumes in the folder it ran in', async ({
  helm,
  world
}) => {
  const { window } = helm
  const id = await startSession(window, 'alpha')
  const pane = window.getByRole('region', { name: 'First pane' })
  await typeLine(pane, 'plan the release')
  await expect.poll(() => terminalText(window, id)).toContain('You said: plan the release')
  const first = await claudeRunIn(world, world.projects.alpha)
  await typeLine(pane, '/exit')
  await expect(window.getByRole('tab', { name: 'alpha, ended' })).toBeVisible()

  // Ctrl+H, from the terminal: the sessions worked in last, this one among them.
  let palette = await openHistory(window)
  const recent = palette.getByRole('listbox', { name: 'Recent sessions' }).getByRole('option', { name: /plan the release/ })
  await expect(recent).toContainText('alpha')
  await recent.click()
  await expect(palette.locator('[data-history-preview]')).toContainText('Enter runs claude --resume in')

  // Ctrl+Enter reads it in a tab: the host's archive took the conversation in
  // the same pass that listed it.
  await window.keyboard.press('ControlOrMeta+Enter')
  await expect(palette).toHaveCount(0)
  await expect(window.getByRole('tab', { name: 'plan the release', exact: true })).toBeVisible()
  await expect(window.locator('[data-transcript]')).toContainText('You said: plan the release')

  // The host indexed and archived it; Helm's own database holds none of it.
  const store = openStore({ file: join(world.dataDir, 'helm.db') })
  try {
    expect(readHistorySessions(store, {}).total).toBe(0)
    expect(readArchiveStats(store, 1024 ** 3).sessions).toBe(0)
  } finally {
    store.close()
  }
  palette = await openHistory(window)

  // Typed into: Titles and prompts first, and Tab gives the conversations that mention it.
  await palette.getByRole('combobox', { name: 'Search History' }).fill('release')
  const lists = palette.getByRole('tablist', { name: 'History lists' })
  await expect(lists.getByRole('tab', { name: /^Titles and prompts/ })).toHaveAttribute('aria-selected', 'true')
  await expect(
    palette.getByRole('listbox', { name: 'Titles and prompts' }).getByRole('option', { name: /plan the release/ })
  ).toBeVisible()
  await window.keyboard.press('Tab')
  await expect(lists.getByRole('tab', { name: /^Conversations/ })).toHaveAttribute('aria-selected', 'true')
  // The session, then each line of it that matched.
  const conversations = palette.getByRole('listbox', { name: 'Conversations' })
  await expect(conversations.locator('[data-history-row="session"]')).toHaveCount(1)
  await expect(conversations.locator('[data-history-row="session"]')).toContainText('plan the release')
  await expect(conversations.locator('[data-history-row="line"]').first()).toContainText('release')
  await window.keyboard.press('Tab')
  const match = palette.getByRole('listbox', { name: 'Titles and prompts' }).getByRole('option', { name: /plan the release/ })
  await expect(match).toHaveAttribute('aria-selected', 'true')

  // Enter resumes it in a session tab of its own, and the palette closes.
  await window.keyboard.press('Enter')
  await expect(palette).toHaveCount(0)
  const session = /^plan the release, /
  await expect(window.getByRole('tab', { name: session })).toBeVisible()
  let resumed: FakeClaudeLog | undefined
  await expect
    .poll(() => {
      resumed = fakeClaudeLogs(world).find((log) => log.resumed)
      return resumed !== undefined
    })
    .toBe(true)
  const run = resumed as FakeClaudeLog
  expect(run.argv[run.argv.indexOf('--resume') + 1]).toBe(first.sessionId)
  for (const flag of ['-n', '--name', '--session-id']) expect(run.argv).not.toContain(flag)
  expect(run.cwd.toLowerCase()).toBe(world.projects.alpha.toLowerCase())

  const resumedId = await sessionIdOf(window, session)
  await expect.poll(() => terminalText(window, resumedId)).toContain(`Resumed ${first.sessionId}`)
})
