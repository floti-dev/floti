import type { BrowserWindow, WebContents } from 'electron'
import type { HelmTheme } from '@floti-dev/plugin-sdk'
import type { PluginRuntime } from './protocol'

/**
 * What the desktop keeps of plugins for its own pages.
 *
 * The host keeps the plugin registry - the folders, their switches, settings,
 * secrets, services, backgrounds and page files - and answers every bridge
 * call and every session's call to a plugin's tool (`host/plugins.ts`). What
 * stays here is what concerns the pages the desktop draws: the runtime they
 * are served (`protocol.ts`), the theme they paint in, and which web
 * contents may frame them: the window.
 */

export interface PluginHostOptions {
  /** The app's window: the page that frames plugins. */
  window: () => BrowserWindow | null
  /** The theme a page paints first; it is kept current from the window after that. */
  theme: () => HelmTheme
  /** The bridge script and primitives stylesheet, as served under `/__helm/`. */
  bridge: string
  stylesheet: string
}

export interface PluginHost {
  /** A theme or shape change, for the window that frames plugins. */
  pushTheme(theme: HelmTheme): void
  /** Whether a web contents is the desktop page that frames plugins: the window. */
  framesPlugins(contentsId: number): boolean
  runtime: PluginRuntime
}

export function createPluginHost(options: PluginHostOptions): PluginHost {
  const windowContents = (): WebContents | null => {
    const win = options.window()
    return win === null || win.isDestroyed() ? null : win.webContents
  }

  return {
    pushTheme(theme) {
      const contents = windowContents()
      if (contents !== null && !contents.isDestroyed()) contents.send('plugins:theme', theme)
    },

    framesPlugins: (contentsId) => windowContents()?.id === contentsId,

    runtime: {
      bridge: () => options.bridge,
      css: () => options.stylesheet,
      boot: () => JSON.stringify({ theme: options.theme() })
    }
  }
}
