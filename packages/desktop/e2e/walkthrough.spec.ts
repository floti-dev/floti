import { existsSync } from 'node:fs'
import { join } from 'node:path'
import { openStore, readSettings } from '@floti/core'
import { seedSettings } from '../test/world'
import { claudeRunIn, expect, test as base } from './desktop'

/**
 * The guided first run, from a first start to a running fake-agent session.
 *
 * The world starts as a first start leaves it: nothing to scan, no setup
 * stamp, the walkthrough at step 1 (`settleWalkthrough`) and the fake
 * `claude` chosen, so the agents step finds it installed and signed in by its
 * own check. The native folder picker is answered in main with the world's
 * projects folder, since a native dialog has no automation surface.
 */
const test = base.extend({
  world: async ({ world }, use) => {
    seedSettings(world, {
      scanRoots: [],
      firstRunCompletedAt: null,
      walkthrough: { step: 1, workspace: null }
    })
    await use(world)
  }
})

const FIRST_MESSAGE = 'Read AGENTS.md and tell me what this workspace is for.'

test('a first start walks from Welcome to a running session in a new Starter workspace', async ({ desktop, world }) => {
  const { app } = desktop
  const setup = desktop.window
  await desktop.app.evaluate(({ dialog }, folder) => {
    dialog.showOpenDialog = (async () => ({ canceled: false, filePaths: [folder] })) as typeof dialog.showOpenDialog
  }, world.projectsDir)

  // Step 1, in the setup window: no shell exists yet.
  await expect(setup.getByRole('heading', { name: 'Welcome to Floti' })).toBeVisible()
  expect(await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().length)).toBe(1)
  await setup.getByRole('button', { name: 'Get started' }).click()

  // Step 2: the backend's own check finds the fake installed and signed in.
  await expect(setup.getByRole('heading', { name: 'Your agents' })).toBeVisible()
  const agent = setup.locator('[data-walkthrough-agent="claude-code"]')
  await expect(agent.getByText('Installed')).toBeVisible()
  await expect(agent.getByText('Signed in')).toBeVisible()
  await setup.getByRole('button', { name: 'Continue' }).click()

  // Step 3: the host Floti started answers, and quitting ends sessions by default.
  await expect(setup.getByRole('heading', { name: 'The host on this computer' })).toBeVisible()
  await expect(setup.locator('[data-walkthrough-host="local"]').getByText('Running')).toBeVisible()
  await expect(setup.getByRole('radio', { name: /End my sessions/ })).toHaveAttribute('aria-checked', 'true')
  await setup.getByRole('button', { name: 'Continue' }).click()

  // Step 4: a new workspace from Starter, inside the folder the picker gave.
  await expect(setup.getByRole('heading', { name: 'A workspace' })).toBeVisible()
  await setup.getByRole('button', { name: 'Choose…' }).click()
  await expect(setup.getByLabel('Create it inside')).toHaveValue(world.projectsDir)
  const shellOpens = app.waitForEvent('window')
  await setup.getByRole('button', { name: 'Create workspace' }).click()

  // The setup window hands over: the shell opens with steps 5 and 6 in its main area.
  const shell = await shellOpens
  const workspace = join(world.projectsDir, 'my-work')
  expect(existsSync(join(workspace, 'workspace.yaml'))).toBe(true)
  expect(existsSync(join(workspace, 'AGENTS.md'))).toBe(true)
  await expect.poll(() => setup.isClosed()).toBe(true)
  await expect(shell.getByRole('heading', { name: 'Around the window' })).toBeVisible()
  await shell.getByRole('button', { name: 'Continue' }).click()

  // Step 6: start Claude Code in my-work with a first message.
  await expect(shell.getByRole('heading', { name: 'Your first session' })).toBeVisible()
  await expect(shell.getByText('Start an agent in my-work.')).toBeVisible()
  await shell.getByRole('textbox', { name: /First message/ }).fill(FIRST_MESSAGE)
  await shell.getByRole('button', { name: 'Start session' }).click()
  await expect(shell.getByRole('tab', { name: 'my-work, ready' })).toBeVisible()

  const run = await claudeRunIn(world, workspace)
  expect(run.openingPrompt).toBe(FIRST_MESSAGE)

  // Done: the walkthrough is not due again, and the workspace is in the launcher.
  await expect(shell.getByRole('heading', { name: 'Your first session' })).toBeHidden()
  await expect(shell.getByRole('button', { name: 'Start a session in my-work' })).toBeAttached()
  // Read beside the running app: main wrote it as step 6 finished.
  const store = openStore({ file: join(world.dataDir, 'desktop.db') })
  try {
    expect(readSettings(store).walkthrough).toBeNull()
  } finally {
    store.close()
  }
})
