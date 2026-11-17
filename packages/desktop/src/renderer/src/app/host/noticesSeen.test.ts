import { NOTICES_SEEN_HOSTS_MAX } from '@helm/core/types'
import { describe, expect, it, vi } from 'vitest'
import { createSeenMarks, memorySeen, newerThan, newestOf, type SeenStorage } from './noticesSeen'

/** desktop.db's `noticesSeen` as the window reads and writes it, recording every write. */
function device(initial: Record<string, string> = {}) {
  let kept: Readonly<Record<string, string>> = initial
  const writes: Array<Readonly<Record<string, string>>> = []
  let reads = 0
  const storage: SeenStorage = {
    read: () => {
      reads += 1
      return Promise.resolve(kept)
    },
    write: (marks) => {
      kept = marks
      writes.push(marks)
      return Promise.resolve()
    }
  }
  return { storage, writes, reads: () => reads, kept: () => kept }
}

describe('what this device has shown of a host’s notices', () => {
  it('orders instants by the time they name, whatever their spelling, and none is older than any', () => {
    expect(newerThan('2026-11-14T20:28:05.253Z', null)).toBe(true)
    expect(newerThan('2026-11-14T20:28:06Z', '2026-11-14T20:28:05.999Z')).toBe(true)
    expect(newerThan('2026-11-14T20:28:05.000Z', '2026-11-14T20:28:05Z')).toBe(false)
    expect(newerThan('2026-11-14T20:28:04Z', '2026-11-14T20:28:05Z')).toBe(false)
    expect(newestOf(['2026-11-14T10:00:00Z', '2026-11-14T12:00:00Z', '2026-11-14T11:00:00Z'])).toBe(
      '2026-11-14T12:00:00Z'
    )
    expect(newestOf([])).toBeNull()
  })

  it('keeps a mark in memory that only moves forward', () => {
    const seen = memorySeen()
    expect(seen.read()).toBeNull()
    seen.mark('2026-11-14T12:00:00Z')
    seen.mark('2026-11-14T11:00:00Z')
    expect(seen.read()).toBe('2026-11-14T12:00:00Z')
  })

  it('reads every host’s mark once, and writes them all whole when one moves forward', async () => {
    const helm = device({ 'h-lab': '2026-11-13T09:00:00Z' })
    const marks = createSeenMarks(helm.storage)
    const build = marks.forHost('h-build')
    await expect(build.read()).resolves.toBeNull()
    expect(marks.forHost('h-lab').read()).toBe('2026-11-13T09:00:00Z')

    build.mark('2026-11-14T12:00:00Z')
    build.mark('2026-11-14T11:00:00Z')
    build.mark('2026-11-14T12:30:00Z')
    await vi.waitFor(() => expect(helm.writes).toHaveLength(2))
    expect(helm.kept()).toEqual({ 'h-lab': '2026-11-13T09:00:00Z', 'h-build': '2026-11-14T12:30:00Z' })
    expect(helm.reads()).toBe(1)
    expect(build.read()).toBe('2026-11-14T12:30:00Z')
  })

  it('marks a host before its marks were read without losing another host’s', async () => {
    const helm = device({ local: '2026-11-13T09:00:00Z' })
    createSeenMarks(helm.storage).forHost('h-build').mark('2026-11-14T12:00:00Z')
    await vi.waitFor(() =>
      expect(helm.kept()).toEqual({ local: '2026-11-13T09:00:00Z', 'h-build': '2026-11-14T12:00:00Z' })
    )
  })

  it('drops the oldest marks past the most hosts it keeps', async () => {
    const many = Object.fromEntries(
      Array.from({ length: NOTICES_SEEN_HOSTS_MAX }, (_, at) => [
        `h${String(at)}`,
        new Date(Date.UTC(2026, 10, 1, 0, at)).toISOString()
      ])
    )
    const helm = device(many)
    const marks = createSeenMarks(helm.storage)
    await marks.forHost('new').read()
    marks.forHost('new').mark('2026-11-14T12:00:00Z')
    await vi.waitFor(() => expect(helm.writes).toHaveLength(1))
    const kept = helm.kept()
    expect(Object.keys(kept)).toHaveLength(NOTICES_SEEN_HOSTS_MAX)
    expect(kept['h0']).toBeUndefined()
    expect(kept['h1']).toBeDefined()
    expect(kept['new']).toBe('2026-11-14T12:00:00Z')
  })

  it('reads a mark it cannot read as none, and says so', async () => {
    const lines: string[] = []
    const marks = createSeenMarks(
      { read: () => Promise.reject(new Error('main is gone')), write: () => Promise.resolve() },
      (line) => lines.push(line)
    )
    await expect(marks.forHost('local').read()).resolves.toBeNull()
    expect(lines).toEqual(['notices: what this device has seen could not be read (main is gone)'])
  })
})
