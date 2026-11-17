import { render, screen, within } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { EMPTY_INVENTORY, type ClaudeInventory, type Project } from '@helm/core/types'
import { ProjectPane } from './ProjectPane'

const project = (hasClaudeDir: boolean, inventory: Partial<ClaudeInventory>): Project => ({
  path: 'C:\\work\\hub',
  name: 'hub',
  kind: 'workspace',
  workspacePath: null,
  hasClaudeDir,
  inventory: { ...EMPTY_INVENTORY, ...inventory },
  git: null
})

function renderPane(shown: Project) {
  render(<ProjectPane project={shown} onReveal={vi.fn()} onLaunch={vi.fn()} />)
}

/** Each line of the Configuration panel, with whether it reads present. */
function configuration(): string[] {
  const panel = screen.getByText('Configuration').closest('section') ?? document.body
  return within(panel)
    .getAllByRole('listitem')
    .map((item) => item.textContent ?? '')
}

/** The count beside a stat's label in the contribution panel. */
function stat(label: string): string | null {
  return screen.getByText(label).parentElement?.textContent?.replace(label, '') ?? null
}

describe('ProjectPane - a workspace in either shape', () => {
  it('names AGENTS.md and .agents/ and counts their skills, as it does CLAUDE.md and .claude/', () => {
    renderPane(project(false, { agentsMd: true, agentsDir: true, skills: 2 }))
    expect(configuration()).toEqual([
      '.claude/absent',
      'CLAUDE.mdabsent',
      '.agents/present',
      'AGENTS.mdpresent',
      'settings.jsonabsent',
      'hooks/absent',
      '.mcp.jsonabsent'
    ])
    expect(stat('skills')).toBe('2')
    expect(screen.queryByText(/Nothing to compose/)).toBeNull()
  })

  it('shows a folder in Claude’s shape as it always has, with no line for the shared shape', () => {
    renderPane(project(true, { claudeMd: true, skills: 2 }))
    expect(configuration()).toEqual([
      '.claude/present',
      'CLAUDE.mdpresent',
      'settings.jsonabsent',
      'hooks/absent',
      '.mcp.jsonabsent'
    ])
    expect(stat('skills')).toBe('2')
  })
})
