import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useState, type JSX } from 'react'
import { beforeAll, describe, expect, it, vi } from 'vitest'
import type { LaunchAgents } from '@helm/core/types'
import { AgentList } from './AgentList'
import { AgentPage, type AgentPageProps } from './AgentPage'
import { UndoToast } from './UndoToast'

const TEXT = [
  '---',
  'description: Researches a new project with you.',
  'backend: claude-code',
  'effort:',
  '  claude-code: high',
  'permission: plan',
  'class: light',
  '---',
  'You research.',
  ''
].join('\n')

const CLAUDE = { id: 'claude-code', name: 'Claude Code' }

beforeAll(() => {
  // The editor measures itself; the test DOM has nothing to measure.
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe(): void {}
      disconnect(): void {}
    }
  )
})

function props(overrides: Partial<AgentPageProps> = {}): AgentPageProps {
  return {
    savedName: 'research-lead',
    copyOf: null,
    name: 'research-lead',
    onNameChange: vi.fn(),
    takenNames: new Set(['research-lead', 'worker']),
    hostName: 'This PC',
    remote: false,
    folder: '~/.config/floti/agents',
    draft: TEXT,
    onDraftChange: vi.fn(),
    dirty: false,
    saving: false,
    savedAt: null,
    problems: [],
    refusal: null,
    bad: false,
    readError: null,
    external: null,
    readOnly: null,
    backends: [CLAUDE],
    profiles: ['studio-docs'],
    onHighlight: null,
    onSave: vi.fn(),
    onRevert: vi.fn(),
    onDiscard: vi.fn(),
    onCopy: vi.fn(),
    onDelete: vi.fn(),
    onStartSession: vi.fn(),
    onReload: vi.fn(),
    onSaveMine: vi.fn(),
    ...overrides
  }
}

/** The page with its draft and name held, as the window holds them. */
function Held({ start }: { start: AgentPageProps }): JSX.Element {
  const [draft, setDraft] = useState(start.draft)
  const [name, setName] = useState(start.name)
  return (
    <AgentPage
      {...start}
      draft={draft}
      name={name}
      onDraftChange={(text) => {
        setDraft(text)
        start.onDraftChange(text)
      }}
      onNameChange={setName}
      dirty={draft !== start.draft || name !== start.name}
    />
  )
}

describe('AgentPage', () => {
  it('shows the file’s fields in the form, says the retired line is dropped, and writes a form edit into the draft', async () => {
    const start = props()
    render(<Held start={start} />)
    expect((screen.getByRole('textbox', { name: 'Description' }) as HTMLInputElement).value).toBe(
      'Researches a new project with you.'
    )
    expect((screen.getByRole('combobox', { name: 'Permissions' }) as HTMLSelectElement).value).toBe('plan')
    expect(screen.getByText(/is a line Floti no longer uses/).textContent).toContain('class: light')

    await userEvent.selectOptions(screen.getByRole('combobox', { name: 'Effort for Claude Code' }), 'xhigh')
    expect(start.onDraftChange).toHaveBeenLastCalledWith(TEXT.replace('claude-code: high', 'claude-code: xhigh'))
    expect(screen.getByText('Unsaved changes')).toBeTruthy()

    // Source is the same draft: the edit made in the form is there.
    await userEvent.click(screen.getByRole('radio', { name: 'Source' }))
    expect((screen.getByRole('textbox', { name: 'Source' }) as HTMLTextAreaElement).value).toContain('claude-code: xhigh')
  })

  it('says the name rule as it is typed, and Save waits for a name the folder accepts', async () => {
    render(<Held start={props()} />)
    const name = screen.getByRole('textbox', { name: 'Name' })
    await userEvent.clear(name)
    await userEvent.type(name, 'Android Worker')
    expect(screen.getByText(/lowercase letters, digits and dashes/).textContent).toContain('android-worker would do')
    expect((screen.getByRole('button', { name: /^Save/ }) as HTMLButtonElement).disabled).toBe(true)

    await userEvent.clear(name)
    await userEvent.type(name, 'worker')
    expect(screen.getByText(/There is already an agent called/)).toBeTruthy()

    await userEvent.clear(name)
    await userEvent.type(name, 'issue-lead')
    expect(screen.getByText(/Saving renames/).textContent).toBe('Saving renames research-lead.md to issue-lead.md.')
    expect((screen.getByRole('button', { name: /^Save/ }) as HTMLButtonElement).disabled).toBe(false)
  })

  it('marks each problem on its field and puts one with no field in the strip with its line', () => {
    render(
      <AgentPage
        {...props({
          bad: true,
          problems: [
            { field: 'permission', line: 6, message: 'permission must be plan, dont-ask, manual, accept-edits, auto or bypass-permissions, not yolo' },
            { field: 'disalowed_tools', line: 9, message: 'disalowed_tools is not a field an agent file has' }
          ]
        })}
      />
    )
    const strip = screen.getByRole('alert')
    expect(within(strip).getByText('Floti cannot load research-lead.md')).toBeTruthy()
    expect(strip.textContent).toContain('line 9 · disalowed_tools is not a field an agent file has. Fix it in Source.')
    const permission = screen.getByRole('combobox', { name: 'Permissions' }).closest('[data-agent-field-box]')
    expect(permission?.getAttribute('data-problem')).toBe('true')
    expect(permission?.textContent).toContain('not yolo')
  })

  it('shows a refused save of a file that loads as Not saved, with each problem, in Form and in Source', async () => {
    const problems = [
      { field: 'permission', line: 6, message: 'permission must be plan, dont-ask, manual, accept-edits, auto or bypass-permissions, not yolo' },
      { field: 'command', line: 3, message: 'command is not a field an agent file has' },
      { field: null, line: null, message: 'description is given twice' },
      { field: null, line: null, message: 'description is given twice' }
    ]
    render(<AgentPage {...props({ dirty: true, problems })} />)
    const strip = screen.getByRole('alert')
    expect(within(strip).getByText('Not saved')).toBeTruthy()
    expect(strip.textContent).toContain('The problems are marked on their fields.')
    expect(strip.textContent).toContain('line 3 · command is not a field an agent file has. Fix it in Source.')
    expect(strip.textContent).not.toContain('not yolo')
    expect(within(strip).getAllByText(/description is given twice/)).toHaveLength(2)

    await userEvent.click(screen.getByRole('radio', { name: 'Source' }))
    const inSource = screen.getByRole('alert')
    expect(within(inSource).getByText('Not saved')).toBeTruthy()
    expect([...inSource.querySelectorAll('[data-agent-problem]')].map((p) => p.textContent)).toEqual([
      'line 6 · permission must be plan, dont-ask, manual, accept-edits, auto or bypass-permissions, not yolo',
      'line 3 · command is not a field an agent file has',
      'description is given twice',
      'description is given twice'
    ])
    expect(document.querySelector('[data-editor-invalid]')?.getAttribute('data-editor-invalid')).toBe('true')
  })

  it('says a refused save of a new agent wrote nothing, as it has no file on disk', () => {
    const problems = [{ field: 'effort', line: 5, message: 'effort for claude-code must be low, medium, high, xhigh or max' }]
    const { unmount } = render(<AgentPage {...props({ savedName: null, name: 'triage', dirty: true, problems })} />)
    const strip = screen.getByRole('alert')
    expect(strip.textContent).toContain('Nothing was written, and your edit is kept.')
    expect(strip.textContent).not.toContain('The file on disk')
    unmount()

    render(<AgentPage {...props({ dirty: true, problems })} />)
    expect(screen.getByRole('alert').textContent).toContain('The file on disk is as it was, and your edit is kept.')
  })

  it('saves on Ctrl+S only while Save is on', async () => {
    const onSave = vi.fn()
    render(<Held start={props({ onSave })} />)
    const name = screen.getByRole('textbox', { name: 'Name' })
    await userEvent.clear(name)
    await userEvent.type(name, 'Worker{Control>}s{/Control}')
    expect(onSave).not.toHaveBeenCalled()

    await userEvent.clear(name)
    await userEvent.type(name, 'issue-lead{Control>}s{/Control}')
    expect(onSave).toHaveBeenCalledTimes(1)
  })

  it('says when the file could not be read again, and keeps the page as last read', () => {
    render(<AgentPage {...props({ readError: 'The connection to build-box ended.' })} />)
    expect(document.querySelector('[data-agent-read-error]')?.textContent).toBe(
      'Floti could not read research-lead.md again, so this is the file as last read: The connection to build-box ended.'
    )
    expect((screen.getByRole('textbox', { name: 'Description' }) as HTMLInputElement).value).toBe(
      'Researches a new project with you.'
    )
  })

  it('marks only the model or effort cell of the backend a problem names, and every cell for the whole field', () => {
    const two = [CLAUDE, { id: 'codex', name: 'Codex' }]
    const draft = TEXT.replace('backend: claude-code', 'backend: claude-code, codex')
    const { rerender } = render(
      <AgentPage
        {...props({
          draft,
          backends: two,
          bad: true,
          problems: [{ field: 'effort', line: 4, message: 'effort for codex must be low, medium, high, xhigh or max', backend: 'codex' }]
        })}
      />
    )
    const marked = (cell: string, id: string): boolean =>
      document.querySelector(`[data-agent-${cell}="${id}"]`)?.className.includes('border-danger') === true
    expect([marked('effort', 'codex'), marked('effort', 'claude-code'), marked('model', 'codex')]).toEqual([true, false, false])
    expect(document.querySelector('[data-agent-backends]')?.className).not.toContain('border-danger')

    rerender(
      <AgentPage
        {...props({ draft, backends: two, bad: true, problems: [{ field: 'model', line: 4, message: 'model is a map from a backend id to a value' }] })}
      />
    )
    expect([marked('model', 'codex'), marked('model', 'claude-code'), marked('effort', 'codex')]).toEqual([true, true, false])
  })

  it('draws the instructions as their markdown source alone, with no rendered preview beside them', () => {
    render(<AgentPage {...props({ draft: TEXT.replace('You research.', 'You *research*.') })} />)
    const box = document.querySelector('[data-agent-instructions]')
    expect(box?.children).toHaveLength(1)
    expect(within(box as HTMLElement).getByRole('textbox', { name: 'Instructions' })).toHaveProperty('value', 'You *research*.\n')
    expect(document.querySelector('[data-agent-preview]')).toBeNull()
    expect(document.querySelector('.markdown')).toBeNull()
    expect(box?.querySelector('em')).toBeNull()
  })

  it('greys the retired line in Source with the words that say it changes nothing', async () => {
    render(<AgentPage {...props()} />)
    await userEvent.click(screen.getByRole('radio', { name: 'Source' }))
    const quiet = [...document.querySelectorAll('[data-editor-highlight] .line[data-editor-quiet]')]
    expect(quiet.map((line) => line.textContent)).toEqual(['class: light'])
    expect(quiet[0]?.getAttribute('data-editor-note')).toBe('no longer used · saving removes it')
    expect(document.querySelectorAll('[data-editor-highlight] .line')[6]).toBe(quiet[0])
  })

  it('deletes at once from the bar, asking nothing first', async () => {
    const onDelete = vi.fn()
    render(<AgentPage {...props({ onDelete })} />)
    await userEvent.click(screen.getByRole('button', { name: 'Delete' }))
    expect(onDelete).toHaveBeenCalledOnce()
    expect(screen.queryByRole('dialog')).toBeNull()
    expect(screen.queryByRole('alertdialog')).toBeNull()
  })

  it('offers Compare, Reload from disk and Save mine for an outside change, and Save waits until one is picked', async () => {
    const start = props({ dirty: true, draft: `${TEXT}mine\n`, external: { text: `${TEXT}theirs\n`, at: '14:31' } })
    render(<AgentPage {...start} />)
    expect(screen.getByText('research-lead.md changed on disk after you opened it')).toBeTruthy()
    expect((document.querySelector('[data-agent-save]') as HTMLButtonElement).disabled).toBe(true)

    await userEvent.click(screen.getByRole('button', { name: 'Compare' }))
    const disk = document.querySelector('[data-compare-side="disk"]')!
    expect(disk.textContent).toContain('On disk now · changed 14:31')
    expect([...disk.querySelectorAll('[data-compare-line="changed"]')].map((l) => l.textContent)).toEqual(['10theirs'])

    await userEvent.click(screen.getByRole('button', { name: 'Save mine' }))
    expect(start.onSaveMine).toHaveBeenCalledOnce()
    await userEvent.click(screen.getByRole('button', { name: 'Reload from disk' }))
    expect(start.onReload).toHaveBeenCalledOnce()
  })

  it('draws a new page with Discard and Save agent, and names a remote host before the path', () => {
    const { unmount } = render(<AgentPage {...props({ savedName: null, copyOf: 'worker', name: 'worker-2' })} />)
    expect(screen.getByText('Copy of worker · not saved')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Discard' })).toBeTruthy()
    expect(screen.getByRole('button', { name: /Save agent/ })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Delete' })).toBeNull()
    unmount()
    render(<AgentPage {...props({ hostName: 'buildbox', remote: true })} />)
    expect(screen.getByText('buildbox · ~/.config/floti/agents/research-lead.md')).toBeTruthy()
  })
})

const LISTING: LaunchAgents = {
  agents: [
    {
      name: 'lead',
      description: 'Delegates work.',
      file: 'lead.md',
      hash: 'h',
      backends: ['claude-code'],
      listedBackends: ['claude-code'],
      models: {},
      efforts: {},
      permission: null,
      profile: null,
      allowedTools: [],
      disallowedTools: [],
      worktree: false,
      canStart: true,
      cannotRun: null
    }
  ],
  bad: [
    {
      name: 'deploy-check',
      file: 'deploy-check.md',
      problem: 'permission must be plan, not yolo',
      problems: [{ field: 'permission', line: 4, message: 'permission must be plan, not yolo' }],
      hash: 'b'
    }
  ],
  backends: [CLAUDE],
  folder: '~/.config/floti/agents',
  readOnly: null,
  shipped: [{ name: 'reviewer', description: 'Read-only review.' }]
}

describe('AgentList', () => {
  it('lists the agents, the files that are not agents with their problem, and the shipped agents with Add', async () => {
    const onOpen = vi.fn()
    const onAdd = vi.fn()
    render(
      <AgentList listing={LISTING} error={null} hostName="This PC" remote={false} selected="lead" onOpen={onOpen} onAdd={onAdd} />
    )
    const lead = screen.getByRole('button', { name: /^lead/ })
    expect(lead.getAttribute('aria-current')).toBe('page')
    expect(screen.getByRole('heading', { name: /Not loaded/ }).textContent).toBe('Not loaded1')
    expect(screen.getByText('deploy-check.md')).toBeTruthy()
    expect(screen.getByText('permission must be plan, not yolo')).toBeTruthy()
    expect(screen.getByText(/Files in/).textContent).toBe(
      'Files in ~/.config/floti/agents on This PC, read afresh on every start.'
    )
    await userEvent.click(screen.getByRole('button', { name: 'Add' }))
    expect(onAdd).toHaveBeenCalledWith('reviewer')
    await userEvent.click(screen.getByRole('button', { name: /deploy-check\.md/ }))
    expect(onOpen).toHaveBeenCalledWith('deploy-check')
  })
})

describe('UndoToast', () => {
  it('stays until closed, with Undo', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'setInterval'] })
    const onUndo = vi.fn()
    const onClose = vi.fn()
    render(<UndoToast title="Deleted triage" detail="Undo puts it back." undoing={false} error={null} onUndo={onUndo} onClose={onClose} />)
    vi.advanceTimersByTime(60 * 60 * 1000)
    vi.useRealTimers()
    expect(screen.getByText('Deleted triage')).toBeTruthy()
    await userEvent.click(screen.getByRole('button', { name: 'Undo' }))
    expect(onUndo).toHaveBeenCalledOnce()
    await userEvent.click(screen.getByRole('button', { name: 'Close notification' }))
    expect(onClose).toHaveBeenCalledOnce()
  })
})
