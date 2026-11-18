import { writeFileSync, mkdirSync } from 'node:fs'
import { join } from 'node:path'
import { isClientOnly } from '../../../crates/floti-protocol/generated/floti-protocol.generated'
import type { RecordedChannel } from '../src/main/ipc-recorder'
import { bearerOf, callTool, readMcpConfig } from '../test/mcp-client'
import { startBrowserFixture } from './browser-fixture'
import { TOKEN, installSample, pluginFrame, startServer, type SampleFixture } from './plugin-fixture'
import { claudeRunIn, expect, openHistory, startSession, terminalText, test as base, typeLine, type Desktop } from './desktop'

/**
 * Electron IPC carries only what `boundary.rs` lists as client-only: the
 * browser pane, clipboard, window, theme, dialogs and the window's own
 * connection to the host. Everything else reaches the host over that
 * connection, in the protocol's types.
 *
 * Main records every channel used and which page used it (`ipc-recorder.ts`);
 * after each workflow the test fails naming any channel the generated
 * client-only list does not cover. A plugin's background is a module the
 * host runs, and the sample's is one. Main registers nothing else, so a
 * channel it does not list could only show up here as one main answered.
 */

const test = base.extend<{ sample: SampleFixture }>({
  sample: [
    // eslint-disable-next-line no-empty-pattern
    async ({}, use) => {
      const server = await startServer()
      await use({ dir: '', server })
      await server.close()
    },
    { scope: 'test' }
  ],
  world: async ({ world, sample }, use) => {
    sample.dir = installSample(world, sample.server)
    const notes = join(world.projects.alpha, 'notes')
    mkdirSync(notes, { recursive: true })
    writeFileSync(join(notes, 'first.md'), '# First note\n\nWritten for the boundary.\n')
    writeFileSync(join(world.projects.alpha, 'CLAUDE.md'), '# Alpha\n')
    await use(world)
  }
})

/** Every channel main recorded so far, and which page used it. */
const recorded = (desktop: Desktop): Promise<RecordedChannel[]> =>
  desktop.app.evaluate(() => (globalThis as unknown as { __flotiIpcChannels: () => RecordedChannel[] }).__flotiIpcChannels())

/** What crossed Electron IPC that `boundary.rs` does not keep on the client, named. */
function strays(channels: readonly RecordedChannel[]): string[] {
  return channels
    .filter(
      ({ channel }) => !isClientOnly(channel)
    )
    .map(({ channel, kind, page }) => `${channel} (${kind}, ${page})`)
    .sort()
}

/** Fails naming every stray channel, and any page but the desktop's own that used a page's connection. */
async function expectOnlyClientChannels(desktop: Desktop): Promise<void> {
  const channels = await recorded(desktop)
  expect(channels.length).toBeGreaterThan(0)
  expect(strays(channels)).toEqual([])
  const foreign = channels.filter(({ channel, page }) => channel.startsWith('hostLink:') && page === 'other')
  expect(foreign).toEqual([])
}

const destination = (desktop: Desktop, name: string | RegExp) =>
  desktop.window.getByRole('navigation', { name: 'Destinations' }).getByRole('button', { name, exact: typeof name === 'string' })

test('the launcher, a session and its terminal, and a project’s terminal use only client channels', async ({
  desktop,
  world
}) => {
  const { window } = desktop
  const pane = window.getByRole('region', { name: 'First pane' })
  await window.getByRole('button', { name: 'alpha main' }).click()
  await expect(pane.getByRole('textbox', { name: 'Terminal input' })).toBeAttached()
  const id = await startSession(window, 'alpha')
  await typeLine(pane, 'hello boundary')
  await expect.poll(() => terminalText(window, id)).toContain('You said: hello boundary')
  await claudeRunIn(world, world.projects.alpha)
  await expectOnlyClientChannels(desktop)
})

test('history comes over the window’s own connection', async ({ desktop }) => {
  const { window } = desktop
  const id = await startSession(window, 'alpha')
  const pane = window.getByRole('region', { name: 'First pane' })
  await typeLine(pane, 'plan the boundary')
  await expect.poll(() => terminalText(window, id)).toContain('You said: plan the boundary')
  await typeLine(pane, '/exit')
  await expect(window.getByRole('tab', { name: 'alpha, ended' })).toBeVisible()

  // History's palette lists it, and Ctrl+Enter reads its conversation from the archive in a tab.
  const palette = await openHistory(window)
  await palette.getByRole('option', { name: /plan the boundary/ }).click()
  await window.keyboard.press('ControlOrMeta+Enter')
  await expect(palette).toHaveCount(0)
  await expect(window.locator('[data-transcript]')).toContainText('You said: plan the boundary')

  const channels = await recorded(desktop)
  // Read over the window's connection: main was asked none of it.
  expect(channels.filter(({ channel }) => /^(history:(summary|sessions|changed)|archive:)/.test(channel))).toEqual([])
  expect(channels).toContainEqual({ channel: 'hostLink:open', kind: 'invoke', page: 'window' })
  await expectOnlyClientChannels(desktop)
})

test('the Files view, a note and the profile form use only client channels', async ({ desktop }) => {
  const { window } = desktop
  await window.locator('[data-rail="files"]').click()
  const picker = window.getByRole('button', { name: 'Project', exact: true })
  if ((await picker.textContent()) !== 'alpha') {
    await picker.click()
    await window.getByRole('listbox', { name: 'Projects' }).getByRole('option', { name: 'alpha', exact: true }).click()
  }
  const files = window.getByRole('group', { name: 'Project files' })
  await files.getByRole('button', { name: /^notes\b/ }).click()
  await files.getByRole('button', { name: /^first\.md/ }).click()
  await expect(window.getByRole('heading', { level: 1, name: 'First note' })).toBeVisible()

  await destination(desktop, 'Profiles').click()
  // With no profile yet, the empty page's own next step.
  await window.getByRole('button', { name: 'New profile' }).and(window.locator('[data-empty-action]')).click()
  const dialog = window.getByRole('dialog', { name: 'New profile' })
  await dialog.getByRole('textbox', { name: 'Profile name' }).fill('boundary')
  await dialog.getByRole('button', { name: 'Cancel' }).click()
  await expectOnlyClientChannels(desktop)
})

test('plugin pages, a tool called by a session, and the Browser tab use only client channels', async ({
  desktop,
  sample,
  world
}) => {
  const { window } = desktop
  await startSession(window, 'alpha')
  await window.evaluate(
    ({ token, host }) =>
      (
        window as unknown as { __flotiRouted: { invoke: (channel: string, payload: unknown) => Promise<unknown> } }
      ).__flotiRouted.invoke('secrets:save', { key: 'sample-token', value: token, hosts: [host], plugins: ['sample'] }),
    { token: TOKEN, host: sample.server.url }
  )
  await destination(desktop, /^Sample/).click()
  const panel = await pluginFrame(window, 'dist/panels/main.html')
  await panel.locator('[data-sample-item="2"]').click()
  const item = await pluginFrame(window, 'dist/tabs/item.html')
  await expect(item.locator('h1')).toHaveText('Second item')

  // Opening the item marked it read, and the page said so on the plugin's
  // channel: the sample's background module heard it through the host and
  // counted again.
  await expect(window.locator('[data-status-plugin="sample"]')).toHaveText('1 unread')

  const run = await claudeRunIn(world, world.projects.alpha)
  const config = readMcpConfig(run.argv[run.argv.indexOf('--mcp-config') + 1] ?? '')
  const plugin = config.mcpServers['floti-plugin-sample']
  if (plugin === undefined) throw new Error(`no floti-plugin-sample in ${JSON.stringify(config)}`)
  // The sample's background module answers the session's call.
  const answered = await callTool(plugin.url, bearerOf(plugin), 'list_items', { unreadOnly: true })
  expect(answered.isError).toBe(false)
  expect(JSON.parse(answered.text)).toEqual([{ id: '1', title: 'Welcome', read: false }])

  const fixture = await startBrowserFixture()
  try {
    await destination(desktop, 'Browser').click()
    const address = window.getByRole('combobox', { name: 'Address' })
    await address.fill(`${fixture.http}/`)
    await address.press('Enter')
    await expect(window.getByRole('tab', { name: /^Floti fixture one/ })).toBeVisible()
  } finally {
    await fixture.close()
  }
  await expectOnlyClientChannels(desktop)
})
