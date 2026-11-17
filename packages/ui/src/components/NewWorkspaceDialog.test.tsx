import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import type { TemplateChoice, TemplatePreview } from '@helm/core'
import { NewWorkspaceDialog, type NewWorkspaceDialogProps } from './NewWorkspaceDialog'

const TEMPLATES: TemplateChoice[] = [
  {
    id: 'starter',
    label: 'Starter',
    description: 'repos/, AGENTS.md',
    order: null,
    builtIn: true
  },
  { id: 'client', label: 'Client work', description: 'Notes, tools and a review skill.', order: 1, builtIn: false },
  { id: 'bare', label: 'bare', description: null, order: null, builtIn: false }
]

/** What `template:preview` answers for Starter. */
const STARTER_PREVIEW: TemplatePreview = {
  template: 'starter',
  entries: ['workspace.yaml', 'AGENTS.md', 'repos/'],
  note: 'Nothing else.',
  problems: []
}

/** What `template:preview` answers for `client`: the manifest first. */
const PREVIEW: TemplatePreview = {
  template: 'client',
  entries: ['workspace.yaml', '.claude/skills/review/SKILL.md', 'CLAUDE.md', 'notes/'],
  note: 'From the Client work template.',
  problems: []
}

function renderDialog(overrides: Partial<NewWorkspaceDialogProps> = {}) {
  const props: NewWorkspaceDialogProps = {
    mode: 'new',
    dir: 'C:\\work',
    onChooseDir: vi.fn(),
    templates: TEMPLATES,
    template: 'starter',
    onTemplateChange: vi.fn(),
    templatesDir: 'C:\\Users\\me\\.config\\helm\\templates',
    onManageTemplates: vi.fn(),
    onCreate: vi.fn(),
    onCancel: vi.fn(),
    ...overrides
  }
  render(<NewWorkspaceDialog {...props} />)
  return props
}

describe('NewWorkspaceDialog', () => {
  it('lists Starter first and chosen, then the user’s own templates by their template.yaml, with no Minimal row', () => {
    renderDialog({ template: undefined })
    const picker = screen.getByRole('radiogroup', { name: 'Template' })
    const rows = within(picker).getAllByRole('radio')
    expect(rows.map((row) => row.textContent)).toEqual([
      'Starterrepos/, AGENTS.md',
      'Client workNotes, tools and a review skill.',
      'bare'
    ])
    expect(within(picker).getByRole('radio', { name: /^Starter/ }).getAttribute('aria-checked')).toBe('true')
    expect(within(picker).queryByRole('radio', { name: /^Minimal/ })).toBeNull()
  })

  it('says Starter writes workspace.yaml, AGENTS.md and an empty repos/, and creates from it', async () => {
    const props = renderDialog({ template: undefined, preview: STARTER_PREVIEW })
    await userEvent.type(screen.getByRole('textbox', { name: 'Workspace name' }), 'my-work')

    expect(screen.getAllByRole('listitem').map((item) => item.textContent)).toEqual([
      'workspace.yaml',
      'AGENTS.md',
      'repos/'
    ])
    await userEvent.click(screen.getByRole('button', { name: 'Create workspace' }))
    expect(props.onCreate).toHaveBeenCalledWith({ mode: 'new', dir: 'C:\\work', name: 'my-work', template: 'starter' })
  })

  it('shows no picker when using a folder I have', () => {
    renderDialog({ mode: 'convert', preview: { ...STARTER_PREVIEW, entries: ['workspace.yaml'] } })
    expect(screen.queryByRole('radiogroup', { name: 'Template' })).toBeNull()
    expect(screen.getAllByRole('listitem').map((item) => item.textContent)).toEqual(['workspace.yaml'])
  })

  it('picks a template and creates the workspace from it', async () => {
    const props = renderDialog({ template: 'client', preview: PREVIEW })
    await userEvent.click(screen.getByRole('radio', { name: /^bare/ }))
    expect(props.onTemplateChange).toHaveBeenCalledWith('bare')

    await userEvent.type(screen.getByRole('textbox', { name: 'Workspace name' }), 'acme')
    await userEvent.click(screen.getByRole('button', { name: 'Create workspace' }))
    expect(props.onCreate).toHaveBeenCalledWith({ mode: 'new', dir: 'C:\\work', name: 'acme', template: 'client' })
  })

  it('lists what gets written exactly as the preview gives it, manifest first', async () => {
    renderDialog({ template: 'client', preview: PREVIEW })
    await userEvent.type(screen.getByRole('textbox', { name: 'Workspace name' }), 'acme')

    expect(screen.getByText('C:\\work\\acme')).toBeTruthy()
    expect(screen.getAllByRole('listitem').map((item) => item.textContent)).toEqual(PREVIEW.entries)
    expect(screen.getByText(PREVIEW.note)).toBeTruthy()
  })

  it('lists nothing under what gets written until the preview answers', () => {
    renderDialog({ preview: null })
    expect(screen.queryAllByRole('listitem')).toEqual([])
  })

  it('opens the template manager from beside the picker', async () => {
    const props = renderDialog()
    await userEvent.click(screen.getByRole('button', { name: 'Manage templates…' }))
    expect(props.onManageTemplates).toHaveBeenCalledTimes(1)
  })
})
