import { useEffect, useState } from 'react'
import type { ArchiveStats } from '@floti/core'
import { bridgeFor } from './bridge'

/**
 * What the transcript archive on `host` holds, for Settings › Archive to
 * state: read once, then kept current by `archive:changed`. Null until it has
 * been read, and while it cannot be.
 */
export function useArchiveStats(host: string): ArchiveStats | null {
  const [stats, setStats] = useState<ArchiveStats | null>(null)
  // Another host: what the last one said is not this one's.
  const [shown, setShown] = useState(host)
  if (shown !== host) {
    setShown(host)
    setStats(null)
  }
  useEffect(() => {
    let live = true
    const desktop = bridgeFor(host)
    const off = desktop.on('archive:changed', (next) => {
      if (live) setStats(next)
    })
    desktop.invoke('archive:stats').then(
      (next) => live && setStats(next),
      () => undefined
    )
    return () => {
      live = false
      off()
    }
  }, [host])
  return stats
}
