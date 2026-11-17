import { describe, expect, it } from 'vitest'
import { HostRefusedError } from '../../../../shared/host/client'
import { FrameDecoder, encodeFrame } from '../../../../shared/host/frame'
import { PROTOCOL_VERSION, type Message } from '../../../../shared/host/protocol'
import { RECONNECT_FIRST_MS } from '../../../../shared/host/reconnect'
import type { EventChannel, EventPayload, HostLinkOpened } from '../../../../shared/ipc'
import {
  HOST_UNUSABLE_FOR_ACTIONS,
  WINDOW_CAPABILITIES,
  createPageLink,
  type HostEnd,
  type HostUnusable,
  type HostUsability,
  type PageLinkDeps
} from './link'

const UNREACHABLE = 'No local host answered on its socket.'

/**
 * Main and the host behind it, as the page sees them over `hostLink:*`: each
 * open answers what the test said, and each connection's frames are read by a
 * host that welcomes the hello and answers `history:summary`.
 */
class FakeMain {
  readonly opens: HostLinkOpened[] = []
  readonly hellos: Message[] = []
  readonly closed: number[] = []
  /** Every request the host was sent, with the connection it came on. */
  readonly requests: Array<{ connection: number; channel: string }> = []
  /** Whether the host answers requests; when false they wait, as on a link that is about to drop. */
  answers = true
  /** How many times the page asked main's link to drop its wait (`hostLink:reconnect`). */
  reconnects = 0
  /** Every open the page made, answered or still held by main. */
  openCount = 0
  /**
   * Whether main holds each open until its own link has a host, as it does
   * while this PC's host cannot be used; `release` answers them.
   */
  holdOpens = false
  private readonly heldOpens: Array<() => void> = []
  private next = 1
  private readonly listeners = new Map<string, Set<(payload: never) => void>>()
  private readonly decoders = new Map<number, FrameDecoder>()
  answer: () => HostLinkOpened = () => ({ connection: this.next++, appVersion: '2.9.0' })

  readonly bridge: PageLinkDeps['bridge'] = {
    invoke: ((channel: string) => {
      if (channel === 'hostLink:reconnect') {
        this.reconnects += 1
        return Promise.resolve()
      }
      if (channel !== 'hostLink:open') return Promise.reject(new Error(`unexpected ${channel}`))
      this.openCount += 1
      if (this.holdOpens) {
        return new Promise<HostLinkOpened>((resolve) => {
          this.heldOpens.push(() => {
            const opened = this.answer()
            this.opens.push(opened)
            resolve(opened)
          })
        })
      }
      const opened = this.answer()
      this.opens.push(opened)
      return Promise.resolve(opened)
    }) as PageLinkDeps['bridge']['invoke'],
    send: ((channel: string, payload: { connection: number; bytes?: Uint8Array }) => {
      if (channel === 'hostLink:close') {
        this.closed.push(payload.connection)
        return
      }
      if (channel !== 'hostLink:send' || payload.bytes === undefined) return
      const decoder = this.decoders.get(payload.connection) ?? new FrameDecoder()
      this.decoders.set(payload.connection, decoder)
      decoder.push(payload.bytes)
      for (let next = decoder.next(); next !== null; next = decoder.next()) {
        if ('malformed' in next) throw next.malformed
        this.received(payload.connection, next.message)
      }
    }) as PageLinkDeps['bridge']['send'],
    on: ((channel: string, listener: (payload: never) => void) => {
      const set = this.listeners.get(channel) ?? new Set()
      set.add(listener)
      this.listeners.set(channel, set)
      return () => set.delete(listener)
    }) as PageLinkDeps['bridge']['on']
  }

  /** Main's link has a host: every held open is answered, and opens are answered at once from now on. */
  release(): void {
    this.holdOpens = false
    for (const answer of this.heldOpens.splice(0)) answer()
  }

  /** Whether the host welcomes the hello; otherwise the bridge ends with `why`, or the host refuses it. */
  welcome: { why: string } | { refused: Message } | true = true

  private received(connection: number, message: Message): void {
    if (message.type === 'hello') {
      this.hellos.push(message)
      if (this.welcome === true) {
        this.reply(connection, {
          type: 'welcome',
          protocol: { major: PROTOCOL_VERSION.major, minor: PROTOCOL_VERSION.minor },
          hostVersion: '1.0.0',
          capabilities: ['terminal'],
          backends: []
        })
      } else if ('refused' in this.welcome) {
        this.reply(connection, this.welcome.refused)
      } else {
        this.end(connection, this.welcome.why)
      }
      return
    }
    if (message.type !== 'request') return
    this.requests.push({ connection, channel: message.channel })
    if (this.answers) this.reply(connection, { type: 'response', id: message.id, payload: 'answered' })
  }

  private reply(connection: number, message: Message): void {
    queueMicrotask(() => this.emit('hostLink:data', { connection, bytes: encodeFrame(message) }))
  }

  emit<K extends EventChannel>(channel: K, payload: EventPayload<K>): void {
    for (const listener of this.listeners.get(channel) ?? []) (listener as (payload: EventPayload<K>) => void)(payload)
  }

  /** The bridge ends, as main says when it exits: by default the host went without a stop, and the desktop starts another. */
  end(connection: number, why: string | null, how: HostEnd = { planned: false, restarts: true }): void {
    queueMicrotask(() => this.emit('hostLink:ended', { connection, why, ...how }))
  }
}

/** Timers the test fires by hand. */
function steppedTimers() {
  const pending = new Map<number, { run: () => void; ms: number }>()
  let next = 1
  return {
    setTimeout: (run: () => void, ms: number): unknown => {
      const id = next++
      pending.set(id, { run, ms })
      return id
    },
    clearTimeout: (timer: unknown) => {
      pending.delete(timer as number)
    },
    fireAll() {
      const due = [...pending.values()]
      pending.clear()
      for (const timer of due) timer.run()
    },
    pending: () => [...pending.values()].map((timer) => timer.ms)
  }
}

/** Lets every promise already settled run its callbacks. */
async function settle(): Promise<void> {
  for (let i = 0; i < 30; i++) await Promise.resolve()
}

/** Main's word on this PC's host, set by the test. */
function usabilityStore() {
  let why: HostUnusable | null = null
  const listeners = new Set<() => void>()
  const store: HostUsability & { set(next: HostUnusable | null): void } = {
    get: () => why,
    subscribe(listener) {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    set(next) {
      why = next
      for (const listener of [...listeners]) listener()
    }
  }
  return store
}

/** This PC's host's page link by default; a host on another computer's with `remote`. */
function world(options: { remote?: boolean } = {}) {
  const main = new FakeMain()
  const timers = steppedTimers()
  const usability = usabilityStore()
  const link = createPageLink({
    bridge: main.bridge,
    capabilities: WINDOW_CAPABILITIES,
    ...(options.remote === true ? { host: 'build-box', retries: false } : { usability }),
    ...timers
  })
  return { main, link, timers, usability }
}

const REFUSED_VERSIONS = { host: { major: 3, minor: 1 }, client: { major: 4, minor: 1 }, update: 'host' as const }
const REFUSED_SENTENCE = 'This client speaks Floti protocol 4.1 and the host speaks 3.1. Update the host.'

describe('a Floti page’s own connection to the local host', () => {
  it('connects on its first open, saying what the window can do', async () => {
    const w = world()
    await w.link.connection()
    expect(w.main.opens).toHaveLength(1)
    expect(w.main.hellos).toEqual([
      {
        type: 'hello',
        protocol: { major: PROTOCOL_VERSION.major, minor: PROTOCOL_VERSION.minor },
        client: { name: 'Floti 2.9.0', kind: 'desktop' },
        capabilities: ['terminal', 'files', 'pluginPages']
      }
    ])
  })

  it('never declares the browser: the window’s connection answers no browser tool', () => {
    expect(WINDOW_CAPABILITIES).not.toContain('browser')
  })

  it('has every caller join one connect, so a read asked before it settled waits for the host’s answer', async () => {
    const w = world()
    const subscribed: string[] = []
    w.link.onConnected(() => subscribed.push('connected'))
    const [a, b, c] = await Promise.all([w.link.connection(), w.link.connection(), w.link.connection()])
    expect(a).toBe(b)
    expect(b).toBe(c)
    expect(w.main.opens).toHaveLength(1)
    expect(subscribed).toEqual(['connected'])
    expect(await a.request('history:summary', null)).toBe('answered')
  })

  it('S5: rejects with main’s sentence when main answers that nothing will connect, never answering empty', async () => {
    const w = world()
    w.main.answer = () => ({ unreachable: UNREACHABLE })
    await expect(w.link.connection()).rejects.toThrow(UNREACHABLE)
    // And asks again the next time, as main's link does.
    await expect(w.link.connection()).rejects.toThrow(UNREACHABLE)
    expect(w.main.opens).toHaveLength(2)
  })

  it('a remote host’s link rejects with the bridge’s sentence when it ends before the host welcomed the page', async () => {
    const w = world({ remote: true })
    const why = 'The local host did not prove the key Floti expected, so Floti did not connect to it.'
    w.main.welcome = { why }
    await expect(w.link.connection()).rejects.toThrow(why)
  })

  it('S5: a remote host’s link rejects with the host’s refusal of the hello ending at its cause, naming no setting', async () => {
    const w = world({ remote: true })
    w.main.welcome = {
      refused: {
        type: 'refused',
        host: { major: 2, minor: 0 },
        client: { major: 1, minor: 0 },
        update: 'client',
        message: 'This host speaks protocol 2.0 and Floti speaks 1.0: update Floti.'
      }
    }
    const error = await w.link.connection().then(
      () => null,
      (e: unknown) => e as Error
    )
    expect(error?.message).toBe('This host speaks protocol 2.0 and Floti speaks 1.0: update Floti.')
    expect(error?.message).not.toMatch(/Settings|own sessions/)
  })

  it('connects again when wanted after the connection ended', async () => {
    const w = world()
    const first = await w.link.connection()
    w.main.end(1, null)
    expect(await first.ended).toBe('The connection to the local host ended.')
    const second = await w.link.connection()
    expect(second).not.toBe(first)
    expect(w.main.opens).toHaveLength(2)
  })

  it('connects again by itself after the host went without a stop, waiting twice as long after each failed try', async () => {
    const w = world()
    const ends: HostEnd[] = []
    let connects = 0
    w.link.onEnded((end) => ends.push(end))
    w.link.onConnected(() => (connects += 1))
    await w.link.connection()

    w.main.end(1, 'The local host closed the connection.', {
      planned: false,
      restarts: true
    })
    await settle()
    expect(ends).toEqual([{ planned: false, restarts: true }])

    // Main cannot reach a host yet.
    w.main.answer = () => ({ unreachable: UNREACHABLE })
    const waits: number[] = []
    for (let tries = 0; tries < 3; tries++) {
      waits.push(...w.timers.pending())
      w.timers.fireAll()
      await settle()
    }
    expect(waits).toEqual([RECONNECT_FIRST_MS, RECONNECT_FIRST_MS * 2, RECONNECT_FIRST_MS * 4])

    let next = 10
    w.main.answer = () => ({
      connection: next++,
      appVersion: '2.9.0'
    })
    w.timers.fireAll()
    await settle()
    expect(connects).toBe(2)
    expect(w.timers.pending()).toEqual([])
    expect(await (await w.link.connection()).request('history:summary', null)).toBe('answered')
    // One open per try: the first, three that failed, and the one that connected.
    expect(w.main.opens).toHaveLength(5)
  })

  it('S8: Reconnect now drops the wait and connects at once, and the next wait starts short again', async () => {
    const w = world()
    let connects = 0
    w.link.onConnected(() => (connects += 1))
    await w.link.connection()
    w.main.end(1, 'Connection to build-box closed by remote host.', { planned: false, restarts: false })
    await settle()
    w.main.answer = () => ({ unreachable: 'build-box is not connected.' })
    for (let tries = 0; tries < 3; tries++) {
      w.timers.fireAll()
      await settle()
    }
    expect(w.timers.pending()).toEqual([RECONNECT_FIRST_MS * 8])

    // Still not reachable: tried at once, and the wait after starts from the first.
    w.link.reconnectNow()
    await settle()
    expect(w.main.opens).toHaveLength(5)
    expect(w.timers.pending()).toEqual([RECONNECT_FIRST_MS])

    w.main.answer = () => ({ connection: 20, appVersion: '2.9.0' })
    w.link.reconnectNow()
    await settle()
    expect(connects).toBe(2)
    expect(w.timers.pending()).toEqual([])
    // Connected: there is nothing to reconnect.
    w.link.reconnectNow()
    await settle()
    expect(w.main.opens).toHaveLength(6)
  })

  it('S9: a request made before the link dropped fails and is never sent again on the next connection', async () => {
    const w = world()
    const first = await w.link.connection()
    w.main.answers = false
    const asked = first.request('session:close', { id: 7, force: true })
    await settle()
    expect(w.main.requests).toEqual([{ connection: 1, channel: 'session:close' }])

    w.main.end(1, 'Connection to build-box closed by remote host.', { planned: false, restarts: false })
    await expect(asked).rejects.toThrow()
    w.main.answers = true
    w.timers.fireAll()
    await settle()
    const second = await w.link.connection()
    expect(second).not.toBe(first)
    await second.request('history:summary', null)
    // On the new connection: only what was asked on it.
    expect(w.main.requests.filter((request) => request.connection !== 1)).toEqual([
      { connection: 2, channel: 'history:summary' }
    ])
  })

  it('does not connect by itself after main stopped the host on purpose, and says it was on purpose', async () => {
    const w = world()
    const ends: HostEnd[] = []
    w.link.onEnded((end) => ends.push(end))
    await w.link.connection()
    w.main.end(1, null, { planned: true, restarts: false })
    await settle()

    expect(ends).toEqual([{ planned: true, restarts: false }])
    expect(w.timers.pending()).toEqual([])
    expect(w.main.opens).toHaveLength(1)
  })

  it('S12: after a Restart host to update it connects to the new host at once, without waiting', async () => {
    const w = world()
    const ends: HostEnd[] = []
    let connects = 0
    w.link.onEnded((end) => ends.push(end))
    w.link.onConnected(() => (connects += 1))
    await w.link.connection()

    w.main.end(1, null, { planned: true, restarts: true, restarting: true })
    await settle()

    expect(ends).toEqual([{ planned: true, restarts: true, restarting: true }])
    expect(w.timers.pending()).toEqual([])
    expect(w.main.opens).toHaveLength(2)
    expect(connects).toBe(2)
  })

  it('S7: a refusal of this PC’s host keeps both versions and the side to update, told once from main’s word and again from the host', async () => {
    const w = world()
    const refusals: unknown[] = []
    w.link.onRefused?.((refused) => refusals.push(refused))

    // Main's own link was refused: its step says so, once, whatever else changes.
    w.usability.set({ sentence: REFUSED_SENTENCE, refused: REFUSED_VERSIONS })
    w.usability.set(w.usability.get())
    expect(refusals).toEqual([REFUSED_VERSIONS])
    // A host that cannot be reached is no refusal.
    w.usability.set({ sentence: UNREACHABLE })
    expect(refusals).toEqual([REFUSED_VERSIONS])

    // The page's own hello was refused, after main had a host.
    w.main.welcome = { refused: { type: 'refused', ...REFUSED_VERSIONS, message: REFUSED_SENTENCE } }
    void w.link.connection()
    await settle()
    expect(refusals).toEqual([REFUSED_VERSIONS, REFUSED_VERSIONS])
  })

  it('AC5: while this PC’s host cannot be used every read waits on one open main holds, and none starts another', async () => {
    const w = world()
    w.main.holdOpens = true
    const reads = [w.link.connection(), w.link.connection(), w.link.connection()]
    await settle()
    reads.push(w.link.connection(), w.link.connection())
    await settle()
    expect(w.main.openCount).toBe(1)
    expect(w.timers.pending()).toEqual([])
    let answered = 0
    for (const read of reads) void read.then(() => (answered += 1))
    await settle()
    expect(answered).toBe(0)

    // Main's link reaches a host that welcomes this Floti.
    w.main.release()
    const connections = await Promise.all(reads)
    expect(new Set(connections).size).toBe(1)
    expect(w.main.openCount).toBe(1)
  })

  it('AC5: a page bridge refused after main connected holds the reads and tries again after the link’s waits; Try again asks main and tries at once', async () => {
    const w = world()
    let connects = 0
    w.link.onConnected(() => (connects += 1))
    w.main.welcome = { refused: { type: 'refused', ...REFUSED_VERSIONS, message: REFUSED_SENTENCE } }
    let settled = false
    const read = w.link.connection()
    void read.then(
      () => (settled = true),
      () => (settled = true)
    )
    await settle()
    expect(settled).toBe(false)
    expect(w.timers.pending()).toEqual([RECONNECT_FIRST_MS])
    w.timers.fireAll()
    await settle()
    expect(w.main.openCount).toBe(2)
    expect(w.timers.pending()).toEqual([RECONNECT_FIRST_MS * 2])
    // Reads asked meanwhile join the held attempt.
    void w.link.connection()
    await settle()
    expect(w.main.openCount).toBe(2)

    // The host that replaced it welcomes this Floti; Try again tries at once.
    w.main.welcome = true
    w.link.reconnectNow()
    await settle()
    expect(w.main.reconnects).toBe(1)
    expect(w.main.openCount).toBe(3)
    expect(connects).toBe(1)
    expect(settled).toBe(true)
    expect(w.timers.pending()).toEqual([])
  })

  it('Try again on this PC’s host asks main’s link to drop its wait; a remote host’s asks nothing of main', () => {
    const local = world()
    local.link.reconnectNow()
    expect(local.main.reconnects).toBe(1)
    const remote = world({ remote: true })
    remote.link.reconnectNow()
    expect(remote.main.reconnects).toBe(0)
  })

  it('a read with something of its own to show is told why at once while this PC’s host cannot be used, or as soon as main says so', async () => {
    const w = world()
    w.usability.set({ sentence: REFUSED_SENTENCE, refused: REFUSED_VERSIONS })
    const refused = await w.link.connectionIfUsable().then(
      () => null,
      (error: unknown) => error
    )
    expect(refused).toBeInstanceOf(HostRefusedError)
    expect((refused as HostRefusedError).message).toBe(REFUSED_SENTENCE)
    expect(w.main.openCount).toBe(0)

    // Connecting when it asked, and main then finds no host.
    w.usability.set(null)
    w.main.holdOpens = true
    const asked = w.link.connectionIfUsable()
    await settle()
    w.usability.set({ sentence: UNREACHABLE })
    await expect(asked).rejects.toThrow(UNREACHABLE)

    // Usable: it is the connection.
    w.usability.set(null)
    w.main.release()
    await expect(w.link.connectionIfUsable()).resolves.toBe(await w.link.connection())
  })

  it('an action fails at once with the short sentence while this PC’s host cannot be used, or as soon as main says so, and is the connection once it can be', async () => {
    const w = world()
    w.usability.set({ sentence: REFUSED_SENTENCE, refused: REFUSED_VERSIONS })
    await expect(w.link.connectionForAction()).rejects.toThrow(HOST_UNUSABLE_FOR_ACTIONS)
    expect(w.main.openCount).toBe(0)

    w.usability.set(null)
    w.main.holdOpens = true
    const asked = w.link.connectionForAction()
    await settle()
    w.usability.set({ sentence: UNREACHABLE })
    await expect(asked).rejects.toThrow(HOST_UNUSABLE_FOR_ACTIONS)

    w.usability.set(null)
    w.main.release()
    await expect(w.link.connectionForAction()).resolves.toBe(await w.link.connection())
  })

  it('an action is told main’s own word when main says nothing will connect, not the notice’s', async () => {
    const w = world()
    w.main.answer = () => ({ unreachable: 'Floti is quitting.' })
    await expect(w.link.connectionForAction()).rejects.toThrow('Floti is quitting.')
  })

  it('takes a connection the page closed itself for no loss of the host', async () => {
    const w = world()
    const ends: HostEnd[] = []
    w.link.onEnded((end) => ends.push(end))
    const connection = await w.link.connection()
    connection.close()
    await settle()

    expect(ends).toEqual([])
    expect(w.timers.pending()).toEqual([])
  })

  it('tells main when it closes its connection', async () => {
    const w = world()
    const connection = await w.link.connection()
    connection.close()
    expect(w.main.closed).toEqual([1])
  })
})
