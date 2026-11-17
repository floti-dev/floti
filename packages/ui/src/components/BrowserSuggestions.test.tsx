import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { BrowserSuggestion } from '@floti/core/types'
import { BrowserSuggestions } from './BrowserSuggestions'

const ROWS: BrowserSuggestion[] = [
  { kind: 'search', text: 'rust docs', engine: 'Google' },
  { kind: 'page', url: 'https://docs.rs/', title: 'Docs.rs' },
  { kind: 'page', url: 'http://localhost:3000/', title: '' }
]

afterEach(cleanup)

const draw = (highlighted = -1) => {
  const onPick = vi.fn()
  const onHover = vi.fn()
  render(<BrowserSuggestions id="suggest" rows={ROWS} highlighted={highlighted} onPick={onPick} onHover={onHover} />)
  return { onPick, onHover }
}

describe('BrowserSuggestions', () => {
  it('draws the Search row, then each page by title and address, or address alone without a title', () => {
    draw()
    const options = screen.getAllByRole('option')
    expect(options.map((option) => option.textContent)).toEqual([
      'Search Google forrust docs',
      'Docs.rshttps://docs.rs/',
      'http://localhost:3000/'
    ])
    expect(options.map((option) => option.id)).toEqual(['suggest-0', 'suggest-1', 'suggest-2'])
  })

  it('marks the highlighted row selected and no other', () => {
    draw(1)
    expect(screen.getAllByRole('option').map((option) => option.getAttribute('aria-selected'))).toEqual([
      'false',
      'true',
      'false'
    ])
  })

  it('says which row the pointer chose on the press, keeping the caret where it is', () => {
    const { onPick } = draw()
    const pressed = fireEvent.mouseDown(screen.getAllByRole('option')[2]!, { button: 0 })
    expect(onPick).toHaveBeenCalledWith(2)
    // The default prevented: focus stays in the address field.
    expect(pressed).toBe(false)
  })

  it('ignores a press with any button but the main one', () => {
    const { onPick } = draw()
    fireEvent.mouseDown(screen.getAllByRole('option')[1]!, { button: 2 })
    expect(onPick).not.toHaveBeenCalled()
  })

  it('says which row the pointer moved onto, and nothing for the one already highlighted', () => {
    const { onHover } = draw(0)
    fireEvent.mouseMove(screen.getAllByRole('option')[0]!)
    fireEvent.mouseMove(screen.getAllByRole('option')[1]!)
    expect(onHover.mock.calls).toEqual([[1]])
  })

  it('draws nothing with no rows', () => {
    const { container } = render(
      <BrowserSuggestions id="suggest" rows={[]} highlighted={-1} onPick={() => undefined} onHover={() => undefined} />
    )
    expect(container.firstChild).toBeNull()
  })
})
