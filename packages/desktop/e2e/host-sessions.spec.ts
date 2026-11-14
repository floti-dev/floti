import type { Page } from '@playwright/test'
import type { AppSettings } from '@helm/core'
import { fakeClaudeLogs } from '../test/world'
import {
  answer,
  claudeRunIn,
  closeKeepRunning,
  closeWindow,
  expect,
  flotiChildren,
  flotiHostsAmong,
  launchHelm,
  mainPid,
  processAlive,
  sessionIdOf,
  startSession,
  terminalText,
  test,
  typeLine
} from './helm'

/**
 * The host's own lifetime around Helm's: sessions run in a `floti host` Helm
 * starts in its own data directory, reached through `floti bridge` from
 * Helm's client folder, and outlive a Helm that quits keeping them, one that
 * is killed, and a host that dies. `pnpm test:e2e` builds both.
 */

/** The window's routed bridge (`inspect.ts`): host features over the window's own connection. */
type Bridge = { __helmRouted: { invoke: (channel: string, payload?: unknown) => Promise<unknown> } }
const invoke = <T>(window: Page, channel: string): Promise<T> =>
  window.evaluate((name) => (window as unknown as Bridge).__helmRouted.invoke(name), channel) as Promise<T>

test('closing the window asks first; Cancel changes nothing, and Keep running and quit leaves the sessions in the host, where the next start brings back this device’s tabs, panes and split, their screens and input', async ({
  helm,
  world
}) => {
  let { window } = helm
  const alpha = await startSession(window, 'alpha')
  await typeLine(window.getByRole('region', { name: 'First pane' }), 'draft the schema')
  await expect.poll(() => terminalText(window, alpha)).toContain('You said: draft the schema')
  const beta = await startSession(window, 'beta')
  const runs = [(await claudeRunIn(world, world.projects.alpha)).pid, (await claudeRunIn(world, world.projects.beta)).pid]
  await expect.poll(async () => flotiChildren(await mainPid(helm)).length).toBe(3)
  const [host] = flotiHostsAmong(flotiChildren(await mainPid(helm)))
  expect(host).toBeDefined()
  await expect(window.getByRole('contentinfo').locator('[data-status-host]')).toHaveText('This PC · quit ends sessions')

  // beta in a pane of its own, with the sessions pane beside it there; then beta's tab closed, kept running.
  await window.getByRole('button', { name: /^Split: move this tab to a pane of its own/ }).click()
  await window.locator('[data-open-sessions]').click()
  const first = window.getByRole('region', { name: 'First pane' })
  const second = window.getByRole('region', { name: 'Second pane' })
  await expect(second.getByRole('tab')).toHaveText(['beta', 'Sessions'])
  await closeKeepRunning(window, second.getByRole('tab', { name: 'beta, ready' }), 'beta')
  await expect(first.getByRole('tab')).toHaveText(['alpha'])
  await expect(second.getByRole('tab')).toHaveText(['Sessions'])

  // Saved in this device's own settings: alpha's tab, the split, and no tab for beta.
  const saved = async (): Promise<string> =>
    JSON.stringify((await invoke<AppSettings>(window, 'settings:read')).paneLayout ?? null)
  await expect
    .poll(async () => {
      const layout = await saved()
      return layout.includes(`"id":${String(alpha)}`) && !layout.includes(`"id":${String(beta)}`) && layout.includes('"axis"')
    })
    .toBe(true)
  const before = await saved()

  const question = window.getByRole('alertdialog', { name: 'Quitting Helm ends 2 sessions on this PC’s host.' })
  await closeWindow(helm.app)
  await expect(question.getByRole('button')).toHaveText(['Cancel', 'Keep running and quit', 'End and quit'])
  await question.getByRole('button', { name: 'Cancel' }).click()
  await expect(question).toHaveCount(0)
  await expect(first.getByRole('tab', { name: 'alpha, ready' })).toBeVisible()
  expect([host!, ...runs].every(processAlive)).toBe(true)

  const main = await mainPid(helm)
  await closeWindow(helm.app)
  await answer(question.getByRole('button', { name: 'Keep running and quit' }))
  await expect.poll(() => processAlive(main)).toBe(false)
  // Helm is gone; the host and the sessions it runs are not.
  expect([host!, ...runs].every(processAlive)).toBe(true)

  const next = await launchHelm(world)
  Object.assign(helm, next)
  window = next.window
  const firstAgain = window.getByRole('region', { name: 'First pane' })
  const secondAgain = window.getByRole('region', { name: 'Second pane' })
  await expect(firstAgain.getByRole('tab')).toHaveText(['alpha'])
  await expect(secondAgain.getByRole('tab')).toHaveText(['Sessions'])
  expect(await sessionIdOf(window, 'alpha, ready')).toBe(alpha)
  // Nothing opened for beta, so the arrangement stands as it was saved.
  await expect(window.getByRole('tab', { name: /^beta/ })).toHaveCount(0)
  expect(await saved()).toBe(before)

  await expect.poll(() => terminalText(window, alpha)).toContain('You said: draft the schema')
  await typeLine(firstAgain, 'add the indexes')
  await expect.poll(() => terminalText(window, alpha)).toContain('You said: add the indexes')

  await window.locator('[data-open-sessions]').click()
  await expect(
    window.getByRole('region', { name: 'Not open here' }).locator(`[data-not-open-here="${String(beta)}"]`)
  ).toContainText('beta')
  expect(runs.every(processAlive)).toBe(true)

  // Reattached to the host it left running, and the answer was for that quit only.
  expect(flotiHostsAmong(flotiChildren(await mainPid(helm)))).toEqual([])
  const { quitSessions } = await invoke<AppSettings>(window, 'settings:read')
  expect(quitSessions['local'] ?? 'end').toBe('end')
  await expect(window.getByRole('contentinfo').locator('[data-status-host]')).toHaveText('This PC · quit ends sessions')
})

test('Settings’ Stop host says how many sessions end; Cancel changes nothing, and Stop host ends them and the host', async ({
  helm,
  world
}) => {
  const { window } = helm
  await startSession(window, 'alpha')
  await startSession(window, 'beta')
  const pids = [
    (await claudeRunIn(world, world.projects.alpha)).pid,
    (await claudeRunIn(world, world.projects.beta)).pid
  ]
  await expect.poll(async () => flotiChildren(await mainPid(helm)).length).toBe(3)
  const [host] = flotiHostsAmong(flotiChildren(await mainPid(helm)))

  await window.getByRole('navigation', { name: 'Destinations' }).getByRole('button', { name: 'Settings' }).click()
  await window.getByRole('navigation', { name: 'Settings sections' }).getByRole('button', { name: 'Sessions', exact: true }).click()
  const ask = window.getByRole('alertdialog', { name: 'Stopping this PC’s host ends 2 sessions.' })

  await window.locator('[data-settings-stop-host]').click()
  await ask.getByRole('button', { name: 'Cancel' }).click()
  await expect(ask).toHaveCount(0)
  expect([host!, ...pids].every(processAlive)).toBe(true)

  await window.locator('[data-settings-stop-host]').click()
  await ask.getByRole('button', { name: 'Stop host' }).click()
  await expect.poll(() => [host!, ...pids].filter(processAlive)).toEqual([])
})

test('after Helm is killed outright, its sessions run on in the host, and the next start reattaches to that host and shows them, offering no restore', async ({
  helm,
  world
}) => {
  let { window } = helm
  const id = await startSession(window, 'alpha')
  await typeLine(window.getByRole('region', { name: 'First pane' }), 'draft the schema')
  await expect.poll(() => terminalText(window, id)).toContain('You said: draft the schema')
  // Its conversation is known, and the pane layout names it.
  await expect
    .poll(async () => {
      const { paneLayout } = await invoke<AppSettings>(window, 'settings:read')
      return JSON.stringify(paneLayout ?? {}).includes(`"id":${String(id)}`)
    })
    .toBe(true)
  const run = await claudeRunIn(world, world.projects.alpha)
  await expect.poll(async () => flotiChildren(await mainPid(helm)).length).toBe(3)
  const floti = flotiChildren(await mainPid(helm))
  const hosts = flotiHostsAmong(floti)
  expect(hosts).toHaveLength(1)
  const host = hosts[0]!
  const bridges = floti.filter((pid) => pid !== host)

  // Outright: on macOS and Linux a plain kill's SIGTERM is a request to quit,
  // which Electron honours as a clean quit.
  process.kill(await mainPid(helm), 'SIGKILL')
  // The bridges end with the Helm whose pipes they were; the host and the
  // session it runs do not.
  await expect.poll(() => bridges.filter(processAlive)).toEqual([])
  expect([host, run.pid].every(processAlive)).toBe(true)

  const next = await launchHelm(world)
  Object.assign(helm, next)
  window = next.window
  await expect(window.getByRole('tab', { name: 'alpha, ready' })).toBeVisible()
  expect(await sessionIdOf(window, 'alpha, ready')).toBe(id)
  await expect.poll(() => terminalText(window, id)).toContain('You said: draft the schema')

  // Reached through the host already running: this Helm started only its bridges.
  await expect.poll(async () => flotiChildren(await mainPid(helm)).length).toBe(2)
  expect(flotiHostsAmong(flotiChildren(await mainPid(helm)))).toEqual([])
  expect([host, run.pid].every(processAlive)).toBe(true)
  // Nothing ended, so there is nothing to offer back and nothing was resumed.
  await expect(window.getByRole('region', { name: /when Helm closed/ })).toHaveCount(0)
  expect(fakeClaudeLogs(world).filter((log) => log.resumed)).toEqual([])
})

test('when the host Helm started dies, Helm starts a new host and reconnects, and the tab takes its session back running, with nothing resumed', async ({
  helm,
  world
}) => {
  const { window } = helm
  const id = await startSession(window, 'alpha')
  const pane = window.getByRole('region', { name: 'First pane' })
  await typeLine(pane, 'sketch the migration')
  await expect.poll(() => terminalText(window, id)).toContain('You said: sketch the migration')
  await claudeRunIn(world, world.projects.alpha)
  await expect.poll(async () => flotiHostsAmong(flotiChildren(await mainPid(helm))).length).toBe(1)
  const [host] = flotiHostsAmong(flotiChildren(await mainPid(helm)))

  // The banner is up only while the window has no host, so the page keeps
  // every text it showed rather than the test racing to see it.
  await window.evaluate(() => {
    const seen: string[] = []
    ;(window as unknown as { __hostStopped: string[] }).__hostStopped = seen
    new MutationObserver(() => {
      const banner = document.querySelector('[data-host-stopped-banner]')
      const text = banner?.textContent ?? null
      if (text !== null && seen.at(-1) !== text) seen.push(text)
    }).observe(document.body, { childList: true, subtree: true, characterData: true })
  })

  // Outright, as a crash would: this test's own host, never another.
  process.kill(host!, 'SIGKILL')

  // Helm started the host that died, so it starts a new one and reconnects.
  await expect
    .poll(async () => {
      const hosts = flotiHostsAmong(flotiChildren(await mainPid(helm)))
      return hosts.length === 1 && hosts[0] !== host
    })
    .toBe(true)
  await expect(window.locator('[data-host-stopped-banner]')).toHaveCount(0)
  expect(await window.evaluate(() => (window as unknown as { __hostStopped: string[] }).__hostStopped)).toEqual([
    'This PC’s host stopped. Starting it again…'
  ])

  // The session ran on in its keeper: its tab is the same session, running,
  // drawn from the new host's redraw, and it takes keys again.
  await expect(pane.locator('[data-session-ended]')).toHaveCount(0)
  await expect(pane.locator('[data-terminal-waiting]')).toHaveCount(0)
  expect(await sessionIdOf(window, /^alpha, /)).toBe(id)
  expect(await terminalText(window, id)).toContain('You said: sketch the migration')
  await typeLine(pane, 'carry on')
  await expect.poll(() => terminalText(window, id)).toContain('You said: carry on')

  // Nothing was lost, so nothing is offered back and nothing is resumed.
  await expect(window.getByRole('tab', { name: 'Restore sessions' })).toHaveCount(0)
  expect(fakeClaudeLogs(world).filter((log) => log.resumed)).toEqual([])
})
