import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { overlayOpen } from '../lib/overlay'
import { UpgradeNoteDialog, type UpgradeNoteDialogProps } from './UpgradeNoteDialog'

function renderNote(): UpgradeNoteDialogProps & { unmount: () => void } {
  const props: UpgradeNoteDialogProps = { onClose: vi.fn(), onWhatsNew: vi.fn() }
  const { unmount } = render(<UpgradeNoteDialog {...props} />)
  return { ...props, unmount }
}

const note = (): HTMLElement => screen.getByRole('dialog', { name: 'Helm is now Floti' })

describe('UpgradeNoteDialog', () => {
  it('says harnesses are now called workspaces and that nothing in the folders changed', () => {
    renderNote()
    const item = note().querySelector('[data-upgrade-note-item="workspaces"]')
    expect(item?.textContent).toBe(
      'Harnesses are now called workspaces. Nothing in your folders changed: a folder with harness.yaml works as before, and new ones get workspace.yaml.'
    )
  })

  it('says what came with the update and where quitting is chosen', () => {
    renderNote()
    expect(note().querySelector('[data-upgrade-note-item="kept"]')?.textContent).toBe(
      'Everything came with you: sessions, history, profiles, templates, plugins and settings.'
    )
    expect(note().querySelector('[data-upgrade-note-item="host"]')?.textContent).toBe(
      'Sessions run in the host now. Quitting still ends the ones you started here; Settings > Sessions can keep them running instead.'
    )
  })

  it('holds Got it in focus, so Enter closes it', async () => {
    const props = renderNote()
    expect(document.activeElement).toBe(within(note()).getByRole('button', { name: 'Got it' }))
    await userEvent.keyboard('{Enter}')
    expect(props.onClose).toHaveBeenCalledTimes(1)
    expect(props.onWhatsNew).not.toHaveBeenCalled()
  })

  it('closes on Escape as on Got it', async () => {
    const props = renderNote()
    await userEvent.keyboard('{Escape}')
    expect(props.onClose).toHaveBeenCalledTimes(1)
  })

  it('opens the release notes only from What’s new', async () => {
    const props = renderNote()
    await userEvent.click(within(note()).getByRole('button', { name: 'What’s new' }))
    expect(props.onWhatsNew).toHaveBeenCalledTimes(1)
    expect(props.onClose).not.toHaveBeenCalled()
  })

  it('stands a browser page down while it is open, and lets it back once it closes', () => {
    const { unmount } = renderNote()
    expect(overlayOpen()).toBe(true)
    unmount()
    expect(overlayOpen()).toBe(false)
  })
})
