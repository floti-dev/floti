import { describe, expect, it } from 'vitest'
import type { ChildReport } from '../types'
import { reportNote, sameReport } from './familyReport'
import { withListedOf } from './listed'

const at = '2026-11-06T10:00:00.000Z'
const report = (state: string, text = 'Two bugs in ledger.rs.'): ChildReport => ({ state, text, at })
const running = (lastReport?: ChildReport): { status: string; lastReport?: ChildReport } => ({
  status: 'running',
  ...(lastReport === undefined ? {} : { lastReport })
})

describe('reportNote', () => {
  it('says "Reported done" with all of a done report', () => {
    expect(reportNote(running(report('done')))).toEqual({
      label: 'Reported done',
      text: 'Two bugs in ledger.rs.',
      at
    })
  })

  it('says "Reported stuck" for a stuck report and "Note" for a note', () => {
    expect(reportNote(running(report('stuck', 'Which table, A or B?')))?.label).toBe('Reported stuck')
    expect(reportNote(running(report('note', 'Half way through.')))?.label).toBe('Note')
  })

  it('calls a report of a state it has no word for a note, and still shows its text', () => {
    expect(reportNote(running(report('paused')))).toMatchObject({ label: 'Note', text: 'Two bugs in ledger.rs.' })
  })

  it('has no note for a child that has reported nothing, or nothing but spaces', () => {
    expect(reportNote(running())).toBeNull()
    expect(reportNote(running(report('done', '  \n ')))).toBeNull()
  })

  it('has no note once the host has cleared the report, as it does when the child’s next turn starts', () => {
    const before = running(report('done'))
    expect(reportNote(before)).not.toBeNull()
    expect(reportNote(withListedOf(before, {}))).toBeNull()
  })

  it('has no note for a child that has ended, whatever its row still says', () => {
    for (const status of ['exited', 'lost']) {
      expect(reportNote({ status, lastReport: report('done') })).toBeNull()
    }
  })

  it('never has a note for a command child, which runs a program and not an agent', () => {
    expect(reportNote(running(report('done')), { command: true })).toBeNull()
    expect(reportNote(running(report('done')), { command: false })).not.toBeNull()
  })
})

describe('sameReport', () => {
  it('compares what was said and when', () => {
    expect(sameReport(undefined, undefined)).toBe(true)
    expect(sameReport(report('done'), report('done'))).toBe(true)
    expect(sameReport(report('done'), undefined)).toBe(false)
    expect(sameReport(undefined, report('done'))).toBe(false)
    expect(sameReport(report('done'), report('stuck'))).toBe(false)
    expect(sameReport(report('done'), report('done', 'Another.'))).toBe(false)
    expect(sameReport(report('done'), { ...report('done'), at: '2026-11-06T10:05:00.000Z' })).toBe(false)
  })
})
