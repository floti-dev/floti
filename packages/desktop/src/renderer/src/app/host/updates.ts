import { compareVersions } from '@helm/core/types'
import type { UpdateView } from '@helm/ui'
import type { HostConnection } from '../../../../shared/host/client'
import type * as P from '../../../../shared/host/protocol'

/**
 * Settings > Updates and the status bar's offer, over the window's own
 * connection to this PC's host. The app contacts nothing itself.
 *
 * - What the host knows comes from an `update:state` subscription, sent whole
 *   first and again on every change: what its last check found, a check
 *   running, each step of an update it runs, and why a host that never
 *   checks (a dev or check run) does not.
 * - Check now is `update:check`: the host checks at once and answers once the
 *   check has ended.
 * - Update is `update:install`: the host starts `floti update`, which
 *   replaces floti and this app. Main quits the app when the update waits
 *   for it (`main/update.ts`), and the update opens the new one.
 * - Whether a newer release exists is the channel's newest version against
 *   this app's own: the host's floti is up to date by the time the host
 *   knows of one, and the app is what is behind.
 */

/** What a watch heard: the host's state, or why it cannot be watched. */
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
   * Watches `update:state` on the host `connection` reaches: `listener`
   * hears the whole state first and after every change, or why it cannot be
   * watched, after which it hears nothing more. Returns the stop.
   */
  watch(connection: () => Promise<HostConnection>, listener: (heard: UpdateHeard) => void): () => void
  /** Asks the host to check now (`update:check`). */
  check(connection: () => Promise<HostConnection>): Promise<CheckResult>
  /** Asks the host to update floti and this app (`update:install`). */
  install(connection: () => Promise<HostConnection>): Promise<InstallResult>
}

const messageOf = (error: unknown): string => (error instanceof Error ? error.message : String(error))

export function createUpdates(deps: UpdatesDeps): Updates {
  return {
    watch(connection, listener) {
      let stopped = false
      let stop = (): void => {
        stopped = true
      }
      const tell = (heard: UpdateHeard): void => {
        if (!stopped) listener(heard)
      }
      connection().then(
        (reached) => {
          if (stopped) return
          const hear = (state: P.UpdateState): void => tell({ ok: true, state })
          const subscription = reached.subscribe('update:state', {
            snapshot: hear,
            event: hear,
            error: (error) => {
              deps.log(`update: watching this PC's host's updates ended (${error.message})`)
              tell({ ok: false, message: error.message })
            }
          })
          stop = () => {
            stopped = true
            subscription.unsubscribe()
          }
        },
        (error: unknown) => tell({ ok: false, message: messageOf(error) })
      )
      return () => stop()
    },

    async check(connection) {
      try {
        const state = await (await connection()).request('update:check', null)
        return { ok: true, state }
      } catch (error) {
        deps.log(`update: asking this PC's host to check failed (${messageOf(error)})`)
        return { ok: false, message: messageOf(error) }
      }
    },

    async install(connection) {
      try {
        await (await connection()).request('update:install', null)
        return { ok: true }
      } catch (error) {
        deps.log(`update: asking this PC's host to update failed (${messageOf(error)})`)
        return { ok: false, message: messageOf(error) }
      }
    }
  }
}

/**
 * The host's `state` as the window draws it for an app at `appVersion`. A
 * version either side cannot read is never called newer, so a mangled one
 * never offers an update.
 */
export function updateView(state: P.UpdateState, appVersion: string | null): UpdateView {
  const order = state.newest !== null && appVersion !== null ? compareVersions(state.newest, appVersion) : null
  return {
    newest: state.newest,
    newer: order !== null && order > 0,
    checkedAt: state.checkedAt,
    failed: state.failed,
    checking: state.checking,
    unavailable: state.unavailable,
    installing: state.installing,
    installFailed: state.installFailed
  }
}
