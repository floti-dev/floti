import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { VersionBanner, type VersionBannerProps } from './VersionBanner'

const RANGE = { min: '2.1.0', max: '2.2.0' }

const banner = (over: Partial<VersionBannerProps>): void => {
  render(
    <VersionBanner kind="missing" version={null} range={RANGE} onDismiss={vi.fn()} onLocate={vi.fn()} {...over} />
  )
}

describe('VersionBanner', () => {
  it('offers Locate claude only when claude is missing', () => {
    banner({ kind: 'missing' })
    expect(screen.getByRole('button', { name: 'Locate claude…' })).toBeTruthy()
    expect(screen.getByText(/was not found/)).toBeTruthy()
  })

  it('offers no Locate claude when claude did not answer', () => {
    banner({ kind: 'unanswered', version: null, error: 'claude --version timed out' })
    expect(screen.queryByRole('button', { name: 'Locate claude…' })).toBeNull()
    expect(screen.getByText('claude --version timed out')).toBeTruthy()
  })

  it('offers no Locate claude outside the tested range, and names the range', () => {
    banner({ kind: 'outside-range', version: '9.0.0' })
    expect(screen.queryByRole('button', { name: 'Locate claude…' })).toBeNull()
    expect(screen.getByText(/claude 9\.0\.0 is outside the range.*2\.1\.0 up to 2\.2\.0/)).toBeTruthy()
  })
})
