import { describe, expect, it } from 'vitest'
import type { SetupClaudeStatus } from './claudeStatus'
import { claudeBanner } from './claudeBanner'

const status = (over: Partial<SetupClaudeStatus>): SetupClaudeStatus => ({
  path: '/bin/claude',
  source: 'discovered',
  version: '2.1.225',
  semver: '2.1.225',
  tested: true,
  testedRange: { min: '2.1.0', max: '2.2.0' },
  configDir: '/home/you/.claude',
  configDirExists: true,
  auth: 'authenticated',
  authSignal: 'credentials present',
  error: null,
  ...over
})

describe('claudeBanner', () => {
  it('says nothing for no status: no answer yet, or the host could not be read', () => {
    expect(claudeBanner(null)).toBeNull()
  })

  it('says missing only when the host answered with no executable', () => {
    expect(claudeBanner(status({ path: null, version: null, source: null, error: 'not on PATH' }))).toBe('missing')
  })

  it('says unanswered when claude was found but gave no version', () => {
    expect(claudeBanner(status({ version: null, semver: null, tested: false }))).toBe('unanswered')
  })

  it('says outside-range when the version is not one this build was tested against', () => {
    expect(claudeBanner(status({ tested: false }))).toBe('outside-range')
  })

  it('says nothing for a found, tested claude', () => {
    expect(claudeBanner(status({}))).toBeNull()
  })
})
