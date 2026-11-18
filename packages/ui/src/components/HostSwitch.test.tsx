import { act, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { HostChoice, HostChoiceHost } from './HostSwitch'
import { HostSwitch } from './HostSwitch'
import { FilesRootPicker } from './FilesTree'
import { PluginsPage } from './PluginSettings'

const HOSTS = [
  { id: 'local', name: 'This PC', remote: false },
  { id: 'h1', name: 'build-box', remote: true }
] as const

/** Four hosts, the last out of reach: past what a segmented control holds. */
const FOUR: readonly HostChoiceHost[] = [
  { id: 'local', name: 'This PC', remote: false },
  { id: 'h1', name: 'buildbox', remote: true },
  { id: 'h2', name: 'mac-air', remote: true },
  { id: 'h3', name: 'lab-pi', remote: true, away: true }
]

const choice = (host: string, onHostChange = vi.fn()): HostChoice => ({ hosts: HOSTS, host, onHostChange })

afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

/**
 * Lays the switch out as a browser would: the segmented control needs
 * `needs` pixels and its row leaves the switch `room`. jsdom measures nothing.
 */
function layOut(needs: number, room: number): void {
  vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockImplementation(function (this: HTMLElement) {
    if (this.hasAttribute('data-host-switch-measure')) return needs
    if (this.hasAttribute('data-host-switch')) return room
    return 0
  })
}

describe('HostSwitch', () => {
  it('draws nothing while this PC is the only host', () => {
    const { container } = render(
      <HostSwitch surface="History" hosts={[HOSTS[0]]} host="local" onHostChange={vi.fn()} />
    )
    expect(container.innerHTML).toBe('')
    const none = render(<HostSwitch surface="History" />)
    expect(none.container.innerHTML).toBe('')
  })

  it('names the host shown, this PC first, a remote host in mono, and switches to another', async () => {
    const onHostChange = vi.fn()
    render(<HostSwitch surface="History" {...choice('local', onHostChange)} />)
    const group = screen.getByRole('radiogroup', { name: 'History host' })
    const options = within(group).getAllByRole('radio')
    expect(options.map((each) => each.textContent)).toEqual(['This PC', 'build-box'])
    expect(options[0]?.getAttribute('aria-checked')).toBe('true')
    expect(options[1]?.getAttribute('aria-checked')).toBe('false')
    expect(options[1]?.className).toContain('font-mono')
    expect(group?.getAttribute('title')).toBe('Showing This PC’s history')

    await userEvent.click(options[0]!)
    expect(onHostChange).not.toHaveBeenCalled()
    await userEvent.click(options[1]!)
    expect(onHostChange).toHaveBeenCalledWith('h1')
  })

  it('S8: is segmented with two hosts that fit, and with three short names', () => {
    layOut(160, 200)
    const { rerender } = render(<HostSwitch surface="Settings" {...choice('h1')} />)
    expect(document.querySelector('[data-host-switch]')?.getAttribute('data-host-switch-form')).toBe('segmented')
    expect(screen.queryByRole('button', { name: 'Settings host' })).toBeNull()

    const three: HostChoiceHost[] = [...HOSTS, { id: 'h2', name: 'pi', remote: true }]
    rerender(<HostSwitch surface="Settings" hosts={three} host="h2" onHostChange={vi.fn()} />)
    expect(within(screen.getByRole('radiogroup', { name: 'Settings host' })).getAllByRole('radio')).toHaveLength(3)
  })

  it('S8: is a menu with four hosts, listing each with its state, the shown one ticked, and picks another', async () => {
    const onHostChange = vi.fn()
    render(<HostSwitch surface="Settings" hosts={FOUR} host="h1" onHostChange={onHostChange} />)
    expect(screen.queryByRole('radiogroup')).toBeNull()
    const button = screen.getByRole('button', { name: 'Settings host' })
    expect(button.textContent).toBe('buildbox')
    expect(button.getAttribute('aria-haspopup')).toBe('listbox')
    expect(button.getAttribute('aria-expanded')).toBe('false')
    expect(button.querySelector('[data-host-state]')?.getAttribute('data-host-state')).toBe('connected')

    await userEvent.click(button)
    expect(button.getAttribute('aria-expanded')).toBe('true')
    const menu = screen.getByRole('listbox', { name: 'Settings host' })
    expect(menu.textContent).toContain('Settings of')
    const options = within(menu).getAllByRole('option')
    expect(options.map((option) => option.textContent)).toEqual([
      'This PCconnected',
      'buildboxconnected',
      'mac-airconnected',
      'lab-piout of reach'
    ])
    expect(options.map((option) => option.getAttribute('aria-selected'))).toEqual(['false', 'true', 'false', 'false'])
    // In Settings > Hosts' order with This PC first; a host on another computer's name in mono, out of reach in danger.
    expect(options[1]?.querySelector('.font-mono')?.textContent).toBe('buildbox')
    expect(options[3]?.querySelector('[data-host-state]')?.getAttribute('data-host-state')).toBe('away')
    expect(within(options[3]!).getByText('out of reach').className).toContain('text-danger')
    // The states are words, in sans as the mockup sets them; host names are machine data, in mono.
    for (const state of within(menu).getAllByText(/^(connected|out of reach)$/)) {
      expect(state.className).not.toContain('font-mono')
    }

    // A host out of reach can be picked: its surface says why it cannot be read.
    await userEvent.click(options[3]!)
    expect(onHostChange).toHaveBeenCalledWith('h3')
    expect(screen.queryByRole('listbox')).toBeNull()
  })

  it('S8: opens with Enter, moves with the arrows, picks with Enter, and Escape leaves the pick as it was', async () => {
    const user = userEvent.setup()
    const onHostChange = vi.fn()
    render(<HostSwitch surface="Agents" hosts={FOUR} host="local" onHostChange={onHostChange} />)
    const button = screen.getByRole('button', { name: 'Agents host' })

    button.focus()
    await user.keyboard('{Enter}')
    const menu = await screen.findByRole('listbox', { name: 'Agents host' })
    await vi.waitFor(() => expect(document.activeElement).toBe(menu))
    await user.keyboard('{Escape}')
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(onHostChange).not.toHaveBeenCalled()
    expect(document.activeElement).toBe(button)

    await user.keyboard(' ')
    const again = await screen.findByRole('listbox', { name: 'Agents host' })
    await vi.waitFor(() => expect(document.activeElement).toBe(again))
    await user.keyboard('{ArrowDown}{ArrowDown}{Enter}')
    expect(onHostChange).toHaveBeenCalledWith('h2')
    expect(screen.queryByRole('listbox')).toBeNull()
  })

  it('S8: is a menu when two names do not fit beside its label, and segmented again when they do', () => {
    const observers: (() => void)[] = []
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(callback: () => void) {
          observers.push(callback)
        }
        observe(): void {}
        unobserve(): void {}
        disconnect(): void {}
      }
    )
    const long: HostChoiceHost[] = [
      { id: 'local', name: 'This PC', remote: false },
      { id: 'h1', name: 'staging-eu-west-2.internal.example.com', remote: true }
    ]
    layOut(320, 180)
    const view = render(<HostSwitch surface="Settings" hosts={long} host="h1" onHostChange={vi.fn()} />)
    const button = screen.getByRole('button', { name: 'Settings host' })
    // Cut on the button, whole in the menu.
    expect(button.querySelector('.truncate')?.textContent).toBe('staging-eu-west-2.internal.example.com')
    expect(button.getAttribute('title')).toBe('Showing staging-eu-west-2.internal.example.com’s settings')

    // The sidebar widens: the switch is told its new size and measures again.
    vi.restoreAllMocks()
    layOut(320, 320)
    act(() => {
      for (const resized of observers) resized()
    })
    expect(screen.getByRole('radiogroup', { name: 'Settings host' })).toBeTruthy()
    view.unmount()
  })

  it('S8: shows the picked host out of reach on the button', () => {
    render(<HostSwitch surface="Settings" hosts={FOUR} host="h3" onHostChange={vi.fn()} />)
    const button = screen.getByRole('button', { name: 'Settings host' })
    expect(button.textContent).toBe('lab-pi')
    expect(button.querySelector('[data-host-state]')?.getAttribute('data-host-state')).toBe('away')
  })
})

describe('Files root picker on more than one host', () => {
  it('lists one host’s folders and says which host, with a switch', async () => {
    const onHostChange = vi.fn()
    render(
      <FilesRootPicker
        roots={[{ kind: 'project', path: '/home/you/work/infra', label: 'infra' }]}
        value="/home/you/work/infra"
        onChange={vi.fn()}
        hostChoice={choice('h1', onHostChange)}
      />
    )
    const group = screen.getByRole('radiogroup', { name: 'Files host' })
    expect(within(group).getByRole('radio', { checked: true })?.textContent).toContain('build-box')
    expect(screen.getByRole('button', { name: 'Project' })?.textContent).toContain('infra')
    await userEvent.click(within(group).getByRole('radio', { name: 'This PC' }))
    expect(onHostChange).toHaveBeenCalledWith('local')
  })

  it('has no switch with only this PC', () => {
    render(<FilesRootPicker roots={[]} value={null} onChange={vi.fn()} />)
    expect(screen.queryByRole('radiogroup')).toBeNull()
  })

  it('gets the menu too, with four hosts', () => {
    render(
      <FilesRootPicker
        roots={[]}
        value={null}
        onChange={vi.fn()}
        hostChoice={{ hosts: FOUR, host: 'h2', onHostChange: vi.fn() }}
      />
    )
    expect(screen.getByRole('button', { name: 'Files host' }).textContent).toBe('mac-air')
  })
})

describe('Settings > Plugins on a host', () => {
  it('has no switch of its own: it names the host Settings’ Floti host group shows', () => {
    render(<PluginsPage plugins={[]} addError={null} onAdd={vi.fn()} onOpen={vi.fn()} hostName="build-box" />)
    expect(screen.queryByRole('radiogroup')).toBeNull()
    expect(document.querySelector('[data-settings-host-line="build-box"]')?.textContent).toBe(
      'On build-box. Every window attached to it shows the same values.'
    )
  })

  it('adds a folder on a host on another computer by its typed path, not this PC’s picker', async () => {
    const onAdd = vi.fn()
    render(
      <PluginsPage
        plugins={[]}
        addError={null}
        onAdd={onAdd}
        onOpen={vi.fn()}
        hostName="build-box"
        remoteHost="build-box"
      />
    )
    await userEvent.click(screen.getByRole('button', { name: /Add folder/ }))
    expect(onAdd).not.toHaveBeenCalled()
    await userEvent.type(screen.getByRole('textbox', { name: 'Plugin folder on build-box' }), '/srv/plugins/trackr{Enter}')
    expect(onAdd).toHaveBeenCalledWith('/srv/plugins/trackr')
    expect(screen.queryByRole('textbox')).toBeNull()
  })

  it('picks the folder on this PC with only this PC', async () => {
    const onAdd = vi.fn()
    render(<PluginsPage plugins={[]} addError={null} onAdd={onAdd} onOpen={vi.fn()} />)
    expect(screen.queryByRole('radiogroup')).toBeNull()
    await userEvent.click(screen.getByRole('button', { name: /Add folder/ }))
    expect(onAdd).toHaveBeenCalledWith()
  })
})
