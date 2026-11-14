import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { HostStoppedBanner } from './HostStoppedBanner'

describe('HostStoppedBanner', () => {
  it('says this PC’s host stopped and that Floti is starting it again, when Floti starts it', () => {
    render(<HostStoppedBanner hostName="this PC’s host" restarting />)
    expect(screen.getByRole('status').textContent).toBe('This PC’s host stopped. Starting it again…')
  })

  it('says Floti is reconnecting, when the host is not one Floti starts', () => {
    render(<HostStoppedBanner hostName="this PC’s host" restarting={false} />)
    expect(screen.getByRole('status').textContent).toBe('Reconnecting to this PC’s host…')
  })
})
