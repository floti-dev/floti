import { useEffect, useState, type JSX } from 'react'
import { BrowserSuggestions, MENU_FRAME, MenuList } from '@helm/ui'
import type { BrowserLayerShow, HelmBridge } from '../../../shared/ipc'
import { applyLook } from '../app/theme'

/**
 * What the menu layer draws: nothing, or the one menu or suggestions list
 * main last sent, filling the page. The page is exactly the menu's rectangle (main sizes the view to
 * it), so the list is drawn edge to edge with the popup recipe's frame, and
 * the window's theme and shape are painted first, from the look that came
 * with it.
 *
 * A menu takes the keys as soon as it is drawn; a pick and a close go back
 * to main on the turn they happened in. The suggestions never take them -
 * the address bar keeps the keyboard and sends the highlight - so only the
 * pointer's pick and hover go back, by row.
 */
export function LayerRoot({ bridge }: { bridge: Pick<HelmBridge, 'on' | 'send'> }): JSX.Element | null {
  const [show, setShow] = useState<BrowserLayerShow>(null)

  useEffect(() => {
    const off = bridge.on('browser:layer-show', (next) => {
      if (next !== null) applyLook(next.look)
      setShow(next)
    })
    // Listening now: anything main held for the page comes after this.
    bridge.send('browser:layer-report', { ready: true })
    return off
  }, [bridge])

  if (show === null) return null
  const { turn, content } = show
  if (content.kind === 'suggestions') {
    return (
      // Hidden from assistive technology: the window keeps the list it reads,
      // which the address bar's aria-activedescendant points into.
      <div aria-hidden="true">
        <BrowserSuggestions
          id="layer-suggestions"
          rows={content.rows}
          highlighted={content.highlighted}
          onPick={(row) => bridge.send('browser:layer-report', { turn, row })}
          onHover={(hover) => bridge.send('browser:layer-report', { turn, hover })}
        />
      </div>
    )
  }
  return (
    <MenuList
      // A new menu starts on its own first row; new content for the same one keeps the row.
      key={turn}
      label={content.label}
      entries={content.entries}
      role={content.role}
      stayOpen={content.stayOpen}
      focus
      onSelect={(id) => bridge.send('browser:layer-report', { turn, pick: id })}
      onDismiss={(why) => {
        setShow(null)
        bridge.send('browser:layer-report', { turn, closed: why })
      }}
      // The page is the menu's rectangle, and `#root` fills the page.
      className={`h-full w-full overflow-y-auto ${MENU_FRAME}`}
    />
  )
}
