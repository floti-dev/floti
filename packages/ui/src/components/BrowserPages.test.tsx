import type { JSX } from 'react'
import { fireEvent, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { BROWSER_PAGE_MIME, BrowserPages, type BrowserPagesProps } from './BrowserPages'
import { TAB_MIME } from './TabBar'

const PAGES: BrowserPagesProps['pages'] = [
  { id: 1, title: 'Dashboard', url: 'http://localhost:5173/', refusedUrl: null, openedBy: null, sharedWith: null, loading: false, icon: null },
  { id: 2, title: '', url: '', refusedUrl: null, openedBy: null, sharedWith: null, loading: false, icon: null },
  {
    id: 3,
    title: 'Settings',
    url: 'http://localhost:5173/settings',
    refusedUrl: null,
    openedBy: 'browser overhaul',
    sharedWith: null,
    loading: false,
    icon: null
  }
]

function renderPages(overrides: Partial<BrowserPagesProps> = {}): BrowserPagesProps {
  const props: BrowserPagesProps = {
    pages: PAGES,
    activeId: 1,
    focused: true,
    onActivate: vi.fn(),
    onClose: vi.fn(),
    onMove: vi.fn(),
    onNew: vi.fn(),
    ...overrides
  }
  render(<BrowserPages {...props} />)
  return props
}

/** A drag carrying `id` as `type`, for a drop on the strip. */
const carrying = (type: string, id: string): { dataTransfer: unknown } => ({
  dataTransfer: { types: [type, 'text/plain'], getData: (asked: string) => (asked === type ? id : ''), dropEffect: '' }
})

describe('BrowserPages', () => {
  it('names each page by its title, an empty one "New tab", and an agent\'s by the session that opened it', () => {
    renderPages()
    const strip = screen.getByRole('tablist', { name: 'Browser tabs' })
    const tabs = within(strip).getAllByRole('tab')
    expect(tabs.map((tab) => tab.textContent)).toEqual(['Dashboard', 'New tab', 'Settingsbrowser overhaul'])
    expect(tabs[0]!.getAttribute('aria-selected')).toBe('true')
    expect(tabs[2]!.getAttribute('title')).toBe('http://localhost:5173/settings\nOpened by the session “browser overhaul”')
  })

  it('names a page refused its first address after its host, and says on hover that nothing loaded', () => {
    renderPages({
      pages: [
        { id: 5, title: 'example.com', url: '', refusedUrl: 'https://example.com/docs', openedBy: null, sharedWith: null, loading: false, icon: null }
      ]
    })
    const tab = screen.getByRole('tab', { name: /^example\.com/ })
    expect(tab.getAttribute('title')).toBe('https://example.com/docs\nRefused, so nothing has loaded')
  })

  it('says "shared" on a page the user shared with a session, and names the session on hover', () => {
    renderPages({
      pages: [
        {
          id: 4,
          title: 'Account',
          url: 'https://example.com/account',
          refusedUrl: null,
          openedBy: null,
          sharedWith: { session: 9, name: 'browser overhaul' },
          loading: false,
          icon: null
        }
      ]
    })
    const tab = screen.getByRole('tab', { name: /^Account/ })
    expect(tab.textContent).toBe('Accountshared')
    expect(tab.getAttribute('title')).toBe('https://example.com/account\nShared with the session “browser overhaul”')
  })

  it('reports pages by their own ids: front, close, and a new one from the + that opens no menu', async () => {
    const props = renderPages()
    await userEvent.click(screen.getByRole('tab', { name: /^Settings/ }))
    expect(props.onActivate).toHaveBeenCalledWith(3)
    await userEvent.click(screen.getByRole('button', { name: 'Close Dashboard' }))
    expect(props.onClose).toHaveBeenCalledWith(1)

    const plus = screen.getByRole('button', { name: 'New browser tab' })
    expect(plus.hasAttribute('aria-haspopup')).toBe(false)
    await userEvent.click(plus)
    expect(props.onNew).toHaveBeenCalledTimes(1)
  })

  it("moves a page dragged along the strip, and ignores a pane's tab dropped on it", () => {
    const props = renderPages()
    const last = screen.getByRole('tab', { name: /^Settings/ }).parentElement!
    // Dropped on the last tab is "before it", counted once the page has left.
    fireEvent.drop(last, carrying(BROWSER_PAGE_MIME, 'page:1'))
    expect(props.onMove).toHaveBeenCalledWith(1, 1)

    fireEvent.drop(last, carrying(TAB_MIME, 'session:4'))
    expect(props.onMove).toHaveBeenCalledTimes(1)
  })
})

describe('BrowserPages - what a tab shows before its title', () => {
  const ICON = 'data:image/png;base64,aWNvbg=='
  const iconOf = (name: RegExp): string | null =>
    screen.getByRole('tab', { name }).querySelector('[data-page-icon]')?.getAttribute('data-page-icon') ?? null

  it("shows a spinner while the page loads, then its site's icon, and the globe for a site with none", () => {
    const page = PAGES[0]!
    const { rerender } = render(
      <BrowserPages
        pages={[{ ...page, loading: true, icon: ICON }]}
        activeId={1}
        focused
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onMove={vi.fn()}
        onNew={vi.fn()}
      />
    )
    expect(iconOf(/^Dashboard/)).toBe('loading')

    const loaded = (icon: string | null): JSX.Element => (
      <BrowserPages
        pages={[{ ...page, loading: false, icon }]}
        activeId={1}
        focused
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onMove={vi.fn()}
        onNew={vi.fn()}
      />
    )
    rerender(loaded(ICON))
    expect(iconOf(/^Dashboard/)).toBe('favicon')
    expect(screen.getByRole('tab', { name: /^Dashboard/ }).querySelector('img')?.getAttribute('src')).toBe(ICON)

    rerender(loaded(null))
    expect(iconOf(/^Dashboard/)).toBe('globe')
  })

  it('falls back to the globe for an icon the image decoder refuses, and tries a new icon again', () => {
    const page = PAGES[0]!
    const strip = (icon: string): JSX.Element => (
      <BrowserPages
        pages={[{ ...page, icon }]}
        activeId={1}
        focused
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onMove={vi.fn()}
        onNew={vi.fn()}
      />
    )
    const { rerender } = render(strip(ICON))
    fireEvent.error(screen.getByRole('tab', { name: /^Dashboard/ }).querySelector('img')!)
    expect(iconOf(/^Dashboard/)).toBe('globe')

    rerender(strip('data:image/png;base64,b3RoZXI='))
    expect(iconOf(/^Dashboard/)).toBe('favicon')
  })

  it('shows each tab its own state', () => {
    renderPages({
      pages: [
        { ...PAGES[0]!, loading: true },
        { ...PAGES[2]!, icon: ICON }
      ]
    })
    expect(iconOf(/^Dashboard/)).toBe('loading')
    expect(iconOf(/^Settings/)).toBe('favicon')
  })
})
