import type { Page } from '@playwright/test'
import type { AppSettings } from '@helm/core'
import { expect, test, typeLine } from './helm'

/**
 * A terminal tab: a plain shell in a folder, with no `claude` in it, opened
 * from Ctrl+N or a pane's `+`. Each is a shell of its own and ends with its
 * tab. A project's own terminal, from its row in the sidebar, is the host's
 * shell in the project's folder, kept across a reload.
 */

/** What the window's terminal inspector says of a shell; see `describeShellTerminals`. */
interface ShellReport {
  path: string
  lines: string[]
}

async function shells(window: Page): Promise<ShellReport[]> {
  return window.evaluate(
    () => (window as unknown as { __helmTerminals: () => { shells: ShellReport[] } }).__helmTerminals().shells
  )
}

const tabShells = async (window: Page): Promise<ShellReport[]> =>
  (await shells(window)).filter((shell) => shell.path.startsWith('terminal:'))

/** The window's routed bridge (`inspect.ts`): host features over the window's own connection. */
type Bridge = { __helmRouted: { invoke: (channel: string, payload?: unknown) => Promise<unknown> } }
const invoke = <T>(window: Page, channel: string): Promise<T> =>
  window.evaluate((name) => (window as unknown as Bridge).__helmRouted.invoke(name), channel) as Promise<T>

/** How many times `text` is in `screen`. */
const count = (screen: string, text: string): number => screen.split(text).length - 1

test('a terminal tab runs a plain shell in the chosen folder, one per tab, ending with its tab, and a project’s own terminal is drawn again after a reload', async ({
  helm,
  world
}) => {
  const { window } = helm
  const pane = window.getByRole('region', { name: 'First pane' })

  // From the launcher: Alt+Enter on a folder opens a terminal there, not a session.
  await expect(window.getByRole('button', { name: 'Start a session in alpha' })).toBeAttached()
  await window.keyboard.press('ControlOrMeta+N')
  const launcher = window.getByRole('dialog', { name: 'New session' })
  await expect(launcher.getByRole('combobox', { name: 'Folder' })).toBeFocused()
  await window.keyboard.type('alp')
  await expect(
    launcher.getByRole('listbox', { name: 'Folders' }).getByRole('option', { name: 'alpha' })
  ).toHaveAttribute('aria-selected', 'true')
  await window.keyboard.press('Alt+Enter')
  await expect(launcher).toBeHidden()
  const first = pane.getByRole('tab', { name: 'alpha' })
  await expect(first).toHaveAttribute('aria-selected', 'true')
  await expect(first).toHaveAttribute('data-tab', /^terminal:\d+$/)

  await typeLine(pane, 'node -e "console.log(\'cwd=\' + process.cwd())"')
  await expect
    .poll(async () => (await tabShells(window)).flatMap((shell) => shell.lines).join('\n'))
    .toContain(`cwd=${world.projects.alpha}`)
  // Nothing about it is a session.
  await expect(window.locator('[data-status-sessions="none"]')).toBeVisible()

  // From the pane's +: the same folder again is a second shell, not the first.
  await pane.getByRole('button', { name: 'New tab' }).click()
  await window.getByRole('menu', { name: 'New tab' }).getByRole('menuitem', { name: 'Terminal' }).click()
  await window.getByRole('menu', { name: 'Open a terminal in' }).getByRole('menuitem', { name: 'alpha' }).click()
  await expect(pane.getByRole('tab', { name: 'alpha' })).toHaveCount(2)
  await expect.poll(async () => (await tabShells(window)).length).toBe(2)

  // Closing one ends its shell and leaves the other.
  await first.first().hover()
  await pane.getByRole('button', { name: 'Close alpha' }).first().click()
  await expect(pane.getByRole('tab', { name: 'alpha' })).toHaveCount(1)
  await expect.poll(async () => (await tabShells(window)).length).toBe(1)
  await pane.getByRole('tab', { name: 'alpha' }).hover()
  await pane.getByRole('button', { name: 'Close alpha' }).click()
  await expect.poll(async () => (await tabShells(window)).length).toBe(0)

  // The project's own shell, under its tab: the tab is kept across a reload.
  const projectText = async (): Promise<string> =>
    (await shells(window))
      .filter((shell) => shell.path.toLowerCase() === world.projects.alpha.toLowerCase())
      .flatMap((shell) => shell.lines)
      .join('\n')
  await window.getByRole('button', { name: 'alpha main' }).click()
  await expect(pane.getByRole('textbox', { name: 'Terminal input' })).toBeAttached()
  // Its folder's name, as the shell in the project's folder prints it.
  await typeLine(pane, `node -e "console.log('here=' + require('path').basename(process.cwd()))"`)
  const cwdLine = 'here=alpha'
  await expect.poll(projectText).toContain(cwdLine)
  // The project's tab is written down before the reload, as any is in time.
  await expect
    .poll(async () => JSON.stringify((await invoke<AppSettings>(window, 'settings:read')).paneLayout ?? {}))
    .toContain('"kind":"project"')

  await window.reload()
  await expect(pane.getByRole('textbox', { name: 'Terminal input' })).toBeAttached()
  // Drawn from the host's screen, once.
  await expect.poll(projectText).toContain(cwdLine)
  expect(count(await projectText(), cwdLine)).toBe(1)
  await typeLine(pane, 'node -e "console.log(\'again=\' + (6 * 7))"')
  await expect.poll(projectText).toContain('again=42')
  expect(count(await projectText(), cwdLine)).toBe(1)
})
