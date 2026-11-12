import { compareVersions } from '@helm/core/types'
import type { UpdateView } from '@helm/ui'
import type { HostSubscription } from '../../../../shared/host/client'
import type * as P from '../../../../shared/host/protocol'
import type { HostReach } from './history'

/**
 * Settings > Updates and the status bar's offer, over the window's own
 * connection to this PC's host. The app contacts nothing itself.
 *
 * - What the host knows comes from an `update:state` subscription made on
 *   every connect, sent whole first and again on every change: what its last
 *   check found, a check running, each step of an update on this computer,
 *   and why a host that never checks (a dev or check run) does not. A host
 *   relaunched on a newer floti ends the connection; the next one subscribes
 *   again, and its first state clears what the end said.
 * - Check now is `update:check`: the host checks at once and answers once the
 *   check has ended.
 * - Update is `update:install`: the host starts `floti update`, which
 *   replaces floti and this app. Main quits the app when the update waits
 *   for it (`main/update.ts`), and the update opens the new one.
 * - Whether a newer release exists is the newer of the channel's newest
 *   version and the host's own floti, against this app's version: the
 *   host's floti is up to date by the time the host knows of one, and the
 *   app is what is behind, even where no check is kept.
 */

/** What a watch heard: the host's state, or why it cannot be heard now. */
export type UpdateHeard = { ok: true; state: P.UpdateState } | { ok: false; message: string }

/** How a Check now ended: the state after it, or why it could not be asked. */
export type CheckResult = { ok: true; state: P.UpdateState } | { ok: false; message: string }

/** How an Update ended: started, or the sentence that says why not. */
export type InstallResult = { ok: true } | { ok: false; message: string }

export interface UpdatesDeps {
  log: (line: string) => void
}

export interface Updates {
  /**
   * Watches this PC's host's `update:state`: `listener` hears what was last
   * heard at once, then the whole state on every change, on every
   * connection, and why it cannot be heard whenever a connection fails or
   * ends. Returns the stop.
   */
  watch(listener: (heard: UpdateHeard) => void): () => void
  /** Asks the host to check now (`update:check`). */
  check(): Promise<CheckResult>
  /** Asks the host to update floti and this app (`update:install`). */
  install(): Promise<InstallResult>
}

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error))

export function createUpdates(link: HostReach, deps: UpdatesDeps): Updates {
  const listeners = new Set<(heard: UpdateHeard) => void>()
  let latest: UpdateHeard | null = null
  let subscription: HostSubscription | null = null

  const tell = (heard: UpdateHeard): void => {
    latest = heard
    for (const listener of [...listeners]) listener(heard)
  }

  link.onConnected((connection) => {
    subscription?.unsubscribe()
    const hear = (state: P.UpdateState): void => tell({ ok: true, state })
    subscription = connection.subscribe('update:state', {
      snapshot: hear,
      event: hear,
      error: (error) => {
        deps.log(`update: hearing this PC's host's updates ended (${error.message})`)
        tell({ ok: false, message: error.message })
      }
    })
  })

  return {
    watch(listener) {
      listeners.add(listener)
      if (latest !== null) listener(latest)
      link.connection().catch((error: unknown) => tell({ ok: false, message: messageOf(error) }))
      return () => {
        listeners.delete(listener)
      }
    },

    async check() {
      try {
        const state = await (await link.connection()).request('update:check', null)
        return { ok: true, state }
      } catch (error) {
        deps.log(`update: asking this PC's host to check failed (${messageOf(error)})`)
        return { ok: false, message: messageOf(error) }
      }
    },

    async install() {
      try {
        await (await link.connection()).request('update:install', null)
        return { ok: true }
      } catch (error) {
        deps.log(`update: asking this PC's host to update failed (${messageOf(error)})`)
        return { ok: false, message: messageOf(error) }
      }
    }
  }
}

/** The newer of two versions, when either can be placed against the other. */
function newerOf(a: string | null, b: string): string {
  if (a === null) return b
  const order = compareVersions(a, b)
  return order !== null && order < 0 ? b : a
}

/**
 * The host's `state` as the window draws it for an app at `appVersion`. A
 * version either side cannot read is never called newer, so a mangled one
 * never offers an update.
 */
export function updateView(state: P.UpdateState, appVersion: string | null): UpdateView {
  const newest = newerOf(state.newest, state.running)
  const order = appVersion !== null ? compareVersions(newest, appVersion) : null
  return {
    newest,
    newer: order !== null && order > 0,
    checkedAt: state.checkedAt,
    failed: state.failed,
    checking: state.checking,
    unavailable: state.unavailable,
    installing: state.installing,
    installFailed: state.installFailed
  }
}
