import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { overlayOpen } from '../lib/overlay'
import { DeleteProfileDialog, type DeleteProfileDialogProps } from './DeleteProfileDialog'

function renderDialog(): DeleteProfileDialogProps & { unmount: () => void } {
  const props: DeleteProfileDialogProps = { name: 'hub dev', onConfirm: vi.fn(), onCancel: vi.fn() }
  const { unmount } = render(<DeleteProfileDialog {...props} />)
  return { ...props, unmount }
}

const dialog = (): HTMLElement => screen.getByRole('alertdialog', { name: 'Delete “hub dev”?' })

describe('DeleteProfileDialog', () => {
  it('asks under the profile’s name, says it cannot be undone and offers the export', () => {
    renderDialog()
    expect(within(dialog()).getByRole('heading').textContent).toBe('Delete “hub dev”?')
    expect(dialog().textContent).toContain(
      'This cannot be undone. Sessions already launched from it keep running, and exporting it first writes a YAML file you can import back.'
    )
  })

  it('holds Cancel in focus, so Enter deletes nothing', async () => {
    const props = renderDialog()
    expect(document.activeElement).toBe(within(dialog()).getByRole('button', { name: 'Cancel' }))
    await userEvent.keyboard('{Enter}')
    expect(props.onCancel).toHaveBeenCalledTimes(1)
    expect(props.onConfirm).not.toHaveBeenCalled()
  })

  it('means no on Escape and on Cancel', async () => {
    const props = renderDialog()
    await userEvent.keyboard('{Escape}')
    await userEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(props.onCancel).toHaveBeenCalledTimes(2)
    expect(props.onConfirm).not.toHaveBeenCalled()
  })

  it('deletes only from its own button', async () => {
    const props = renderDialog()
    await userEvent.click(screen.getByRole('button', { name: 'Delete profile' }))
    expect(props.onConfirm).toHaveBeenCalledTimes(1)
    expect(props.onCancel).not.toHaveBeenCalled()
  })

  it('stands a browser page down while it is open, and lets it back once it closes', () => {
    const { unmount } = renderDialog()
    expect(overlayOpen()).toBe(true)
    unmount()
    expect(overlayOpen()).toBe(false)
  })
})
