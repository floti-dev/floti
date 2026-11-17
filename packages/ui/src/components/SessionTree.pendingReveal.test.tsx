import { render } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { EMPTY_INVENTORY, type DiscoveryResult, type Project } from '@floti/core/types'
import { SessionTree, type SessionTreeProps, type TreeReveal } from './SessionTree'

/**
 * A window just opened has not listed its projects when the request for one's
 * row arrives: the request waits for the row, and is carried out once.
 */

const ZETA = 'C:\\work\\north\\repos\\zeta'

const project = (path: string): Project => ({
  path,
  name: path.split('\\').pop() ?? path,
  kind: 'repo',
  workspacePath: 'C:\\work\\north',
  hasClaudeDir: false,
  inventory: EMPTY_INVENTORY,
  git: null
})

const DISCOVERY: DiscoveryResult = {
  roots: ['C:\\work'],
  workspaces: [{ path: 'C:\\work\\north', name: 'north', template: null, version: null, repoPaths: [ZETA] }],
  projects: [project(ZETA)],
  errors: [],
  scannedAt: '2026-10-03T00:00:00.000Z',
  durationMs: 1
}

let scrolled: string[] = []
const scrollIntoView = Element.prototype.scrollIntoView
beforeEach(() => {
  scrolled = []
  Element.prototype.scrollIntoView = function record(this: Element) {
    scrolled.push(this.getAttribute('title') ?? '')
  }
})
afterEach(() => {
  Element.prototype.scrollIntoView = scrollIntoView
})

function renderTree(reveal: TreeReveal | null) {
  const props: SessionTreeProps = {
    discovery: null,
    scanning: true,
    selectedPath: ZETA,
    pinnedPaths: [],
    onTogglePin: vi.fn(),
    sessionsByPath: new Map(),
    elsewhere: [],
    onSelect: vi.fn(),
    onLaunch: vi.fn(),
    launchingPath: null,
    onOpenSession: vi.fn(),
    onNewSession: vi.fn(),
    reveal
  }
  const view = render(<SessionTree {...props} />)
  return (next: Partial<SessionTreeProps>) => view.rerender(<SessionTree {...props} {...next} />)
}

describe('SessionTree: a reveal before its row is listed', () => {
  it('waits for the project to appear, then reveals it once', () => {
    const reveal: TreeReveal = { seq: 1, kind: 'project', path: ZETA }
    const rerender = renderTree(reveal)
    expect(scrolled).toEqual([])

    rerender({ reveal, discovery: DISCOVERY, scanning: false })
    expect(document.querySelectorAll('[aria-current="true"]')).toHaveLength(1)
    expect(scrolled).toEqual([ZETA])

    rerender({ reveal, discovery: { ...DISCOVERY }, scanning: false })
    expect(scrolled).toEqual([ZETA])
  })

  it('lets a newer request replace the one still waiting', () => {
    const rerender = renderTree({ seq: 1, kind: 'project', path: 'C:\\work\\gone' })
    const reveal: TreeReveal = { seq: 2, kind: 'project', path: ZETA }
    rerender({ reveal })
    rerender({ reveal, discovery: DISCOVERY, scanning: false })
    expect(scrolled).toEqual([ZETA])
  })
})
