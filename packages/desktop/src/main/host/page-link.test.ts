import { describe, expect, it } from 'vitest'
import type { EventPayload } from '../../shared/ipc'
import { FakeConnection, fakeLocalHost } from '../../../test/host-connection'
import { HOST_RETRY_MS, LocalHostRefusedError, type PageBridge } from './launcher'
import { createHostLink } from './link'
import { FOREIGN_PAGE, createPageLinks, type FlotiPage, type PageLinksDeps } from './page-link'

class FakeBridge implements PageBridge {
  readonly written: Uint8Array[] = []
  endedByDesktop = false
  private settle: (why: string | null) => void = () => undefined
  readonly ended = new Promise<string | null>((resolve) => {
    this.settle = resolve
  })
  constructor(readonly data: (bytes: Uint8Array) => void) {}
  write(bytes: Uint8Array): void {
    this.written.push(bytes)
  }
  end(): void {
    this.endedByDesktop = true
    this.settle(null)
  }
  /** The bridge exits on its own. */
  exit(why: string | null): void {
    this.settle(why)
  }
}

class FakePage implements FlotiPage {
  readonly sent: Array<{ channel: string; payload: unknown }> = []
  private gone: Array<() => void> = []
  constructor(readonly id: number) {}
  send<K extends 'hostLink:data' | 'hostLink:ended'>(channel: K, payload: EventPayload<K>): void {
    this.sent.push({ channel, payload })
  }
  onGone(listener: () => void): void {
    this.gone.push(listener)
  }
  /** Navigates, reloads or closes. */
  leave(): void {
    for (const listener of this.gone) listener()
  }
}

const WINDOW = 1
const BACKGROUND = 2

function world(overrides: Partial<PageLinksDeps> = {}) {
  const bridges: FakeBridge[] = []
  const links = createPageLinks({
    appVersion: '2.9.0',
    isFlotiPage: (id) => id === WINDOW || id === BACKGROUND,
    ready: () => Promise.resolve(),
    howEnded: () => ({ planned: false, restarts: true }),
    openBridge: (data) => {
      const bridge = new FakeBridge(data)
      bridges.push(bridge)
      return bridge
    },
    log: () => undefined,
    ...overrides
  })
  return { links, bridges }
}

describe('each Floti page’s own connection to the local host', () => {
  it('refuses a page that is not one of Floti’s own, and starts no bridge for it', async () => {
    const w = world()
    await expect(w.links.open(new FakePage(77))).rejects.toThrow(FOREIGN_PAGE)
    expect(w.bridges).toEqual([])
  })

  it('drops bytes a foreign page sends on a Floti page’s connection', async () => {
    const w = world()
    const opened = await w.links.open(new FakePage(WINDOW))
    if (!('connection' in opened)) throw new Error('not opened')
    w.links.send(77, opened.connection, Uint8Array.of(1))
    w.links.close(77, opened.connection)
    expect(w.bridges[0]?.written).toEqual([])
    expect(w.bridges[0]?.endedByDesktop).toBe(false)
  })

  it('passes bytes both ways unread, under the connection’s number', async () => {
    const w = world()
    const page = new FakePage(WINDOW)
    const opened = await w.links.open(page)
    expect(opened).toEqual({ connection: 1, appVersion: '2.9.0' })
    const frame = Uint8Array.of(0, 0, 0, 1, 0xa0)
    w.links.send(WINDOW, 1, frame)
    expect(w.bridges[0]?.written).toEqual([frame])
    const reply = Uint8Array.of(9, 8, 7)
    w.bridges[0]?.data(reply)
    expect(page.sent).toEqual([{ channel: 'hostLink:data', payload: { connection: 1, bytes: reply } }])
  })

  it('opens a page’s bridge only after main’s own link has connected, and says why when it could not', async () => {
    let connect: () => void = () => undefined
    const w = world({ ready: () => new Promise<void>((resolve) => (connect = resolve)) })
    const opening = w.links.open(new FakePage(WINDOW))
    await Promise.resolve()
    expect(w.bridges).toEqual([])
    connect()
    await opening
    expect(w.bridges).toHaveLength(1)

    const sentence = 'No local host answered on its socket.'
    const failing = world({ ready: () => Promise.reject(new Error(sentence)) })
    expect(await failing.links.open(new FakePage(WINDOW))).toEqual({ unreachable: sentence })
    expect(failing.bridges).toEqual([])
  })

  it('ends the page’s bridge when it reloads, navigates or closes, and when it opens again', async () => {
    const w = world()
    const page = new FakePage(WINDOW)
    await w.links.open(page)
    page.leave()
    expect(w.bridges[0]?.endedByDesktop).toBe(true)

    await w.links.open(page)
    await w.links.open(page)
    expect(w.bridges[1]?.endedByDesktop).toBe(true)
    expect(w.bridges[2]?.endedByDesktop).toBe(false)
    // The page that left was not told: it is gone.
    expect(page.sent.filter((s) => s.channel === 'hostLink:ended')).toEqual([])
  })

  it('tells the page when its bridge ends on its own, with the sentence', async () => {
    const w = world()
    const page = new FakePage(BACKGROUND)
    await w.links.open(page)
    w.bridges[0]?.exit('The local host closed the connection; turn off …')
    await w.bridges[0]?.ended
    await Promise.resolve()
    expect(page.sent).toEqual([
      {
        channel: 'hostLink:ended',
        payload: {
          connection: 1,
          why: 'The local host closed the connection; turn off …',
          planned: false,
          restarts: true
        }
      }
    ])
    // Its bytes go nowhere now.
    w.links.send(BACKGROUND, 1, Uint8Array.of(1))
    expect(w.bridges[0]?.written).toEqual([])
  })

  it('tells the page a bridge that ended with a host Floti stopped on purpose was no crash, and whether Floti starts another', async () => {
    let how = { planned: true, restarts: false }
    const w = world({ howEnded: () => how })
    const page = new FakePage(WINDOW)
    await w.links.open(page)
    w.bridges[0]?.exit(null)
    await w.bridges[0]?.ended
    await Promise.resolve()
    how = { planned: false, restarts: false }
    await w.links.open(page)
    w.bridges[1]?.exit('The local host closed the connection; turn off …')
    await w.bridges[1]?.ended
    await Promise.resolve()

    expect(page.sent.filter((s) => s.channel === 'hostLink:ended').map((s) => s.payload)).toEqual([
      { connection: 1, why: null, planned: true, restarts: false },
      {
        connection: 2,
        why: 'The local host closed the connection; turn off …',
        planned: false,
        restarts: false
      }
    ])
  })

  it('ends every bridge at quit, and opens none after', async () => {
    const w = world()
    await w.links.open(new FakePage(WINDOW))
    await w.links.open(new FakePage(BACKGROUND))
    await w.links.stop()
    expect(w.bridges.map((b) => b.endedByDesktop)).toEqual([true, true])
    expect(await w.links.open(new FakePage(WINDOW))).toEqual({ unreachable: 'Floti is quitting.' })
    expect(w.bridges).toHaveLength(2)
  })
})

describe('page opens while this PC’s host cannot be used', () => {
  it('AC5: every page open waits on main’s link, starts no connect or host of its own, and is answered once a retry reaches a host', async () => {
    const timers = new Map<number, () => void>()
    let nextTimer = 1
    let starts = 0
    let usable = false
    const link = createHostLink({
      start: () => {
        starts += 1
        return usable
          ? Promise.resolve(fakeLocalHost(new FakeConnection()))
          : Promise.reject(
              new LocalHostRefusedError('This client speaks Floti protocol 4.1 and the host speaks 3.1. Update the host.', {
                host: { major: 3, minor: 1 },
                client: { major: 4, minor: 1 },
                update: 'host'
              })
            )
      },
      retryFailedConnects: true,
      log: () => undefined,
      setTimeout: (run, ms) => {
        expect(ms).toBe(HOST_RETRY_MS)
        const id = nextTimer++
        timers.set(id, run)
        return id
      },
      clearTimeout: (timer) => void timers.delete(timer as number)
    })
    // Main's first connect at start-up is refused.
    await expect(link.connection()).rejects.toThrow('Update the host.')
    expect(starts).toBe(1)

    const w = world({ ready: () => link.usable() })
    const window = new FakePage(WINDOW)
    const background = new FakePage(BACKGROUND)
    // The window opens, its reads open again after a reload, the plugin window opens too.
    const opened = [w.links.open(window), w.links.open(background), w.links.open(new FakePage(WINDOW))]
    for (let i = 0; i < 30; i++) await Promise.resolve()
    expect(starts).toBe(1)
    expect(w.bridges).toEqual([])

    // Main's own retry reaches a host that welcomes this Floti.
    usable = true
    const due = [...timers.values()]
    timers.clear()
    for (const run of due) run()
    const answers = await Promise.all(opened)
    expect(starts).toBe(2)
    // The window's first open was ended by its second; the others each have a bridge.
    expect(answers.map((answer) => 'connection' in answer)).toEqual([false, true, true])
    expect(w.bridges).toHaveLength(2)
  })
})
