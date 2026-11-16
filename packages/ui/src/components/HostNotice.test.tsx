import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { HostNotice, type HostNoticeState } from './HostNotice'

const HOST = 'this PC’s host'

function show(notice: HostNoticeState) {
  const onTryAgain = vi.fn()
  const onUpdateFloti = vi.fn()
  render(<HostNotice hostName={HOST} notice={notice} onTryAgain={onTryAgain} onUpdateFloti={onUpdateFloti} />)
  return { onTryAgain, onUpdateFloti, button: screen.queryByRole('button') }
}

describe('HostNotice', () => {
  it('says a host that refused this Floti once, in the sentence Floti gave, with Try again', () => {
    const sentence =
      'This PC’s host speaks Floti protocol 3.1, older than this Floti’s 4.1. Floti restarted it onto floti 3.0.0, and the host it started refused this Floti too.'
    const { onTryAgain, onUpdateFloti, button } = show({ kind: 'refused', sentence, fix: 'try-again', retrying: false })
    expect(screen.getByRole('status').getAttribute('data-host-notice')).toBe('refused')
    expect(screen.getByRole('status').textContent).toBe(`${sentence}Try again`)
    fireEvent.click(button!)
    expect(onTryAgain).toHaveBeenCalledTimes(1)
    expect(onUpdateFloti).not.toHaveBeenCalled()
  })

  it('offers updating Floti, not Try again, when the host is the newer side', () => {
    const { onTryAgain, onUpdateFloti, button } = show({
      kind: 'refused',
      sentence: 'This client speaks Floti protocol 3.1 and the host speaks 4.1. Update the client.',
      fix: 'update-floti',
      retrying: false
    })
    expect(button?.textContent).toBe('Update Floti…')
    fireEvent.click(button!)
    expect(onUpdateFloti).toHaveBeenCalledTimes(1)
    expect(onTryAgain).not.toHaveBeenCalled()
  })

  it('says a host that could not be reached, with why and Try again', () => {
    const { onTryAgain, button } = show({
      kind: 'unreachable',
      reason: 'floti host did not answer within 30 seconds.',
      retrying: false
    })
    expect(screen.getByRole('status').getAttribute('data-host-notice')).toBe('unreachable')
    expect(screen.getByRole('status').textContent).toBe(
      'Floti could not reach this PC’s host. floti host did not answer within 30 seconds.Try again'
    )
    fireEvent.click(button!)
    expect(onTryAgain).toHaveBeenCalledTimes(1)
  })

  it('says it is trying again while Floti tries by itself, and Try again waits for that try', () => {
    const { onTryAgain, button } = show({ kind: 'unreachable', reason: 'No host answered.', retrying: true })
    expect(button?.textContent).toBe('Trying again…')
    expect(button).toHaveProperty('disabled', true)
    fireEvent.click(button!)
    expect(onTryAgain).not.toHaveBeenCalled()
  })

  it('says the host is being updated, with nothing to press', () => {
    const { button } = show({ kind: 'updating', to: '3.0.0-preview.2' })
    expect(screen.getByRole('status').getAttribute('data-host-notice')).toBe('updating')
    expect(screen.getByRole('status').textContent).toBe(
      'Updating this PC’s host to floti 3.0.0-preview.2… Its sessions carry over.'
    )
    expect(button).toBeNull()
  })

  it('says this PC’s host stopped and that Floti is starting it again, when Floti starts it', () => {
    const { button } = show({ kind: 'lost', restarts: true })
    expect(screen.getByRole('status').textContent).toBe('This PC’s host stopped. Starting it again…')
    expect(button).toBeNull()
  })

  it('says Floti is reconnecting, when the host is not one Floti starts', () => {
    show({ kind: 'lost', restarts: false })
    expect(screen.getByRole('status').textContent).toBe('Reconnecting to this PC’s host…')
  })
})
