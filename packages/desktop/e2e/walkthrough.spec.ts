import { existsSync } from 'node:fs'
import { join } from 'node:path'
import { openStore, readSettings } from '@floti/core'
import { seedSettings } from '../test/world'
import { expect, test as base } from './desktop'

/**
 * The guided first run, from a first start to the shell's home screen.
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

test('a first start walks from Welcome to the home screen with a new Starter workspace selected', async ({ desktop, world }) => {
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

  // The setup window hands over: the shell opens with step 5 in its main area.
  const shell = await shellOpens
  const workspace = join(world.projectsDir, 'my-work')
  expect(existsSync(join(workspace, 'workspace.yaml'))).toBe(true)
  expect(existsSync(join(workspace, 'AGENTS.md'))).toBe(true)
  await expect.poll(() => setup.isClosed()).toBe(true)
  await expect(shell.getByRole('heading', { name: 'Around the window' })).toBeVisible()
  await expect(shell.getByText('Step 5 of 5')).toBeVisible()
  await shell.getByRole('button', { name: 'Continue' }).click()

  // Done: the home screen, with no tab open and no session started, and the
  // workspace selected in the sidebar.
  await expect(shell.getByRole('heading', { name: 'Around the window' })).toBeHidden()
  await expect(shell.locator('[data-welcome-pane]')).toBeVisible()
  await expect(shell.getByRole('tab', { name: /my-work/ })).toHaveCount(0)
  await expect(shell.getByTitle(workspace, { exact: true }).and(shell.locator('[aria-current="true"]'))).toBeVisible()
  await expect(shell.getByRole('button', { name: 'Start a session in my-work' })).toBeAttached()
  // Read beside the running app: the walkthrough is not due again.
  const store = openStore({ file: join(world.dataDir, 'desktop.db') })
  try {
    expect(readSettings(store).walkthrough).toBeNull()
  } finally {
    store.close()
  }
})
