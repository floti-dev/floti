import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { BrowserOff } from './BrowserOff'

describe('BrowserOff', () => {
  it('shows the command that turns the sandbox on, and copies it', async () => {
    const onCopy = vi.fn(() => Promise.resolve(true))
    const { container } = render(<BrowserOff command="sudo true" onCopy={onCopy} onReadMore={() => undefined} />)

    expect(container.querySelector('[data-empty-state="browser-off"]')).not.toBeNull()
    expect(screen.getByText('The browser is off')).toBeTruthy()
    expect(screen.getByText('sudo true').tagName).toBe('CODE')
    await userEvent.setup().click(screen.getByRole('button', { name: 'Copy command' }))
    expect(onCopy).toHaveBeenCalledWith('sudo true')
    expect(await screen.findByRole('button', { name: 'Copied' })).toBeTruthy()
  })

  it('says how to turn it on without a command where Helm could not write one', async () => {
    const onReadMore = vi.fn()
    const { container } = render(<BrowserOff command={null} onCopy={() => Promise.resolve(true)} onReadMore={onReadMore} />)

    expect(container.querySelector('code')).toBeNull()
    expect(screen.queryByRole('button', { name: 'Copy command' })).toBeNull()
    expect(screen.getByText(/Start it with the sandbox/)).toBeTruthy()
    await userEvent.setup().click(screen.getByRole('button', { name: 'How this works' }))
    expect(onReadMore).toHaveBeenCalledOnce()
  })
})
