import { act, renderHook } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { TemplateChoice } from '@helm/core'
import { DEFAULT_SETTINGS } from '@helm/core/types'
import { bridge, remoteBridge } from './bridge.testkit'
import { useSetup } from './useSetup'
import { PATIENCE } from '../../../../../../test/patience'

vi.mock('./bridge', () => import('./bridge.testkit'))

const STARTER: TemplateChoice = {
  id: 'starter',
  label: 'Starter',
  description: 'repos/, AGENTS.md',
  order: null,
  builtIn: true
}
const CLIENT_WORK: TemplateChoice = {
  id: 'client-work',
  label: 'client-work',
  description: null,
  order: null,
  builtIn: false
}

/** A host from before Starter, whose built-in row is still `minimal`. */
const OLDER = 'h1'

beforeEach(() => {
  bridge.reset()
  bridge.whenUnanswered('pending')
  bridge.answer('template:list', () => ({
    templates: [STARTER, CLIENT_WORK],
    dir: '/home/you/.config/floti/templates',
    problems: []
  }))
  bridge.answer('template:preview', ({ template }) => ({
    template,
    entries: template === 'starter' ? ['workspace.yaml', 'AGENTS.md', 'repos/'] : ['workspace.yaml'],
    note: '',
    problems: []
  }))
  const older = remoteBridge(OLDER)
  older.reset()
  older.whenUnanswered('pending')
  older.answer('template:list', () => ({
    templates: [
      { id: 'minimal', label: 'Minimal', description: null, order: null, builtIn: true },
      { id: 'infra', label: 'Infra', description: null, order: 1, builtIn: false }
    ],
    dir: '/home/you/.config/helm/templates',
    problems: []
  }))
  older.answer('template:preview', ({ template }) => ({ template, entries: ['workspace.yaml'], note: '', problems: [] }))
})

describe("useSetup's template list", () => {
  it('opens with Starter first and chosen, then the user’s own templates, and previews Starter', async () => {
    const { result } = renderHook(() => useSetup(DEFAULT_SETTINGS, vi.fn()))

    act(() => result.current.openDialog('new'))
    expect(result.current.template).toBe('starter')
    await vi.waitFor(
      () => expect(result.current.templates.map((each) => each.id)).toEqual(['starter', 'client-work']),
      { timeout: PATIENCE }
    )
    expect(result.current.template).toBe('starter')
    await vi.waitFor(
      () => expect(result.current.templatePreview?.entries).toEqual(['workspace.yaml', 'AGENTS.md', 'repos/']),
      { timeout: PATIENCE }
    )
    expect(bridge.invoked('template:preview')).toContainEqual({ template: 'starter', mode: 'new' })
  })

  it('goes back to Starter each time the dialog opens, whatever was chosen last', async () => {
    const { result } = renderHook(() => useSetup(DEFAULT_SETTINGS, vi.fn()))
    act(() => result.current.openDialog('new'))
    await vi.waitFor(() => expect(result.current.templates).toHaveLength(2), { timeout: PATIENCE })

    act(() => result.current.chooseTemplate('client-work'))
    expect(result.current.template).toBe('client-work')
    act(() => result.current.closeDialog())

    act(() => result.current.openDialog('new'))
    expect(result.current.template).toBe('starter')
  })

  it('keeps a choice the listing still has when it is read again, and falls back to the built-in when it is gone', async () => {
    const { result } = renderHook(() => useSetup(DEFAULT_SETTINGS, vi.fn()))
    act(() => result.current.openDialog('new'))
    await vi.waitFor(() => expect(result.current.templates).toHaveLength(2), { timeout: PATIENCE })
    act(() => result.current.chooseTemplate('client-work'))

    act(() => result.current.refreshTemplates())
    await vi.waitFor(() => expect(bridge.invoked('template:list')).toHaveLength(2), { timeout: PATIENCE })
    expect(result.current.template).toBe('client-work')

    // Deleted in the template manager over the dialog.
    bridge.answer('template:list', () => ({ templates: [STARTER], dir: '', problems: [] }))
    act(() => result.current.refreshTemplates())
    await vi.waitFor(() => expect(result.current.template).toBe('starter'), { timeout: PATIENCE })
  })

  it('chooses an older host’s own built-in row, so its picker is never left with nothing chosen', async () => {
    const { result } = renderHook(() => useSetup(DEFAULT_SETTINGS, vi.fn()))

    act(() => result.current.openDialog('new', OLDER))
    await vi.waitFor(() => expect(result.current.template).toBe('minimal'), { timeout: PATIENCE })
    expect(result.current.templates.map((each) => each.id)).toEqual(['minimal', 'infra'])
  })
})
