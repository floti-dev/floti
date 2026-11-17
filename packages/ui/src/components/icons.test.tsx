import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { FLOTI_MARK_PATH } from './flotiMark'
import { FlotiMarkIcon } from './icons'

describe('FlotiMarkIcon', () => {
  it('is the mark as one path, filled with the colour of the text around it, on the art cropped square', () => {
    const { container } = render(<FlotiMarkIcon />)
    const svg = container.querySelector('svg')!

    expect(svg.getAttribute('viewBox')).toBe('31.3 32 226.7 226.7')
    expect(svg.getAttribute('fill')).toBe('currentColor')
    expect(svg.getAttribute('stroke')).toBeNull()
    const paths = svg.querySelectorAll('path')
    expect(paths).toHaveLength(1)
    expect(paths[0]!.getAttribute('d')).toBe(FLOTI_MARK_PATH)
    expect(paths[0]!.getAttribute('fill')).toBeNull()
  })

  it('is hidden from assistive technology unless it is labelled', () => {
    const { container } = render(<FlotiMarkIcon />)

    expect(container.querySelector('svg')!.getAttribute('aria-hidden')).toBe('true')
    expect(screen.queryByRole('img')).toBeNull()
  })

  it('is announced by its label where nothing beside it names Floti', () => {
    render(<FlotiMarkIcon role="img" aria-label="Floti" aria-hidden={false} width={20} height={20} />)

    const mark = screen.getByRole('img', { name: 'Floti' })
    expect(mark.getAttribute('width')).toBe('20')
    expect(mark.getAttribute('height')).toBe('20')
  })
})
