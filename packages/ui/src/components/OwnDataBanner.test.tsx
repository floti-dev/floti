import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { OwnDataBanner } from './OwnDataBanner'

describe('OwnDataBanner', () => {
  it('says the copy did not finish and will be tried again at the next start, then why, and Dismiss puts it away', () => {
    const onDismiss = vi.fn()
    render(<OwnDataBanner reason="the local host refused a batch of conversations (failed)." onDismiss={onDismiss} />)

    const banner = screen.getByRole('status')
    expect(banner.textContent).toContain(
      'Copying Helm’s history, usage and config versions into the local host did not finish. It will be tried again when Helm next starts.'
    )
    // The reason follows in brackets, without its own full stop.
    expect(banner.textContent).toContain('(the local host refused a batch of conversations (failed))')

    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }))
    expect(onDismiss).toHaveBeenCalledTimes(1)
  })
})
