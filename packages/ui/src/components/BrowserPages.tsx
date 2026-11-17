import { useState, type JSX } from 'react'
import { GlobeIcon } from './icons'
import { TabBar, type Tab } from './TabBar'

/** One page in the Browser tab's strip. */
export interface BrowserPage {
  id: number
  title: string
  url: string
  /** The address it was asked for and refused, while it has loaded nothing. */
  refusedUrl: string | null
  /** The session that opened it, or null when the user did. */
  openedBy: string | null
  /** The session the user shared it with, or null. */
  sharedWith: { session: number; name: string } | null
  /** Whether its page is loading: the tab shows a spinner in place of its icon. */
  loading: boolean
  /** Its site's icon as a `data:` URL, or null for the globe. */
  icon: string | null
}

/**
 * The drag type of a page tab. Its own, so a page cannot be dropped among a
 * pane's tabs and a pane's tab cannot be dropped among the pages.
 */
export const BROWSER_PAGE_MIME = 'application/x-floti-browser-page'

export interface BrowserPagesProps {
  pages: readonly BrowserPage[]
  activeId: number | null
  /** Whether the pane holding the Browser tab is the focused one. */
  focused: boolean
  onActivate: (id: number) => void
  onClose: (id: number) => void
  /** A page should end up at `toIndex`, counted after it has left its place. */
  onMove: (id: number, toIndex: number) => void
  onNew: () => void
}

/** The size of a page tab's icon, in pixels: the globe's, so the three states take one place. */
const ICON_SIZE = 13

/**
 * What a page tab shows before its title: a spinner while the page loads, then
 * the site's icon, or the globe for a site with none - or with one the image
 * decoder refuses, which only shows itself as an `error` on the `<img>`.
 */
function PageIcon({ loading, icon }: { loading: boolean; icon: string | null }): JSX.Element {
  if (loading) {
    // A quarter arc turning on a faint ring, in the tab's icon colour: the
    // shape every browser draws a loading tab with.
    return (
      <svg
        data-page-icon="loading"
        aria-hidden="true"
        width={ICON_SIZE}
        height={ICON_SIZE}
        viewBox="0 0 16 16"
        focusable="false"
        className="block animate-spin"
      >
        <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" strokeOpacity="0.25" strokeWidth="1.5" />
        <path d="M8 2a6 6 0 0 1 6 6" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      </svg>
    )
  }
  // Keyed on the icon, so a new icon gets a fresh chance after a broken one.
  if (icon !== null) return <SiteIcon key={icon} icon={icon} />
  return <GlobeIcon data-page-icon="globe" width={ICON_SIZE} height={ICON_SIZE} />
}

function SiteIcon({ icon }: { icon: string }): JSX.Element {
  const [broken, setBroken] = useState(false)
  if (broken) return <GlobeIcon data-page-icon="globe" width={ICON_SIZE} height={ICON_SIZE} />
  return (
    <img
      data-page-icon="favicon"
      src={icon}
      alt=""
      width={ICON_SIZE}
      height={ICON_SIZE}
      draggable={false}
      className="block object-contain"
      style={{ width: ICON_SIZE, height: ICON_SIZE }}
      onError={() => setBroken(true)}
    />
  )
}

/** A page's id in the strip. Never shaped like a pane's tab id. */
const tabId = (id: number): string => `page:${String(id)}`

/**
 * The Browser tab's own strip of pages.
 *
 * The pane's `TabBar`, so a page tab looks, closes, middle-clicks and
 * reorders exactly like every other tab in Helm - and a strip of its own,
 * inside the Browser tab, so pages never sit among the sessions. That was the
 * complaint: a sign-in that opened four pages left four tabs between the
 * sessions somebody was working in.
 *
 * A page an agent opened carries the session's name, as its tab did before,
 * and a page the user shared with a session says "shared".
 */
export function BrowserPages({
  pages,
  activeId,
  focused,
  onActivate,
  onClose,
  onMove,
  onNew
}: BrowserPagesProps): JSX.Element {
  const byTab = new Map(pages.map((page) => [tabId(page.id), page.id]))
  const tabs: Tab[] = pages.map((page) => {
    // The address, which tells three pages on one dev server apart, is in the
    // hint. An empty page is "New tab": a tab with no label is a tab you
    // cannot aim at. One refused its first address names that address, and
    // says nothing loaded there.
    const where =
      page.url !== ''
        ? page.url
        : page.refusedUrl !== null
          ? `${page.refusedUrl}\nRefused, so nothing has loaded`
          : 'A browser tab with no address yet'
    const badge = page.openedBy ?? (page.sharedWith === null ? undefined : 'shared')
    return {
      id: tabId(page.id),
      title: page.title === '' ? 'New tab' : page.title,
      ...(badge === undefined ? {} : { badge }),
      hint:
        page.openedBy !== null
          ? `${where}\nOpened by the session “${page.openedBy}”`
          : page.sharedWith !== null
            ? `${where}\nShared with the session “${page.sharedWith.name}”`
            : where,
      icon: <PageIcon loading={page.loading} icon={page.icon} />
    }
  })
  /** The page behind a strip id, or nothing for an id that is not one of ours. */
  const page = (id: string, then: (page: number) => void): void => {
    const found = byTab.get(id)
    if (found !== undefined) then(found)
  }

  return (
    <div data-browser-pages>
      <TabBar
        label="Browser tabs"
        tabs={tabs}
        activeId={activeId === null ? null : tabId(activeId)}
        focused={focused}
        onActivate={(id) => page(id, onActivate)}
        onClose={(id) => page(id, onClose)}
        onMove={(id, toIndex) => page(id, (found) => onMove(found, toIndex))}
        onNewTab={onNew}
        newTabLabel="New browser tab"
        newTabMenu={false}
        dragType={BROWSER_PAGE_MIME}
      />
    </div>
  )
}
