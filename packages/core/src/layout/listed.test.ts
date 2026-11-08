import { describe, expect, it } from 'vitest'
import type { ChildReport } from '../types'
import { unlistedOf, withListed, withListedOf } from './listed'

const at = '2026-11-06T10:00:00.000Z'
const report = (state: string, text = 'Two bugs in ledger.rs.'): ChildReport => ({ state, text, at })
/** A row as a window holds it: some fields of its own, and a report or title the host may have moved on from. */
interface Row {
  id: number
  label?: string
  lastReport?: ChildReport
  title?: string
  kept?: boolean
}

describe('withListedOf', () => {
  it('keeps the very same row when the report and the title agree', () => {
    const known: Row = { id: 4, label: 'mine', lastReport: report('done'), title: 'Fix the login form' }
    expect(withListedOf(known, { lastReport: report('done'), title: 'Fix the login form' })).toBe(known)
    const bare: Row = { id: 4, label: 'mine' }
    expect(withListedOf(bare, {})).toBe(bare)
  })

  it('takes the report the host lists and leaves the rest of the row as the window has it', () => {
    const known: Row = { id: 4, label: 'renamed here' }
    const taken = withListedOf(known, { lastReport: report('stuck') })
    expect(taken).toEqual({ id: 4, label: 'renamed here', lastReport: report('stuck') })
    expect(known).not.toHaveProperty('lastReport')
  })

  it('goes without a report the host no longer lists', () => {
    const taken = withListedOf({ id: 4, lastReport: report('done') }, {})
    expect(taken).toEqual({ id: 4 })
    expect(taken).not.toHaveProperty('lastReport')
  })

  it('takes the title its agent gave the session, and a new one when the work changes', () => {
    const known: Row = { id: 4, label: 'renamed here' }
    const titled = withListedOf(known, { title: 'Agents page: edit and delete agents' })
    expect(titled).toEqual({ id: 4, label: 'renamed here', title: 'Agents page: edit and delete agents' })
    expect(known).not.toHaveProperty('title')
    const retitled = withListedOf(titled, { title: 'Agents page: tests' })
    expect(retitled.title).toBe('Agents page: tests')
    expect(retitled.label).toBe('renamed here')
  })
})

describe('kept', () => {
  it('takes kept from the listing, and leaves the very same row while it agrees', () => {
    const known: Row = { id: 4, label: 'mine' }
    const kept = withListedOf(known, { kept: true })
    expect(kept).toEqual({ id: 4, label: 'mine', kept: true })
    expect(withListedOf(kept, { kept: true })).toBe(kept)
    expect(withListedOf(known, { kept: false })).toBe(known)
    const removed = withListedOf(kept, {})
    expect(removed).toEqual({ id: 4, label: 'mine' })
    expect(removed).not.toHaveProperty('kept')
  })

  it('is no longer kept when a listing does not name the row, and the rest of it stays', () => {
    const kept: Row = { id: 4, label: 'mine', kept: true }
    expect(unlistedOf(kept)).toEqual({ id: 4, label: 'mine' })
    const plain: Row = { id: 5 }
    expect(unlistedOf(plain)).toBe(plain)
  })

  it('stops keeping a held row the listing has left out, and changes nothing for one that was not kept', () => {
    const kept = { id: 2, kept: true }
    const plain = { id: 3 }
    expect(withListed([kept, plain], [{ id: 1 }])).toEqual([{ id: 2 }, plain])
    expect(withListed([plain], [])).toBeNull()
  })
})

describe('withListed', () => {
  it('returns null when no report or title changed, so the sessions the window holds stay as they are', () => {
    const current = [{ id: 1 }, { id: 2, lastReport: report('done') }, { id: 3, title: 'Fix the login form' }]
    expect(
      withListed(current, [
        { id: 1 },
        { id: 2, lastReport: report('done') },
        { id: 3, title: 'Fix the login form' },
        { id: 4 }
      ])
    ).toBeNull()
  })

  it('replaces only the rows whose report or title changed, and ignores a listed row it does not hold', () => {
    const one = { id: 1 }
    const two = { id: 2, lastReport: report('done') }
    const three = { id: 3, lastReport: report('note') }
    const four = { id: 4 }
    const next = withListed(
      [one, two, three, four],
      [
        { id: 2 },
        { id: 3, lastReport: report('note') },
        { id: 4, title: 'Fix the login form' },
        { id: 9, lastReport: report('done') }
      ]
    )!
    expect(next).toEqual([one, { id: 2 }, three, { id: 4, title: 'Fix the login form' }])
    expect(next[0]).toBe(one)
    expect(next[2]).toBe(three)
  })
})
