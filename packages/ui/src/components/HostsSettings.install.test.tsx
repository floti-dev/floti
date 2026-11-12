import type { HostCheck, HostSystem, InstallOffer, InstallOutcome } from '@helm/core/types'
import { act, render, screen, within } from '@testing-library/react'
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
    // An install, which ends the sessions there, says first what it does.
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
    updatesTo: '2.6.0',
    found: { version: '2.5.0', protocol: '3.1' },
    sessions: 0,
    talks: true
  }

  /** Install actions whose install never ends, to see the update running. */
  function neverEnding(offer: InstallOffer) {
    return {
      check: vi.fn<HostInstallActions['check']>(async () => ({ kind: 'offer', id: 'offer-3', offer })),
      install: vi.fn<HostInstallActions['install']>(() => new Promise<InstallOutcome>(() => undefined)),
      cancel: vi.fn<HostInstallActions['cancel']>()
    }
  }

  it('names both versions on a connected host whose floti is older, and Update floti runs floti update at once', async () => {
    const install = actions({ kind: 'offer', id: 'offer-3', offer: OLDER_OFFER }, { ok: true, startsAtNextSignIn: false })
    const { onInstalled } = renderHosts(install, {
      hosts: [host({ state: 'connected', version: '2.5.0', olderThan: '2.6.0' })]
    })
    const row = screen.getByRole('group', { name: 'build-box' })
    expect(within(row).getByText('Older floti')).toBeTruthy()
    expect(row.querySelector('[data-settings-host-older]')?.textContent).toBe(
      'build-box runs floti 2.5.0, older than the floti 2.6.0 this Helm carries.'
    )
    await userEvent.click(within(row).getByRole('button', { name: 'Update floti' }))
    expect(install.check).toHaveBeenCalledWith({ host: 'h-build' })
    // No question first: the update starts as the check answers.
    await vi.waitFor(() => expect(install.install).toHaveBeenCalledWith('offer-3', 'h-build'))
    expect(screen.queryByText('Update floti on build-box?')).toBeNull()
    expect(row.querySelector('[data-settings-host-offer]')).toBeNull()
    // floti update replaced floti where it was: no new path to keep.
    await vi.waitFor(() => expect(row.querySelector('[data-settings-host-working]')).toBeNull())
    expect(onInstalled).not.toHaveBeenCalled()
  })

  it('the sidebar’s Update floti runs floti update at once too', async () => {
    const install = neverEnding(OLDER_OFFER)
    const hosts = [host({ state: 'connected', version: '2.5.0', olderThan: '2.6.0' })]
    const { rerender } = renderHosts(install, { hosts })
    rerender({ hosts, updateRequest: { id: 'h-build', nonce: 1 } })
    expect(await screen.findByText('Updating floti on build-box from 2.5.0 to 2.6.0, the newest on its channel…')).toBeTruthy()
    expect(install.install).toHaveBeenCalledWith('offer-3', 'h-build')
    expect(screen.queryByText('Update floti on build-box?')).toBeNull()
  })

  it('an older floti’s sentence comes back once a failed update is put away', async () => {
    renderHosts(
      actions({ kind: 'offer', id: 'offer-3', offer: OLDER_OFFER }, { ok: false, step: 'update', lastLine: 'no network' }),
      { hosts: [host({ state: 'connected', version: '2.5.0', olderThan: '2.6.0' })] }
    )
    const row = screen.getByRole('group', { name: 'build-box' })
    await userEvent.click(within(row).getByRole('button', { name: 'Update floti' }))
    await userEvent.click(await within(row).findByRole('button', { name: 'Back' }))
    expect(row.querySelector('[data-settings-host-older]')?.textContent).toContain('build-box runs floti 2.5.0')
  })

  it('a floti update over the login that went well records no path, so the link tries to connect at once', async () => {
    const install = actions(
      { kind: 'offer', id: 'offer-6', offer: { ...OLDER_OFFER, via: 'login', updatesTo: null, talks: false } },
      { ok: true, startsAtNextSignIn: false }
    )
    const onReconnect = vi.fn<(id: string) => void>()
    const { onInstalled } = renderHosts(install, {
      hosts: [host({ state: 'tooOld', host: '3.1', client: '3.2' })],
      onReconnect
    })
    const row = screen.getByRole('group', { name: 'build-box' })
    await userEvent.click(within(row).getByRole('button', { name: 'Update floti' }))
    await vi.waitFor(() => expect(onReconnect).toHaveBeenCalledWith('h-build'))
    expect(install.install).toHaveBeenCalledWith('offer-6', 'h-build')
    expect(onInstalled).not.toHaveBeenCalled()
  })

  it('S13: a floti update whose new host cannot take the keeper back names the sessions it ends on its first line', async () => {
    const install = neverEnding({ ...OLDER_OFFER, via: 'login', updatesTo: null, talks: false, sessions: 3 })
    renderHosts(install, { hosts: [host({ state: 'tooOld', host: '3.1', client: '3.2' })] })
    const row = screen.getByRole('group', { name: 'build-box' })
    await userEvent.click(within(row).getByRole('button', { name: 'Update floti' }))
    const first = await within(row).findByText(
      'Updating floti on build-box from 2.5.0 to the newest on its channel, ending 3 sessions, which are offered back when it starts…'
    )
    expect(first.closest('[data-settings-host-working]')).toBeTruthy()
    expect(install.install).toHaveBeenCalledWith('offer-3', 'h-build')
  })

  it('a floti update that failed there shows its last line with the fix', async () => {
    const install = actions(
      { kind: 'offer', id: 'offer-5', offer: OLDER_OFFER },
      { ok: false, step: 'update', lastLine: 'floti-2.6.0-linux-x64 does not match its signature.' }
    )
    renderHosts(install, { hosts: [host({ state: 'connected', version: '2.5.0', olderThan: '2.6.0' })] })
    const row = screen.getByRole('group', { name: 'build-box' })
    await userEvent.click(within(row).getByRole('button', { name: 'Update floti' }))
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

describe('a floti update running from a host’s row, and how it ends', () => {
  const UPDATE_OFFER: InstallOffer = {
    ...NOT_FOUND_OFFER,
    action: 'update',
    how: 'flotiUpdate',
    via: 'login',
    updatesTo: null,
    found: { version: '2.5.0', protocol: '3.1' },
    sessions: null,
    talks: true
  }
  /** What the check offers for an install from this Helm's floti instead. */
  const INSTALL_INSTEAD: InstallOffer = { ...NOT_FOUND_OFFER, action: 'update', found: { version: '2.5.0', protocol: '3.1' } }

  /** Install actions whose install ends only when the test says, and whose progress the test tells. */
  function running(offer: InstallOffer) {
    let finish: (outcome: InstallOutcome) => void = () => undefined
    const listeners = new Map<string, (line: string) => void>()
    const install = {
      check: vi.fn<HostInstallActions['check']>(async (target) =>
        'install' in target && target.install === true
          ? { kind: 'offer', id: 'offer-8', offer: INSTALL_INSTEAD }
          : { kind: 'offer', id: 'offer-7', offer }
      ),
      install: vi.fn<HostInstallActions['install']>(
        () =>
          new Promise<InstallOutcome>((resolve) => {
            finish = resolve
          })
      ),
      cancel: vi.fn<HostInstallActions['cancel']>(),
      progress: vi.fn<NonNullable<HostInstallActions['progress']>>((id, listener) => {
        listeners.set(id, listener)
        return () => listeners.delete(id)
      })
    }
    return { install, finish: (outcome: InstallOutcome) => finish(outcome), say: (id: string, line: string) => listeners.get(id)?.(line) }
  }

  async function startUpdate(offer: InstallOffer) {
    const r = running(offer)
    renderHosts(r.install, { hosts: [host({ state: 'connected', version: '2.5.0', olderThan: '2.6.0' })] })
    const row = screen.getByRole('group', { name: 'build-box' })
    await userEvent.click(within(row).getByRole('button', { name: 'Update floti' }))
    const working = (await within(row).findByText(/^Updating floti on build-box/)).closest('[data-settings-host-working]') as HTMLElement
    return { ...r, row, working }
  }

  it('over the login, its first line names the floti it replaces and that it fetches the newest on its channel', async () => {
    const { working, install } = await startUpdate(UPDATE_OFFER)
    expect(working.querySelector('[data-settings-host-working-first]')?.textContent).toBe(
      'Updating floti on build-box from 2.5.0 to the newest on its channel…'
    )
    expect(install.install).toHaveBeenCalledWith('offer-7', 'h-build')
    expect(screen.queryByText('Update floti on build-box?')).toBeNull()
  })

  it('over the connection, its first line names the version it installs', async () => {
    const { working } = await startUpdate({ ...UPDATE_OFFER, via: 'connection', updatesTo: '2.6.0', sessions: 0 })
    expect(working.querySelector('[data-settings-host-working-first]')?.textContent).toBe(
      'Updating floti on build-box from 2.5.0 to 2.6.0, the newest on its channel…'
    )
  })

  it('shows each line the update says while it runs, and Stop waiting stops waiting for it', async () => {
    const r = await startUpdate(UPDATE_OFFER)
    expect(r.install.progress).toHaveBeenCalledWith('offer-7', expect.any(Function))
    act(() => r.say('offer-7', 'Downloading floti-2.6.0-linux-x64.'))
    const line = r.row.querySelector('[data-settings-host-working-line]')
    expect(line?.textContent).toBe('Downloading floti-2.6.0-linux-x64.')
    await userEvent.click(within(r.row).getByRole('button', { name: 'Stop waiting' }))
    expect(r.install.cancel).toHaveBeenCalledWith('offer-7')
    act(() => r.finish({ ok: null, ended: 'stoppedWaiting', lastLine: 'Downloading floti-2.6.0-linux-x64.' }))
    const notice = await screen.findByRole('status')
    expect(notice.textContent).toContain('Helm stopped waiting for floti update on build-box')
    expect(notice.textContent).toContain('It may still be running there.')
    expect(within(notice).queryByRole('button', { name: 'Update from this Helm’s floti' })).toBeNull()
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('an update that found nothing newer is no failure, and an install from this Helm’s floti instead says first what it does', async () => {
    const r = await startUpdate(UPDATE_OFFER)
    act(() => r.finish({ ok: null, ended: 'upToDate', lastLine: 'floti 2.5.0 is up to date on stable (its newest is 2.5.0).' }))
    const notice = await screen.findByRole('status')
    expect(notice.textContent).toContain('floti on build-box is up to date on its channel')
    expect(notice.querySelector('[data-settings-host-notice-line]')?.textContent).toBe(
      'floti 2.5.0 is up to date on stable (its newest is 2.5.0).'
    )
    expect(screen.queryByRole('alert')).toBeNull()
    await userEvent.click(within(notice).getByRole('button', { name: 'Update from this Helm’s floti' }))
    expect(r.install.check).toHaveBeenLastCalledWith({ host: 'h-build', install: true })
    const panel = (await screen.findByText('Update floti on build-box?')).closest('[data-settings-host-offer]') as HTMLElement
    expect(panel).toBeTruthy()
    expect(r.install.install).toHaveBeenCalledTimes(1)
  })
})
