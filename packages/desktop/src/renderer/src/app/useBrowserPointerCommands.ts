import { useEffect } from 'react'
import type { BrowserCommand } from '../../../shared/browserKeys'
import { helm } from './bridge'

/**
 * The mouse's back and forward buttons. Windows and Linux deliver them to the
 * window as an `app-command`, and main sends where the pointer was
 * (`browser:pointerCommand`); the page in front goes back or forward when that
 * is over the Browser tab - its view, strip or bar.
 *
 * `shown` is whether the Browser tab is in front of any visible pane, not the
 * focused one: a click in a page's native view never focuses its pane, so with
 * the browser beside a session the buttons would otherwise do nothing over a
 * page the user is looking at. Only `frontPage` is ever the target; a page an
 * agent drives in the background is not. macOS has no such event, and the
 * Browser tab's own `onMouseUp` in `App.tsx` reads the buttons there.
 */
export function useBrowserPointerCommands(
  shown: boolean,
  frontPage: number | null,
  run: (page: number, command: BrowserCommand) => void
): void {
  useEffect(
    () =>
      helm.on('browser:pointerCommand', ({ command, at }) => {
        if (!shown || frontPage === null) return
        const tab = document.querySelector('[data-browser-tab]')
        if (tab === null) return
        const box = tab.getBoundingClientRect()
        if (at.x < box.left || at.x >= box.right || at.y < box.top || at.y >= box.bottom) return
        run(frontPage, command)
      }),
    [shown, frontPage, run]
  )
}
