import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { MENU_FRAME, MenuList } from '../components/Menu'
import type {
  LayerMenu,
  LayerMenuClosed,
  LayerRect,
  LayerSuggestions,
  MenuLayerPort,
  SuggestionsLayerHandle
} from './menuLayer'

/**
 * The browser's menu layer, played in the test's own document.
 *
 * A menu is drawn as the layer page draws it (`MenuList`, taking the keys),
 * in a container of its own, so a test finds and clicks it by role as a person
 * would; a pick, Escape or a press anywhere else closes it as main does. The
 * blur a press elsewhere causes reaches the window after that press, as it
 * does from main, so it is delivered once the press's own listeners have run.
 *
 * The suggestions are not drawn here - the window keeps its own copy of the
 * list for assistive technology, which a test reads - but what the window
 * sends is recorded, and the layer's side (a pick, a hover, a press on the
 * list) is played through `suggestions`.
 */
export interface FakeMenuLayer extends MenuLayerPort {
  /** Every menu opened, newest last. */
  readonly menus: Array<{ menu: LayerMenu; rect: LayerRect; updates: number; open: boolean }>
  /** The suggestions open now, or null. */
  readonly suggestions: {
    content: LayerSuggestions
    rect: LayerRect
    updates: number
    /** The layer reports the pointer picking a row. */
    pick: (index: number) => void
    hover: (index: number) => void
    /** A press on the list: the keyboard leaves the bar and comes straight back, and the list stays. */
    press: () => void
  } | null
  /** How many times the suggestions were opened. */
  readonly suggestionsOpened: number
  /** How many times the bar said it lost the keyboard while the suggestions were up. */
  readonly blurs: number
}

export function fakeMenuLayer(): FakeMenuLayer {
  const menus: FakeMenuLayer['menus'] = []
  let suggestions: FakeMenuLayer['suggestions'] = null
  let blurs = 0
  let suggestionsOpened = 0

  return {
    menus,
    get suggestions() {
      return suggestions
    },
    get blurs() {
      return blurs
    },
    get suggestionsOpened() {
      return suggestionsOpened
    },

    open(menu, rect, on) {
      const record = { menu, rect, updates: 0, open: true }
      menus.push(record)
      const container = document.createElement('div')
      container.dataset['fakeMenuLayer'] = ''
      document.body.append(container)
      const root: Root = createRoot(container)
      let current = menu
      const end = (): void => {
        if (!record.open) return
        record.open = false
        document.removeEventListener('pointerdown', elsewhere, true)
        // Unmounted after the event that ended it, as a separate document's would be.
        queueMicrotask(() => {
          act(() => root.unmount())
          container.remove()
        })
      }
      const closed = (why: LayerMenuClosed): void => {
        if (!record.open) return
        end()
        on.closed(why)
      }
      const draw = (): void =>
        root.render(
          <MenuList
            label={current.label}
            entries={current.entries}
            role={current.role}
            stayOpen={current.stayOpen}
            focus
            onSelect={(id) => on.pick(id)}
            onDismiss={closed}
            className={MENU_FRAME}
          />
        )
      // A press anywhere but the layer takes the keyboard from it: its blur
      // arrives once that press has been handled.
      const elsewhere = (event: PointerEvent): void => {
        if (event.target instanceof Node && container.contains(event.target)) return
        queueMicrotask(() => closed('blur'))
      }
      document.addEventListener('pointerdown', elsewhere, true)
      draw()
      return {
        update: (next) => {
          if (!record.open) return
          record.updates += 1
          current = next
          draw()
        },
        close: end
      }
    },

    openSuggestions(content, rect, on) {
      let pressed = false
      const state = {
        content,
        rect,
        updates: 0,
        pick: (index: number) => on.pick(index),
        hover: (index: number) => on.hover(index),
        press: () => {
          pressed = true
        }
      }
      suggestions = state
      suggestionsOpened += 1
      const open = (): boolean => suggestions === state
      const handle: SuggestionsLayerHandle = {
        update: (next, at) => {
          if (!open()) return
          state.updates += 1
          state.content = next
          state.rect = at
        },
        blurred: () => {
          if (!open()) return
          blurs += 1
          if (pressed) {
            pressed = false
            return
          }
          suggestions = null
          on.closed('blur')
        },
        close: () => {
          if (open()) suggestions = null
        }
      }
      return handle
    }
  }
}
