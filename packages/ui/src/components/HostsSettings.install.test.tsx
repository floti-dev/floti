import type { HostCheck, HostSystem, InstallOffer, InstallOutcome } from '@helm/core/types'
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { HostsSettings, type HostInstallActions, type HostsSettingsProps, type SettingsHost } from './HostsSettings'

const NOT_FOUND_OFFER: InstallOffer = {
  system: { os: 'linux', cpu: 'x64', label: 'Linux · x86_64' },
  found: null,
  action: 'install',
  how: 'copy',
  version: '2.6.0',
  folder: '~/.local/bin',
  sessions: null,
  talks: false
}

function actions(check: HostCheck, outcome: InstallOutcome = { ok: true, floti: '/home/me/.local/bin/floti', startsAtNextSignIn: false }) {
  return {
    check: vi.fn<HostInstallActions['check']>(async () => check),
    install: vi.fn<HostInstallActions['install']>(async () => outcome),
    cancel: vi.fn<HostInstallActions['cancel']>(async () => undefined)
  }
}

function renderHosts(install: HostInstallActions, overrides: Partial<HostsSettingsProps> = {}) {
  const onInstalled = vi.fn<(id: string, floti: string, system: HostSystem) => void>()
  const props = (next: Partial<HostsSettingsProps>): HostsSettingsProps => ({
    hosts: [],
    onAdd: vi.fn(),
    onRemove: vi.fn(),
    onQuitChange: vi.fn(),
    install,
    onInstalled,
    ...overrides,
    ...next
  })
  const view = render(<HostsSettings {...props({})} />)
  return { onInstalled, rerender: (next: Partial<HostsSettingsProps>) => view.rerender(<HostsSettings {...props(next)} />) }
}

const host = (connection: SettingsHost['connection'], floti?: string): SettingsHost => ({
  host: { id: 'h-build', name: 'build-box', transport: { kind: 'ssh', target: 'me@build-box' }, ...(floti ? { floti } : {}) },
  quit: 'keep',
  connection
})

describe('installing floti from a host’s row in Settings > Hosts', () => {
  it('S13, S14: an update names the version found and the sessions it ends, on the button that does it', async () => {
    const install = actions({
      kind: 'offer',
      id: 'offer-2',
      offer: { ...NOT_FOUND_OFFER, action: 'update', found: { version: '2.1.0', protocol: '0.9' }, sessions: 2 }
    })
    const { onInstalled } = renderHosts(install, { hosts: [host({ state: 'tooOld', host: '0.9', client: '1.0' }, '/home/me/.local/bin/floti')] })
    const row = screen.getByRole('group', { name: 'build-box' })
    expect(row.querySelector('[data-settings-host-too-old]')?.textContent).toContain('protocol 0.9')
    await userEvent.click(within(row).getByRole('button', { name: 'Update floti' }))
    expect(install.check).toHaveBeenCalledWith({ host: 'h-build' })
    const panel = (await screen.findByText('Update floti on build-box?')).closest('[data-settings-host-offer]') as HTMLElement
    expect(panel.querySelector('[data-settings-host-offer-found]')?.textContent).toContain('2.1.0')
    expect(panel.textContent).toContain('ending 2 sessions')
    await userEvent.click(within(panel).getByRole('button', { name: 'Update floti on build-box, ending 2 sessions' }))
    await vi.waitFor(() => expect(onInstalled).toHaveBeenCalledWith('h-build', '/home/me/.local/bin/floti', { os: 'Linux', cpu: 'x86_64' }))
  })

  it('S18, S19, S17: names a missing floti, one too old and one newer with both versions, and one that starts at the next sign-in', () => {
    renderHosts(actions({ kind: 'ready', found: { version: '2.6.0', protocol: '1.0' }, system: { os: 'linux' as const, cpu: 'x64', label: 'Linux · x86_64' } }), {
      hosts: [
        { ...host({ state: 'missing', lastLine: 'sh: 1: floti: not found' }), host: { id: 'a', name: 'a', transport: { kind: 'ssh', target: 'a' } } },
        { ...host({ state: 'tooOld', host: '0.9', client: '1.0' }), host: { id: 'b', name: 'b', transport: { kind: 'ssh', target: 'b' } } },
        { ...host({ state: 'newer', host: '2.0', client: '1.0' }), host: { id: 'c', name: 'c', transport: { kind: 'ssh', target: 'c' } } },
        { ...host({ state: 'startsAtSignIn' }), host: { id: 'd', name: 'd', transport: { kind: 'ssh', target: 'd' } } }
      ]
    })
    const missing = screen.getByRole('group', { name: 'a' })
    expect(missing.textContent).toContain('floti not found')
    expect(within(missing).getByText('sh: 1: floti: not found')).toBeTruthy()
    expect(within(missing).getByRole('button', { name: 'Install floti' })).toBeTruthy()

    const old = screen.getByRole('group', { name: 'b' })
    expect(old.textContent).toContain('floti too old')
    expect(within(old).getByRole('button', { name: 'Update floti' })).toBeTruthy()

    const newer = screen.getByRole('group', { name: 'c' })
    expect(newer.querySelector('[data-settings-host-newer]')?.textContent).toContain(
      'it speaks protocol 2.0, and this Helm speaks 1.0. Update Helm on this PC, then it connects.'
    )
    expect(within(newer).queryByRole('button', { name: 'Update floti' })).toBeNull()

    const later = screen.getByRole('group', { name: 'd' })
    expect(later.textContent).toContain('Starts at next sign-in')
    expect(later.querySelector('[data-settings-host-starts-at-sign-in]')).not.toBeNull()
    expect(later.textContent).not.toContain('Cannot connect')
  })

  it('S18: the sidebar’s Update floti starts the same update in the host’s row', async () => {
    const install = actions({ kind: 'offer', id: 'offer-3', offer: { ...NOT_FOUND_OFFER, action: 'update', found: { version: '2.1.0', protocol: '0.9' } } })
    const { rerender } = renderHosts(install, { hosts: [host({ state: 'tooOld', host: '0.9', client: '1.0' })] })
    expect(install.check).not.toHaveBeenCalled()
    rerender({ hosts: [host({ state: 'tooOld', host: '0.9', client: '1.0' })], updateRequest: { id: 'h-build', nonce: 1 } })
    expect(await screen.findByText('Update floti on build-box?')).toBeTruthy()
    expect(install.check).toHaveBeenCalledWith({ host: 'h-build' })
  })
})

describe('updating an older floti from a connected host’s row in Settings > Hosts', () => {
  const OLDER_OFFER: InstallOffer = {
    ...NOT_FOUND_OFFER,
    action: 'update',
    how: 'flotiUpdate',
    via: 'connection',
    found: { version: '2.5.0', protocol: '3.1' },
    sessions: 0,
    talks: true
  }

  it('names both versions on a connected host whose floti is older, offers Update floti, and says floti update ends no session', async () => {
    const install = actions({ kind: 'offer', id: 'offer-3', offer: OLDER_OFFER }, { ok: true, startsAtNextSignIn: false })
    const { onInstalled } = renderHosts(install, {
      hosts: [host({ state: 'connected', version: '2.5.0', olderThan: '2.6.0' })]
    })
    const row = screen.getByRole('group', { name: 'build-box' })
    expect(within(row).getByText('Older floti')).toBeTruthy()
    expect(row.querySelector('[data-settings-host-older]')?.textContent).toBe(
      'build-box runs floti 2.5.0, older than the floti 2.6.0 this Helm carries. Update floti there says first what it ends.'
    )
    await userEvent.click(within(row).getByRole('button', { name: 'Update floti' }))
    expect(install.check).toHaveBeenCalledWith({ host: 'h-build' })
    const panel = (await screen.findByText('Update floti on build-box?')).closest('[data-settings-host-offer]') as HTMLElement
    expect(panel.textContent).toContain('build-box has floti 2.5.0, older than the floti 2.6.0 this Helm carries.')
    expect(panel.textContent).toContain('Helm can run floti update there now')
    const steps = panel.querySelector('[data-settings-host-offer-steps]')?.textContent ?? ''
    expect(steps).toContain('Asks the floti host on build-box to run floti update')
    expect(panel.querySelector('[data-settings-host-offer-sessions]')?.textContent).toContain('no session ends')
    // Nothing about stopping the host, copying a build or running setup.
    expect(steps).not.toContain('Stops the floti host')
    expect(steps).not.toContain('floti setup')
    expect(install.install).not.toHaveBeenCalled()

    await userEvent.click(within(panel).getByRole('button', { name: 'Update floti on build-box' }))
    expect(install.install).toHaveBeenCalledWith('offer-3', 'h-build')
    // floti update replaced floti where it was: no new path to keep.
    await vi.waitFor(() => expect(row.querySelector('[data-settings-host-offer]')).toBeNull())
    expect(onInstalled).not.toHaveBeenCalled()
  })

  it('S13: a floti update whose new host cannot take the keeper back names the sessions it ends, on its button too', async () => {
    const install = actions({
      kind: 'offer',
      id: 'offer-4',
      offer: { ...OLDER_OFFER, via: 'login', talks: false, found: { version: '2.5.0', protocol: '3.1' }, sessions: 3 }
    })
    renderHosts(install, { hosts: [host({ state: 'tooOld', host: '3.1', client: '3.2' })] })
    const row = screen.getByRole('group', { name: 'build-box' })
    await userEvent.click(within(row).getByRole('button', { name: 'Update floti' }))
    const panel = (await screen.findByText('Update floti on build-box?')).closest('[data-settings-host-offer]') as HTMLElement
    expect(panel.textContent).toContain('too old for this Helm to talk to')
    expect(panel.querySelector('[data-settings-host-offer-steps]')?.textContent).toContain(
      'Runs floti update on build-box over the same login'
    )
    expect(panel.querySelector('[data-settings-host-offer-sessions]')?.textContent).toContain('ending 3 sessions')
    expect(within(panel).getByRole('button', { name: 'Update floti on build-box, ending 3 sessions' })).toBeTruthy()
  })

  it('a floti update that failed there shows its last line with the fix', async () => {
    const install = actions(
      { kind: 'offer', id: 'offer-5', offer: OLDER_OFFER },
      { ok: false, step: 'update', lastLine: 'floti-2.6.0-linux-x64 does not match its signature.' }
    )
    renderHosts(install, { hosts: [host({ state: 'connected', version: '2.5.0', olderThan: '2.6.0' })] })
    const row = screen.getByRole('group', { name: 'build-box' })
    await userEvent.click(within(row).getByRole('button', { name: 'Update floti' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Update floti on build-box' }))
    const problem = await screen.findByRole('alert')
    expect(problem.textContent).toContain('floti update failed on build-box')
    expect(problem.querySelector('[data-settings-host-problem-line]')?.textContent).toBe(
      'floti-2.6.0-linux-x64 does not match its signature.'
    )
  })

  it('a connected host on this Helm’s floti, or one that names no version, offers nothing', () => {
    renderHosts(actions({ kind: 'ready', found: { version: '2.6.0', protocol: '3.1' }, system: NOT_FOUND_OFFER.system }), {
      hosts: [host({ state: 'connected', version: '2.6.0' })]
    })
    const row = screen.getByRole('group', { name: 'build-box' })
    expect(within(row).queryByRole('button', { name: 'Update floti' })).toBeNull()
    expect(row.querySelector('[data-settings-host-older]')).toBeNull()
    expect(within(row).getByText('Connected')).toBeTruthy()
  })
})
