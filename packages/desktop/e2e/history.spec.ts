import { fakeClaudeLogs, type FakeClaudeLog } from '../test/world'
import { claudeRunIn, expect, openHistory, sessionIdOf, startSession, terminalText, test, typeLine } from './helm'

test('a session ended in Helm is in History, found by what was said, and resumes in the folder it ran in', async ({
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
  const palette = await openHistory(window)
  const recent = palette.getByRole('listbox', { name: 'Recent sessions' }).getByRole('option', { name: /plan the release/ })
  await expect(recent).toContainText('alpha')
  await recent.click()
  await expect(palette.locator('[data-history-preview]')).toContainText('Enter runs claude --resume in')

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

  // Enter resumes it, and the palette closes.
  await window.keyboard.press('Enter')
  await expect(palette).toHaveCount(0)
  const tab = window.getByRole('tab', { name: /^plan the release/ })
  await expect(tab).toBeVisible()
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

  const resumedId = await sessionIdOf(window, /^plan the release/)
  await expect.poll(() => terminalText(window, resumedId)).toContain(`Resumed ${first.sessionId}`)
})
