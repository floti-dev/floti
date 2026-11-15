import { NOTICES_SEEN_HOSTS_MAX } from '@helm/core/types'

/**
 * What this device has shown of a host's notices: the `since` of the newest
 * one it was given, in that host's clock. While you were away lists only the
 * notices that began after it, so a notice the device has shown is not news
 * again when its connection to the host drops and comes back, nor after Floti
 * restarts. Everything undismissed still stands in the notices list.
 */
export interface NoticesSeen {
  /** The newest `since` shown, or null when none has been; a promise while it is being read. */
  read(): string | null | Promise<string | null>
  /** Every notice up to `since` has been shown. A `since` no newer than the one kept changes nothing. */
  mark(since: string): void
}

/** Whether `since` began after `mark`: by the instant each names, or as text when either names none. */
export function newerThan(since: string, mark: string | null): boolean {
  if (mark === null) return true
  const at = Date.parse(since)
  const seen = Date.parse(mark)
  return Number.isNaN(at) || Number.isNaN(seen) ? since > mark : at > seen
}

/** The newest of `sinces`, or null for none. */
export function newestOf(sinces: Iterable<string>): string | null {
  let newest: string | null = null
  for (const since of sinces) if (newerThan(since, newest)) newest = since
  return newest
}

/** A mark kept in memory alone: it holds across reconnects, and is gone with the page. */
export function memorySeen(): NoticesSeen {
  let mark: string | null = null
  return {
    read: () => mark,
    mark: (since) => {
      if (newerThan(since, mark)) mark = since
    }
  }
}

/** Where every host's mark is kept: `noticesSeen` in this device's own settings (helm.db). */
export interface SeenStorage {
  read(): Promise<Readonly<Record<string, string>>>
  write(marks: Readonly<Record<string, string>>): Promise<unknown>
}

export interface SeenMarks {
  /** Host `host`'s mark, kept with every other host's. */
  forHost(host: string): NoticesSeen
}

/**
 * Every host's mark, read from `storage` once when a host first asks and
 * written whole when one moves, one write at a time and the latest last. Past
 * `NOTICES_SEEN_HOSTS_MAX` hosts the oldest marks are dropped. A mark that
 * cannot be read is none: the host's standing notices are listed as away, as
 * they would be on a device that has shown nothing.
 */
export function createSeenMarks(storage: SeenStorage, log: (line: string) => void = () => undefined): SeenMarks {
  let marks: Record<string, string> | null = null
  let loading: Promise<void> | null = null
  let writing: Promise<unknown> = Promise.resolve()

  const load = (): Promise<void> =>
    (loading ??= storage.read().then(
      (read) => {
        marks = { ...read }
      },
      (error: unknown) => {
        log(`notices: what this device has seen could not be read (${(error as Error).message})`)
        marks ??= {}
      }
    ))

  const save = (): void => {
    const kept = marks ?? {}
    const hosts = Object.keys(kept)
    if (hosts.length > NOTICES_SEEN_HOSTS_MAX) {
      const oldest = hosts.sort((a, b) => (newerThan(kept[a] ?? '', kept[b] ?? null) ? 1 : -1))
      for (const host of oldest.slice(0, hosts.length - NOTICES_SEEN_HOSTS_MAX)) delete kept[host]
    }
    const snapshot = { ...kept }
    writing = writing
      .then(() => storage.write(snapshot))
      .catch((error: unknown) => {
        log(`notices: what this device has seen could not be kept (${(error as Error).message})`)
      })
  }

  return {
    forHost: (host) => ({
      read: () => (marks === null ? load().then(() => marks?.[host] ?? null) : (marks[host] ?? null)),
      mark: (since) => {
        const apply = (): void => {
          const kept = (marks ??= {})
          if (!newerThan(since, kept[host] ?? null)) return
          kept[host] = since
          save()
        }
        if (marks === null) void load().then(apply)
        else apply()
      }
    })
  }
}
