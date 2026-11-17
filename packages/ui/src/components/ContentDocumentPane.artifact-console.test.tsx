import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import type { ContentFile } from '@floti/core'
import { ContentDocumentPane, type ArtifactConsoleEntry } from './ContentDocumentPane'

const REPORT: ContentFile = {
  path: 'C:\\vault\\notes\\report.html',
  relPath: 'notes/report.html',
  root: 'notes',
  rootKind: 'notes',
  kind: 'html',
  slug: 'report',
  ext: 'html',
  title: 'Report',
  size: 2048,
  mtimeMs: 0,
  noteType: null,
  date: null,
  tags: []
}

const LOGGED: ArtifactConsoleEntry[] = [
  { level: 'info', message: 'chart drawn', source: 'floti-content://artifact/abc/report.html', line: 12 },
  { level: 'error', message: 'axis data missing', source: 'floti-content://artifact/abc/report.html', line: 30 }
]

/** The artifact viewer showing REPORT with what its frame logged. */
function renderArtifact(logged: ArtifactConsoleEntry[]): void {
  render(
    <ContentDocumentPane
      file={REPORT}
      document={null}
      preview={null}
      previewPending={false}
      mode="read"
      artifactUrl="floti-content://artifact/abc/report.html"
      artifactConsole={logged}
      snapshots={[]}
      saving={false}
      error={null}
      external={null}
      highlight={null}
      onSave={vi.fn()}
      onReload={vi.fn()}
      onRestore={vi.fn()}
      onDirtyChange={vi.fn()}
      onDraftChange={vi.fn()}
      onOpenPath={vi.fn()}
      onOpenWikilink={vi.fn()}
      onOpenExternal={vi.fn()}
    />
  )
}

/**
 * An HTML artifact's console: the count in the frame's header is the toggle,
 * and the panel it opens is the browser pane's, read-only.
 */
describe('ContentDocumentPane - the artifact console', () => {
  it('opens from the count in the header, shows what the frame logged, and offers no input line', async () => {
    const user = userEvent.setup()
    renderArtifact(LOGGED)
    const count = screen.getByRole('button', { name: '1 console error' })
    expect(count.getAttribute('aria-expanded')).toBe('false')
    expect(screen.queryByText('axis data missing')).toBeNull()

    await user.click(count)
    expect(count.getAttribute('aria-expanded')).toBe('true')
    expect(screen.getByText('chart drawn')).toBeTruthy()
    expect(screen.getByText('axis data missing')).toBeTruthy()
    expect(screen.getByText(/^Read-only\./)).toBeTruthy()
    expect(screen.queryByRole('textbox', { name: 'Evaluate JavaScript in the page' })).toBeNull()

    await user.click(count)
    expect(screen.queryByText('axis data missing')).toBeNull()
  })

  it('counts warnings as warnings, not errors', () => {
    const warning = { level: 'warning', message: 'deprecated api', source: 'floti-content://artifact/abc/report.html', line: 3 }
    renderArtifact([warning, warning])
    expect(screen.getByRole('button', { name: '2 console warnings' })).toBeTruthy()
  })

  it('names both when there are errors and warnings', () => {
    const warning = { level: 'warning', message: 'deprecated api', source: 'floti-content://artifact/abc/report.html', line: 3 }
    renderArtifact([...LOGGED, warning])
    expect(screen.getByRole('button', { name: '1 console error, 1 warning' })).toBeTruthy()
  })
})
