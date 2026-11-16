import { describe, expect, it } from 'vitest'
import type { LocalHostStep } from '../../../../shared/ipc'
import {
  USABLE,
  createLocalHostUsable,
  localHostNotice,
  nextLocalHostUsable,
  usabilityOf,
  type LocalHostUsable
} from './localHostUsable'

const REFUSED: LocalHostStep = {
  step: 'refused',
  since: 1,
  sentence: 'This client speaks Floti protocol 4.1 and the host speaks 3.1. Update the host.',
  versions: { host: { major: 3, minor: 1 }, client: { major: 4, minor: 1 }, update: 'host' }
}
const NEWER: LocalHostStep = {
  step: 'refused',
  since: 1,
  sentence: 'This client speaks Floti protocol 3.1 and the host speaks 4.1. Update the client.',
  versions: { host: { major: 4, minor: 1 }, client: { major: 3, minor: 1 }, update: 'client' }
}
const UNREACHABLE: LocalHostStep = { step: 'unreachable', since: 1, reason: 'floti host did not answer within 30 seconds.' }
const UPDATING: LocalHostStep = { step: 'updating', since: 2, from: 'protocol 3.1', to: '3.0.0-preview.2' }
const at = (step: LocalHostStep['step']): LocalHostStep => ({ step, since: 3 }) as LocalHostStep

/** Where the host stands after each step in turn. */
const through = (...steps: LocalHostStep[]): LocalHostUsable => steps.reduce(nextLocalHostUsable, USABLE)

/** A step store the test drives. */
function steps() {
  let step: LocalHostStep | null = null
  const listeners = new Set<() => void>()
  return {
    get: () => step,
    subscribe(listener: () => void) {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    begin(next: LocalHostStep) {
      step = next
      for (const listener of [...listeners]) listener()
    }
  }
}

describe('whether this PC’s host can be used', () => {
  it('is usable while main starts and connects to it, before anything failed', () => {
    expect(through(at('preparing'), at('connecting'), at('starting'))).toEqual(USABLE)
    expect(through(at('connecting'), at('ready'))).toEqual(USABLE)
  })

  it('keeps a refusal or a host that could not be reached through the connects main tries after it, saying it tries again', () => {
    expect(through(REFUSED)).toEqual({ unusable: REFUSED, retrying: false, updating: null })
    expect(through(REFUSED, at('connecting'), at('starting'))).toEqual({ unusable: REFUSED, retrying: true, updating: null })
    expect(through(REFUSED, at('connecting'), UNREACHABLE)).toEqual({ unusable: UNREACHABLE, retrying: false, updating: null })
  })

  it('is usable again once a connect is ready', () => {
    expect(through(UNREACHABLE, at('connecting'), at('ready'))).toEqual(USABLE)
  })

  it('says main updates it, through the new host’s start, until it is ready or the update fails', () => {
    expect(through(REFUSED, at('connecting'), UPDATING, at('starting'))).toEqual({
      unusable: null,
      retrying: false,
      updating: UPDATING
    })
    expect(through(UPDATING, at('ready'))).toEqual(USABLE)
    expect(through(UPDATING, REFUSED).unusable).toBe(REFUSED)
  })

  it('tells its listeners only of a change, and the page link one value per refusal', () => {
    const driven = steps()
    const store = createLocalHostUsable(driven)
    const usability = usabilityOf(store)
    let told = 0
    store.subscribe(() => (told += 1))
    driven.begin(at('connecting'))
    expect(told).toBe(0)
    driven.begin(REFUSED)
    const why = usability.get()
    expect(why).toEqual({
      sentence: 'This client speaks Floti protocol 4.1 and the host speaks 3.1. Update the host.',
      refused: { host: { major: 3, minor: 1 }, client: { major: 4, minor: 1 }, update: 'host' }
    })
    driven.begin(at('connecting'))
    driven.begin(at('starting'))
    expect(told).toBe(2)
    expect(usability.get()).toBe(why)
    driven.begin(UNREACHABLE)
    expect(usability.get()).toEqual({ sentence: 'floti host did not answer within 30 seconds.' })
    driven.begin(at('ready'))
    expect(usability.get()).toBeNull()
  })
})

describe('the one notice about this PC’s host', () => {
  it('offers Try again for a host older than this Floti that was not updated, and for one that could not be reached', () => {
    expect(localHostNotice(through(REFUSED), null)).toEqual({
      kind: 'refused',
      sentence: 'This client speaks Floti protocol 4.1 and the host speaks 3.1. Update the host.',
      fix: 'try-again',
      retrying: false
    })
    expect(localHostNotice(through(UNREACHABLE, at('connecting')), null)).toEqual({
      kind: 'unreachable',
      reason: 'floti host did not answer within 30 seconds.',
      retrying: true
    })
  })

  it('offers updating Floti for a host newer than it', () => {
    expect(localHostNotice(through(NEWER), null)).toMatchObject({ kind: 'refused', fix: 'update-floti' })
  })

  it('says the host is being updated over anything else, and that it stopped only when there is nothing else to say', () => {
    expect(localHostNotice(through(UPDATING), { restarts: true })).toEqual({ kind: 'updating', to: '3.0.0-preview.2' })
    expect(localHostNotice(through(REFUSED), { restarts: true })).toMatchObject({ kind: 'refused' })
    expect(localHostNotice(USABLE, { restarts: false })).toEqual({ kind: 'lost', restarts: false })
    expect(localHostNotice(USABLE, null)).toBeNull()
  })
})
