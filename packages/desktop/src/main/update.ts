import { app, net } from 'electron'
import type { UpdateCheck } from '../shared/ipc'

/**
 * What Helm's own process knows about newer releases. It makes one kind of
 * request, and only when a person asks: Check now in Settings
 * (`checkForUpdate`, through `update:check`), which reads a version number
 * and hands back a link. It downloads nothing and executes nothing.
 *
 * Helm asks nothing by itself. The local host keeps floti up to date in the
 * background while its `updateCheck` setting is on (`crates/host/src/update/
 * background.rs`), which is the one automatic contact: floti.dev's `update.json`, and
 * the GitHub release it names when there is a newer floti. Helm hears of a newer release from that host: a host that
 * runs a newer floti than this Helm is one that put a newer release in place,
 * and `hostVersionNotice` turns that into the same answer a check gives, for
 * the status bar. Updating the desktop app itself is `floti update`'s.
 */

/** Where releases are published. Not a filesystem path and not a user's - the
 * project's own address, the same identity `appId` carries. */
export const RELEASES_API = 'https://api.github.com/repos/floti-dev/floti/releases/latest'
export const RELEASES_PAGE = 'https://github.com/floti-dev/floti/releases/latest'
/**
 * Where each release's files are, under `v<version>`. This PC never fetches
 * from it: a host's computer Helm installs floti on downloads its own build
 * from there, after the user's yes (`host/remote-install.ts`).
 */
export const RELEASES_DOWNLOAD = 'https://github.com/floti-dev/floti/releases/download'

const TIMEOUT_MS = 6000

/**
 * Where the tag comes from, and a seam for the checks to aim somewhere else.
 *
 * A function rather than a URL on purpose. Pointing this at a second address
 * would mean a check standing up an HTTP server, and this app has no inbound
 * listener anywhere in it - a claim worth keeping true. Replacing the source
 * instead leaves the request machinery alone and lets a driver produce the one
 * thing the network cannot be asked for on demand: a newer version, an older
 * one, and a failure, in that order, on a machine that is online.
 *
 * Not on the IPC contract, for the same reason `pointGh` is not: which address
 * Helm asks is not the window's to choose.
 */
let releaseSource: (() => Promise<string>) | null = null
let timesAsked = 0

export function pointReleases(source: (() => Promise<string>) | null): void {
  releaseSource = source
  timesAsked = 0
}

/** How many times the source has been asked since it was pointed. */
export function releasesAsked(): number {
  return timesAsked
}

function parseSemver(text: string): [number, number, number] | null {
  const match = /(\d+)\.(\d+)\.(\d+)/.exec(text)
  if (!match) return null
  return [Number(match[1]), Number(match[2]), Number(match[3])]
}

/** True only when `latest` is strictly higher, so a mangled tag never nags. */
export function isNewer(current: string, latest: string): boolean {
  const a = parseSemver(latest)
  const b = parseSemver(current)
  if (!a || !b) return false
  for (let i = 0; i < 3; i++) {
    const diff = (a[i] ?? 0) - (b[i] ?? 0)
    if (diff !== 0) return diff > 0
  }
  return false
}

function fetchLatestTag(): Promise<string> {
  return new Promise((resolve, reject) => {
    const request = net.request({ method: 'GET', url: RELEASES_API })
    // GitHub rejects a request with no user agent, and the version identifies
    // which build asked - which is the entire payload of this request.
    request.setHeader('User-Agent', `Helm/${app.getVersion()}`)
    request.setHeader('Accept', 'application/vnd.github+json')

    const timer = setTimeout(() => {
      request.abort()
      reject(new Error(`no answer within ${String(TIMEOUT_MS)} ms`))
    }, TIMEOUT_MS)

    request.on('response', (response) => {
      const chunks: Buffer[] = []
      response.on('data', (chunk: Buffer) => chunks.push(chunk))
      response.on('end', () => {
        clearTimeout(timer)
        if (response.statusCode !== 200) {
          reject(new Error(`GitHub answered ${String(response.statusCode)}`))
          return
        }
        try {
          const parsed: unknown = JSON.parse(Buffer.concat(chunks).toString('utf8'))
          const tag =
            parsed !== null && typeof parsed === 'object'
              ? (parsed as Record<string, unknown>)['tag_name']
              : null
          if (typeof tag !== 'string') {
            reject(new Error('the answer carried no tag'))
            return
          }
          resolve(tag)
        } catch (err) {
          reject(err instanceof Error ? err : new Error(String(err)))
        }
      })
    })
    request.on('error', (err) => {
      clearTimeout(timer)
      reject(err)
    })
    request.end()
  })
}

/**
 * What the window is told when the local host runs `hostVersion` and this
 * Helm is `current`: a newer release, without a request, when the host's is
 * strictly higher (its background update put it in place); nothing otherwise.
 */
export function hostVersionNotice(current: string, hostVersion: string, checkedAt: string): UpdateCheck | null {
  if (!isNewer(current, hostVersion)) return null
  return { current, latest: hostVersion, newer: true, url: RELEASES_PAGE, error: null, checkedAt }
}

/**
 * Ask now, whoever is asking and however recently anyone last did.
 *
 * Deliberately unthrottled: it is only ever reached because a person pressed
 * something, and a deliberate act that silently did nothing would be worse
 * than no button at all.
 */
export async function checkForUpdate(): Promise<UpdateCheck> {
  const current = app.getVersion()
  const checkedAt = new Date().toISOString()
  try {
    timesAsked += 1
    const tag = await (releaseSource ?? fetchLatestTag)()
    const latest = tag.replace(/^v/i, '')
    return {
      current,
      latest,
      newer: isNewer(current, latest),
      url: RELEASES_PAGE,
      error: null,
      checkedAt
    }
  } catch (err) {
    // Offline is the expected case, not an exception: the answer is "could not
    // ask", said out loud, rather than a silent "up to date".
    return {
      current,
      latest: null,
      newer: false,
      url: RELEASES_PAGE,
      error: err instanceof Error ? err.message : String(err),
      checkedAt
    }
  }
}
