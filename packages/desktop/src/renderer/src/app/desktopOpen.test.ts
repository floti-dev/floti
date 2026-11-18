import { LOCAL_HOST_KEY, type PaneRef } from '@floti/core/types'
import { SETTINGS_SECTIONS } from '@floti/ui'
import { describe, expect, it } from 'vitest'
import desktopTable from '../../../../../../crates/host/src/server/desktop_tools.json'
import { openForAgent, type DesktopOpenWindow } from './desktopOpen'

const ROOT = 'C:\\work\\my project'
const FILE = 'C:\\work\\my project\\src\\main.rs'

/** A window that records what it was asked to do, with the caller's tab at `anchor`. */
function windowWith(anchor: string | null) {
  const done: unknown[][] = []
  const window: DesktopOpenWindow = {
    anchorOf: (host, session) => {
      done.push(['anchorOf', host, session])
      return anchor
    },
    openBeside: (ref: PaneRef, at) => done.push(['openBeside', ref, at]),
    revealLine: (host, path, line) => done.push(['revealLine', host, path, line]),
    showFolder: (host, root, path) => done.push(['showFolder', host, root, path]),
    showHistory: (host, search) => done.push(['showHistory', host, search]),
    showSettingsSection: (section) => done.push(['showSettingsSection', section])
  }
  return { window, done }
}

const call = (args: unknown, host = LOCAL_HOST_KEY) => ({ host, session: 7, tool: 'desktop_open', arguments: args })

const texts = (result: { content: { text: string }[] }): string[] => result.content.map((block) => block.text)

describe('desktop_open in the window', () => {
  it('opens a file beside the caller’s tab at its line, and says so in words and as JSON', async () => {
    const { window, done } = windowWith('session:7')
    const result = await openForAgent(call({ view: 'file', root: ROOT, path: FILE, line: 42 }), window)
    expect(done).toEqual([
      ['anchorOf', LOCAL_HOST_KEY, 7],
      ['revealLine', LOCAL_HOST_KEY, FILE, 42],
      ['openBeside', { kind: 'file', root: ROOT, path: FILE }, 'session:7']
    ])
    expect(result.isError).toBeUndefined()
    const [sentence, json] = texts(result)
    expect(sentence).toBe('Opened main.rs beside your tab, at line 42.')
    expect(JSON.parse(json!)).toEqual({ opened: 'file', path: FILE, line: 42, beside: 'yourTab' })
  })

  it('opens beside the pane in front when the caller’s tab is not in this window, and a remote host’s file on that host', async () => {
    const { window, done } = windowWith(null)
    const result = await openForAgent(call({ view: 'file', root: '/srv/app', path: '/srv/app/a.ts' }, 'build-box'), window)
    expect(done).toContainEqual(['openBeside', { kind: 'file', root: '/srv/app', path: '/srv/app/a.ts', host: 'build-box' }, null])
    expect(done.some(([what]) => what === 'revealLine')).toBe(false)
    expect(texts(result)[0]).toBe('Opened a.ts beside the pane in front, because your tab is not in this window.')
  })

  it('points Files at a folder, opens a project’s page beside the tab, and History with the search typed in', async () => {
    const { window, done } = windowWith('session:7')
    const folder = await openForAgent(call({ view: 'folder', root: ROOT, path: `${ROOT}\\src` }), window)
    expect(done).toContainEqual(['showFolder', LOCAL_HOST_KEY, ROOT, `${ROOT}\\src`])
    expect(texts(folder)[0]).toBe(`Showed src in the Files view, on ${ROOT}, with its changed files marked.`)

    const page = await openForAgent(call({ view: 'project', path: ROOT }), window)
    expect(done).toContainEqual(['openBeside', { kind: 'project', path: ROOT }, 'session:7'])
    expect(texts(page)[0]).toBe('Opened the page of my project beside your tab.')

    const history = await openForAgent(call({ view: 'history', search: 'login bug' }, 'build-box'), window)
    expect(done).toContainEqual(['showHistory', 'build-box', 'login bug'])
    expect(texts(history)[0]).toBe('Opened History searching for “login bug”. It takes the keyboard until the user closes it.')
  })

  it('opens Settings beside the tab on a section, or where it was left', async () => {
    const { window, done } = windowWith('session:7')
    const agents = await openForAgent(call({ view: 'settings', section: 'agents' }), window)
    expect(done).toContainEqual(['showSettingsSection', 'agents'])
    expect(done).toContainEqual(['openBeside', { kind: 'settings' }, 'session:7'])
    expect(texts(agents)[0]).toBe('Opened Settings on Agents beside your tab.')

    const plain = windowWith('session:7')
    const left = await openForAgent(call({ view: 'settings' }), plain.window)
    expect(plain.done.some(([what]) => what === 'showSettingsSection')).toBe(false)
    expect(texts(left)[0]).toBe('Opened Settings beside your tab.')
  })

  it('opens nothing for what it cannot read', async () => {
    for (const args of [null, 'file', { view: 'window' }, { view: 'file', path: FILE }, { view: 'settings', section: 'secret' }]) {
      const { window, done } = windowWith('session:7')
      const result = await openForAgent(call(args), window)
      expect(result.isError).toBe(true)
      expect(done).toEqual([])
    }
  })

  it('knows every Settings section the tool’s schema offers', () => {
    const tool = desktopTable.tools.find((each) => each.name === 'desktop_open')
    const offered = (tool?.inputSchema.properties.section as { enum: string[] }).enum
    expect(offered).toEqual(SETTINGS_SECTIONS.map((section) => section.id))
  })
})
