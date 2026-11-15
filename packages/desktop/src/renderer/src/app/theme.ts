import { THEME_TOKENS, type AppSettings, type ThemeState } from '@helm/core/types'
import type { BrowserLayerLook } from '../../../shared/ipc'

/**
 * Paints a theme onto the document: every token as a `--helm-*` custom
 * property on `<html>`, which is what `theme.css` maps the Tailwind colours
 * onto, plus the `.dark` class and `color-scheme` for the few things keyed on
 * the kind rather than a colour (Chromium's own form controls, shiki's two
 * palettes).
 *
 * Inline on `<html>` rather than a stylesheet of `[data-theme]` blocks: a user
 * theme is data that arrives at runtime, and the built-ins arriving the same
 * way means there is one path, not one for Helm's themes and another for
 * everybody else's. Every value was parsed and re-spelled by core
 * (`formatColor`), so nothing here is text a theme file typed.
 */
export function applyTheme(state: ThemeState): void {
  const root = document.documentElement
  for (const token of THEME_TOKENS) {
    root.style.setProperty(`--helm-${token}`, state.applied.tokens[token])
  }
  root.style.setProperty('--helm-shadow', state.applied.shadow)
  root.classList.toggle('dark', state.applied.kind === 'dark')
  root.style.colorScheme = state.applied.kind
  root.dataset['theme'] = state.applied.id
}

/**
 * The shape settings, as the three custom properties every gutter, corner and
 * dense row in the app reads (`theme.css`, "Shape"). Separate from the theme
 * because they are not part of one: a person's corners survive a change of
 * colours.
 */
export function applyShape(
  settings: Pick<AppSettings, 'paneGap' | 'cornerRadius' | 'density'>
): void {
  const root = document.documentElement
  root.style.setProperty('--helm-gap', `${String(settings.paneGap)}px`)
  root.style.setProperty('--helm-radius', `${String(settings.cornerRadius)}px`)
  root.dataset['density'] = settings.density
}

/**
 * The theme and shape as the window has painted them, for the browser's menu
 * layer (`layer/main.tsx`) to paint the same: every custom property on
 * `<html>` - the theme's tokens and shadow, the shape's gap and corner - its
 * kind, and its data attributes (the theme's id, the density). Read off the
 * document rather than rebuilt from settings, so the layer matches whatever
 * `applyTheme` and `applyShape` last did, by construction.
 */
export function lookOfDocument(root: HTMLElement = document.documentElement): BrowserLayerLook {
  const vars: Record<string, string> = {}
  for (let i = 0; i < root.style.length; i += 1) {
    const name = root.style.item(i)
    if (name.startsWith('--')) vars[name] = root.style.getPropertyValue(name)
  }
  return {
    vars,
    kind: root.classList.contains('dark') ? 'dark' : 'light',
    data: { ...root.dataset } as Record<string, string>
  }
}

/** Paints a look read by `lookOfDocument` onto this document. */
export function applyLook(look: BrowserLayerLook, root: HTMLElement = document.documentElement): void {
  for (const [name, value] of Object.entries(look.vars)) root.style.setProperty(name, value)
  root.classList.toggle('dark', look.kind === 'dark')
  root.style.colorScheme = look.kind
  for (const [key, value] of Object.entries(look.data)) root.dataset[key] = value
}
