import type { HostConnection } from '../shared/host/client'
import type * as P from '../shared/host/protocol'

/**
 * Updates, from Helm's main process: the addresses releases live at, and
 * quitting when this PC's host is updating the app.
 *
 * Helm's own process contacts nothing about updates. This PC's host checks
 * floti.dev's signed `update.json` (`crates/host/src/update/background.rs`)
 * and tells its clients on `update:state`; the window shows that, asks the
 * host to check now (`update:check`) and to update (`update:install`) over
 * its own connection (`renderer/src/app/host/updates.ts`). The update the
 * host runs, `floti update --from-desktop`, replaces floti and this app,
 * and waits for the app to quit before it replaces it. Main hears that and
 * quits, asking nothing and leaving every session running
 * (`followHostUpdate`); the update opens the app again once it is replaced.
 */

/** Where releases are published. Not a filesystem path and not a user's - the
 * project's own address, the same identity `appId` carries. Opened through
 * `shell:openExternal` when a person asks for the release notes, never
 * fetched. */
export const RELEASES_PAGE = 'https://github.com/floti-dev/floti/releases/latest'
/**
 * Where each release's files are, under `v<version>`. This PC never fetches
 * from it: a host's computer Helm installs floti on downloads its own build
 * from there, after the user's yes (`host/remote-install.ts`).
 */
export const RELEASES_DOWNLOAD = 'https://github.com/floti-dev/floti/releases/download'

/**
 * Follows `update:state` on `connection`, this PC's host's, and calls
 * `quit` once when a `floti update` there is waiting for this app to quit.
 * A state that already says so when the app connects quits it too: the
 * update replaces whichever copy runs, and opens the new one. Returns the
 * stop; a connection that ends stops it as well.
 */
export function followHostUpdate(
  connection: Pick<HostConnection, 'subscribe'>,
  quit: () => void,
  log: (line: string) => void
): () => void {
  let quitAsked = false
  const hear = (state: P.UpdateState): void => {
    if (quitAsked || state.installing?.waitingForApp !== true) return
    quitAsked = true
    log('update: floti update is waiting for Floti to quit; quitting, with every session left running')
    quit()
  }
  const subscription = connection.subscribe('update:state', {
    snapshot: hear,
    event: hear,
    error: (error) => log(`update: following this PC's host's updates ended (${error.message})`)
  })
  return () => subscription.unsubscribe()
}
