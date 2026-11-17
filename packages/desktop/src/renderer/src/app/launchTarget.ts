/** Where a launcher opens: the host it starts on, and the folder it highlights there. */
export interface LaunchTarget {
  host: string
  /** The folder highlighted when it opens; null for none. */
  folder: string | null
}

/**
 * A launcher asked for on `host`, from what is in front of the focused pane.
 *
 * It opens on `host` whatever is in front: an empty state that offers this
 * PC's New session opens this PC's launcher even with another computer's tab
 * in front. The front tab's folder is highlighted only when that tab is on
 * `host` too, since a folder on another computer is no folder here.
 */
export function launchTarget(host: string, front: { host: string; folder: string | null }): LaunchTarget {
  return { host, folder: front.host === host ? front.folder : null }
}
