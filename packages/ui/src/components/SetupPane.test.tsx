import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { SetupPane, type SetupClaudeStatus, type SetupPaneProps } from './SetupPane'

const FOUND: SetupClaudeStatus = {
  path: 'C:/bin/claude.exe',
  source: 'discovered',
  version: '2.1.999 (Claude Code)',
  semver: '2.1.999',
  tested: true,
  testedRange: { min: '2.1.0', max: '2.2.0' },
  configDir: 'C:/home/.claude',
  configDirExists: true,
  auth: 'authenticated',
  authSignal: 'credentials present',
  error: null
}

function renderPane(over: Partial<SetupPaneProps>): void {
  render(
    <SetupPane
      status={null}
      roots={[]}
      suggestions={[]}
      projectCount={0}
      scanning={false}
      checking={false}
      onRecheck={vi.fn()}
      onLocateClaude={vi.fn()}
      onAddFolder={vi.fn()}
      onAcceptSuggestion={vi.fn()}
      onCreateWorkspace={vi.fn()}
      onConvertFolder={vi.fn()}
      onFinish={vi.fn()}
      {...over}
    />
  )
}

const INSTALL_HINT = /Install it from/

describe('SetupPane Claude Code step', () => {
  it('says Unknown with the reason, with no install hint, when the read failed and nothing has answered', () => {
    renderPane({ status: null, statusError: 'host unreachable' })
    expect(screen.getByText('Unknown: host unreachable')).toBeTruthy()
    expect(screen.queryByText('Not found.')).toBeNull()
    expect(screen.queryByText(INSTALL_HINT)).toBeNull()
  })

  it('keeps the last answer and adds the failure of a later check beside it', () => {
    renderPane({ status: FOUND, statusError: 'host unreachable' })
    expect(screen.getByText('2.1.999 (Claude Code) · C:/bin/claude.exe')).toBeTruthy()
    expect(screen.getByText('Could not check again: host unreachable')).toBeTruthy()
  })

  it('says a claude found without a version was found, with no install hint', () => {
    renderPane({ status: { ...FOUND, version: null, semver: null, tested: false } })
    expect(screen.getByText('Found, but it did not report its version.')).toBeTruthy()
    expect(screen.queryByText(INSTALL_HINT)).toBeNull()
  })

  it('still says not found, with the install hint, when the host answered no claude', () => {
    renderPane({ status: { ...FOUND, path: null, version: null, source: null } })
    expect(screen.getByText('Not found.')).toBeTruthy()
    expect(screen.getByText(INSTALL_HINT)).toBeTruthy()
  })
})
