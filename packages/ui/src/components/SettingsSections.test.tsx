import { fireEvent, render, screen, within } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { SETTINGS_SECTIONS, SettingsSections, pluginSection, settingsGroupOf } from './SettingsSections'

/** Every section, Hosts and Devices included. */
const EVERY = SETTINGS_SECTIONS.map((section) => section.label)

describe('SettingsSections', () => {
  it('lists every section in order, marks the one on screen, and picks another', () => {
    const onSelect = vi.fn()
    render(<SettingsSections current="terminal" onSelect={onSelect} />)
    const nav = screen.getByRole('navigation', { name: 'Settings sections' })
    const rows = [...nav.querySelectorAll('button')]
    expect(rows.map((row) => row.textContent)).toEqual(EVERY)
    expect(rows.filter((row) => row.getAttribute('aria-current') === 'page').map((row) => row.textContent)).toEqual([
      'Terminal'
    ])
    fireEvent.click(screen.getByRole('button', { name: 'Browser' }))
    expect(onSelect).toHaveBeenCalledWith('browser')
  })

  it('S1: lists this desktop’s sections under Floti desktop, then the host’s under Floti host', () => {
    render(<SettingsSections current="general" onSelect={vi.fn()} />)
    const listed = (group: string): string[] =>
      [...(screen.getByRole('group', { name: new RegExp(`^${group}`) }).querySelectorAll('button'))].map(
        (row) => row.textContent ?? ''
      )
    expect(listed('Floti desktop')).toEqual([
      'General',
      'Appearance',
      'Terminal',
      'Sessions',
      'Browser',
      'Files',
      'Hosts',
      'Devices'
    ])
    expect(listed('Floti host')).toEqual([
      'Programs',
      'Tools',
      'Agents',
      'Workspace',
      'Plugins',
      'Secrets',
      'Archive',
      'Updates'
    ])
    expect(SETTINGS_SECTIONS.filter((section) => section.group === 'host').map((section) => section.id)).toEqual([
      'programs',
      'tools',
      'agents',
      'workspace',
      'plugins',
      'secrets',
      'archive',
      'updates'
    ])
    expect(settingsGroupOf('browser')).toBe('desktop')
    expect(settingsGroupOf('tools')).toBe('host')
    expect(settingsGroupOf(pluginSection('/plugins/sample'))).toBe('host')
  })

  it('S4: with one host there is no switch, and the host’s name sits beside the label in mono', () => {
    render(<SettingsSections current="tools" onSelect={vi.fn()} host={{ name: 'This PC' }} />)
    expect(screen.queryByRole('radiogroup')).toBeNull()
    const label = document.querySelector('[data-settings-host-label]') as HTMLElement
    expect(label.textContent).toBe('Floti hostThis PC')
    expect(within(label).getByText('This PC').className).toContain('font-mono')
  })

  it('S2: the Floti host label carries the host switch, which picks for the whole group; the desktop group has none', () => {
    const onHostChange = vi.fn()
    const hosts = [
      { id: 'local', name: 'This PC', remote: false },
      { id: 'h1', name: 'buildbox', remote: true }
    ]
    render(
      <SettingsSections
        current="tools"
        onSelect={vi.fn()}
        host={{ name: 'buildbox', choice: { hosts, host: 'h1', onHostChange } }}
      />
    )
    const label = document.querySelector('[data-settings-host-label]') as HTMLElement
    const sw = within(label).getByRole('radiogroup', { name: 'Settings host' })
    expect(within(sw).getByRole('radio', { checked: true }).textContent).toBe('buildbox')
    fireEvent.click(within(sw).getByRole('radio', { name: 'This PC' }))
    expect(onHostChange).toHaveBeenCalledWith('local')
    expect(
      within(screen.getByRole('group', { name: /^Floti desktop/ })).queryByRole('radiogroup')
    ).toBeNull()
  })
})

describe("SettingsSections: each plugin's page", () => {
  // Folders as Windows writes them: a path with backslashes is the common case.
  const SAMPLE = String.raw`C:\plugins\sample`
  const OFF = String.raw`C:\plugins\off`
  const plugins = [
    { path: SAMPLE, name: 'Sample', state: 'on' as const },
    { path: OFF, name: 'Off one', state: 'off' as const },
    { path: String.raw`C:\plugins\broken`, name: 'Broken', state: 'error' as const }
  ]

  it('lists every plugin under Plugins, saying which are off or failed, and opens one', () => {
    const onSelect = vi.fn()
    render(<SettingsSections current={pluginSection(OFF)} onSelect={onSelect} plugins={plugins} />)
    const rows = [...screen.getByRole('navigation', { name: 'Settings sections' }).querySelectorAll('button')].map(
      (row) => row.textContent
    )
    const at = rows.indexOf('Plugins')
    expect(rows.slice(at, at + 5)).toEqual(['Plugins', 'Sample', 'Off oneOff', 'BrokenNot loaded', 'Secrets'])
    const off = screen.getByRole('button', { name: /^Off one/ })
    expect(off.getAttribute('aria-current')).toBe('page')
    expect(off.getAttribute('title')).toBe(OFF)
    // The plugin's page is chosen, not Plugins itself.
    expect(screen.getByRole('button', { name: 'Plugins' }).getAttribute('aria-current')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Sample' }))
    expect(onSelect).toHaveBeenCalledWith(`plugin:${SAMPLE}`)
  })

  it('lists no plugins when there are none', () => {
    render(<SettingsSections current="plugins" onSelect={vi.fn()} plugins={[]} />)
    const rows = [...screen.getByRole('navigation', { name: 'Settings sections' }).querySelectorAll('button')]
    expect(rows.map((row) => row.textContent)).toEqual(EVERY)
  })
})
