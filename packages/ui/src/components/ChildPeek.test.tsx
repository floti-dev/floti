import { fireEvent, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { overlayOpen } from '../lib/overlay'
import { ChildPeek, peekScreenCap, type ChildPeekProps, type PeekScreenRoom } from './ChildPeek'

function peek(overrides: Partial<ChildPeekProps> = {}): ChildPeekProps {
  const props: ChildPeekProps = {
    name: 'reviewer',
    mark: { glyph: 'dot', state: 'busy' },
    state: { label: 'working', tone: 'working' },
    role: 'reviewer',
    folder: 'helm',
    folderPath: 'C:\\work\\helm',
    branch: 'main',
    mode: 'plan',
    report: null,
    screen: () => <div data-testid="screen">the child’s screen</div>,
    at: { x: 300, y: 80 },
    onClose: vi.fn(),
    onStop: vi.fn(),
    onOpenAsTab: vi.fn(),
    ...overrides
  }
  return props
}

describe('the peek', () => {
  it('S1: names the child with its state, role, folder, branch and mode, and shows its screen read-only with Stop and Open as tab', () => {
    render(<ChildPeek {...peek({ report: { state: 'done', text: 'Reviewed the diff; two nits.' } })} />)
    const dialog = screen.getByRole('dialog', { name: 'reviewer' })
    expect(within(dialog).getByText('working').getAttribute('data-peek-state')).toBe('working')
    const facts = dialog.querySelector('dl')
    expect(facts?.textContent).toBe('RolereviewerFolderhelmBranchmainModeplan')
    expect(within(dialog).getByText('helm').getAttribute('title')).toBe('C:\\work\\helm')
    expect(within(dialog).getByTestId('screen')).toBeTruthy()
    const report = dialog.querySelector('[data-peek-report]')
    expect(report?.textContent).toBe('Last report · doneReviewed the diff; two nits.')
    expect(within(dialog).getByText('Watching only. Open it as a tab to type.')).toBeTruthy()
    expect(within(dialog).getByRole('button', { name: 'Stop' })).toBeTruthy()
    expect(within(dialog).getByRole('button', { name: 'Open as tab' })).toBeTruthy()
  })

  it('S1: says none for a child started with no role, and has no report block when it told nothing', () => {
    render(<ChildPeek {...peek({ role: null, branch: null, mode: null })} />)
    const dialog = screen.getByRole('dialog')
    expect(dialog.querySelector('dl')?.textContent).toBe('RolenoneFolderhelmBranchnoneModedefault')
    expect(dialog.querySelector('[data-peek-report]')).toBeNull()
  })

  it('S1: a command child shows its command, folder and branch, and no role or mode', () => {
    render(
      <ChildPeek
        {...peek({
          name: 'tests',
          mark: { glyph: 'command', state: 'running' },
          command: 'pnpm test --watch',
          runsIn: 'your shell, pwsh'
        })}
      />
    )
    const dialog = screen.getByRole('dialog', { name: 'tests' })
    expect(dialog.querySelector('dl')?.textContent).toBe(
      'Commandpnpm test --watchFolderhelmBranchmainRuns inyour shell, pwsh'
    )
    expect(dialog.querySelector('[data-child-command="running"]')).not.toBeNull()
    expect(dialog.querySelector('[data-child-dot]')).toBeNull()
    expect(within(dialog).queryByText('Role')).toBeNull()
    expect(within(dialog).queryByText('Mode')).toBeNull()
  })

  it('S24: a command child shows its command in fg, its facts in mono, no mode and no report, with Stop and Open as tab', () => {
    render(
      <ChildPeek
        {...peek({
          name: 'tests',
          mark: { glyph: 'command', state: 'running' },
          state: { label: 'running', tone: 'idle' },
          command: 'pnpm test',
          runsIn: 'your shell, pwsh',
          report: { state: 'done', text: 'never shown' }
        })}
      />
    )
    const dialog = screen.getByRole('dialog', { name: 'tests' })
    expect(within(dialog).getByText('pnpm test').className).toContain('text-fg')
    expect(within(dialog).getByText('pnpm test').className).toContain('font-mono')
    expect(within(dialog).getByText('your shell, pwsh').className).toContain('font-mono')
    expect(within(dialog).getByText('running').getAttribute('data-peek-state')).toBe('idle')
    expect(dialog.querySelector('[data-peek-report]')).toBeNull()
    expect(within(dialog).getByRole('button', { name: 'Stop' })).toBeTruthy()
    expect(within(dialog).getByRole('button', { name: 'Open as tab' })).toBeTruthy()
  })

  it('S24: a command whose shell the window cannot name has no Runs in row', () => {
    render(<ChildPeek {...peek({ command: 'pnpm test', mark: { glyph: 'command', state: 'running' } })} />)
    expect(screen.queryByText('Runs in')).toBeNull()
  })

  it('S25: an ended command says how it ended in its glyph tone and chip, keeps Open as tab and has no Stop', () => {
    for (const [state, tone, label, text] of [
      ['ended', 'text-fg-subtle', 'exited 0', 'neutral'],
      ['failed', 'text-danger', 'exited 2', 'danger']
    ] as const) {
      const { unmount } = render(
        <ChildPeek
          {...peek({
            name: 'tests',
            mark: { glyph: 'command', state },
            state: { label, tone: text },
            command: 'pnpm test',
            runsIn: 'your shell, pwsh',
            ended: true
          })}
        />
      )
      const dialog = screen.getByRole('dialog', { name: 'tests' })
      expect(dialog.querySelector(`[data-child-command="${state}"]`)?.className).toContain(tone)
      expect(within(dialog).getByText(label).getAttribute('data-peek-state')).toBe(text)
      expect(within(dialog).getByText('Ended. Open as tab shows its last screen.')).toBeTruthy()
      expect(within(dialog).queryByRole('button', { name: 'Stop' })).toBeNull()
      expect(within(dialog).getByRole('button', { name: 'Open as tab' })).toBeTruthy()
      unmount()
    }
  })

  it('S1: is 470px wide, its screen box spans it with no height of its own, and its last report label is semibold', () => {
    render(<ChildPeek {...peek({ report: { state: 'done', text: 'Reviewed.' } })} />)
    const dialog = screen.getByRole('dialog')
    expect(dialog.className).toContain('w-[470px]')
    const box = dialog.querySelector<HTMLElement>('[data-peek-screen]')
    // Its height is the screen's: no fixed height and no fixed width.
    const classes = box?.className.split(' ') ?? []
    expect(classes.filter((name) => name.startsWith('h-') || name.startsWith('w-'))).toEqual([])
    expect(classes).toContain('mx-3')
    expect(dialog.querySelector('[data-peek-report] span')?.className).toContain('font-semibold')
  })

  it('keeps the screen box 150px high while there is no screen to draw yet', () => {
    render(<ChildPeek {...peek({ screen: null })} />)
    expect(screen.getByRole('dialog').querySelector('[data-peek-screen]')?.className).toContain('h-[150px]')
  })

  describe('the room it gives its screen', () => {
    afterEach(() => {
      vi.restoreAllMocks()
    })

    /** Lays the peek out as 400px high with a 150px screen box whose border is 1px and whose inner width is 444px. */
    function layOut(): void {
      const box = (element: HTMLElement): boolean => element.hasAttribute('data-peek-screen')
      vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockImplementation(function (this: HTMLElement) {
        return box(this) ? 444 : 0
      })
      vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockImplementation(function (this: HTMLElement) {
        return box(this) ? 148 : 0
      })
      vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockImplementation(function (this: HTMLElement) {
        return box(this) ? 150 : this.getAttribute('role') === 'dialog' ? 400 : 0
      })
    }

    it('is the box’s inner width, and the cap less its edges, the cap being what fits under the peek’s other parts', () => {
      layOut()
      vi.spyOn(window, 'innerHeight', 'get').mockReturnValue(520)
      const rooms: PeekScreenRoom[] = []
      render(
        <ChildPeek
          {...peek({
            screen: (room) => {
              rooms.push(room)
              return <div data-testid="screen" />
            }
          })}
        />
      )
      // 520 less 8px above and below and the 250px the other parts take, less the 2px border.
      expect(rooms.at(-1)).toEqual({ width: 444, maxHeight: 520 - 16 - 250 - 2 })
      expect(screen.getByTestId('screen')).toBeTruthy()
    })

    it('is capped at 360px in a tall window', () => {
      layOut()
      vi.spyOn(window, 'innerHeight', 'get').mockReturnValue(1200)
      const rooms: PeekScreenRoom[] = []
      render(<ChildPeek {...peek({ screen: (room) => (rooms.push(room), null) })} />)
      expect(rooms.at(-1)?.maxHeight).toBe(360 - 2)
    })
  })

  it('S1: an ended child says so, keeps Open as tab and has no Stop', () => {
    render(<ChildPeek {...peek({ ended: true, state: { label: 'ended', tone: 'neutral' } })} />)
    const dialog = screen.getByRole('dialog')
    const chip = within(dialog).getByText('ended')
    expect(chip.getAttribute('data-peek-state')).toBe('neutral')
    expect(chip.className).toContain('text-fg-subtle')
    expect(chip.className).not.toContain('text-fg-muted')
    expect(within(dialog).getByText('Ended. Open as tab shows its last screen.')).toBeTruthy()
    expect(within(dialog).queryByRole('button', { name: 'Stop' })).toBeNull()
    expect(within(dialog).getByRole('button', { name: 'Open as tab' })).toBeTruthy()
  })

  it('an ended child that sent no screen to this window says so', () => {
    render(<ChildPeek {...peek({ ended: true, screenSeen: false })} />)
    expect(screen.getByText('Ended. This window saw none of its screen.')).toBeTruthy()
    expect(screen.queryByText('Ended. Open as tab shows its last screen.')).toBeNull()
  })

  it('S2: is an overlay while it is mounted, so a browser view stands down for it', () => {
    expect(overlayOpen()).toBe(false)
    const view = render(<ChildPeek {...peek()} />)
    expect(overlayOpen()).toBe(true)
    view.unmount()
    expect(overlayOpen()).toBe(false)
  })

  it('S2: Escape and the close button close it', async () => {
    const props = peek()
    render(<ChildPeek {...props} />)
    await userEvent.keyboard('{Escape}')
    expect(props.onClose).toHaveBeenCalledTimes(1)
    await userEvent.click(screen.getByRole('button', { name: 'Close peek' }))
    expect(props.onClose).toHaveBeenCalledTimes(2)
  })

  it('takes the keyboard on the close button when it opens, and gives it back to what pressed it', () => {
    const opener = document.createElement('button')
    document.body.append(opener)
    opener.focus()
    const view = render(<ChildPeek {...peek()} />)
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Close peek' }))
    view.unmount()
    expect(document.activeElement).toBe(opener)
    opener.remove()
  })

  it('a press outside closes it, and a press on the row or chip it opened from does not', () => {
    const anchor = document.createElement('button')
    document.body.append(anchor)
    const props = peek({ anchorRef: { current: anchor } })
    render(<ChildPeek {...props} />)
    fireEvent.pointerDown(anchor)
    expect(props.onClose).not.toHaveBeenCalled()
    fireEvent.pointerDown(screen.getByRole('dialog'))
    expect(props.onClose).not.toHaveBeenCalled()
    fireEvent.pointerDown(document.body)
    expect(props.onClose).toHaveBeenCalledTimes(1)
    anchor.remove()
  })

  it('S5: Stop is the window to answer: it closes the peek as the question opens, so the peek closes nothing itself', async () => {
    const props = peek()
    render(<ChildPeek {...props} />)
    await userEvent.click(screen.getByRole('button', { name: 'Stop' }))
    expect(props.onStop).toHaveBeenCalledTimes(1)
    expect(props.onClose).not.toHaveBeenCalled()
  })

  it('S6: Open as tab is the window’s to answer', async () => {
    const props = peek()
    render(<ChildPeek {...props} />)
    await userEvent.click(screen.getByRole('button', { name: 'Open as tab' }))
    expect(props.onOpenAsTab).toHaveBeenCalledTimes(1)
    expect(props.onClose).not.toHaveBeenCalled()
  })

  it('has Open after Open as tab for a child closed out of its parent’s family view, in the accent outline, and it is the window’s to answer', async () => {
    const onOpen = vi.fn()
    const props = peek({ onOpen })
    render(<ChildPeek {...props} />)
    const asTab = screen.getByRole('button', { name: 'Open as tab' })
    const open = screen.getByRole('button', { name: 'Open' })
    expect(asTab.nextElementSibling).toBe(open)
    expect(open.className).toContain('border-accent')
    expect(asTab.className).toContain('border-border-strong')
    expect(screen.getByText('Watching only. Open it to type.')).toBeTruthy()
    await userEvent.click(open)
    expect(onOpen).toHaveBeenCalledTimes(1)
    expect(props.onOpenAsTab).not.toHaveBeenCalled()
    expect(props.onClose).not.toHaveBeenCalled()
  })

  it('has no Open for any other child, whose Open as tab takes the accent outline', () => {
    render(<ChildPeek {...peek()} />)
    expect(screen.queryByRole('button', { name: 'Open' })).toBeNull()
    expect(screen.getByRole('button', { name: 'Open as tab' }).className).toContain('border-accent')
  })

  it('is placed at the point it is given, hidden until measured', () => {
    render(<ChildPeek {...peek({ at: { x: 320, y: 90 } })} />)
    const dialog = screen.getByRole('dialog')
    expect(dialog.style.position).toBe('fixed')
    expect(dialog.style.left).toBe('320px')
    expect(dialog.style.top).toBe('90px')
  })
})

describe('the cap on the peek’s screen box', () => {
  it('is what fits in the window under the peek’s other parts, inside its margins', () => {
    expect(peekScreenCap(600, 250)).toBe(600 - 16 - 250)
  })

  it('is never less than 150px, however short the window', () => {
    expect(peekScreenCap(380, 250)).toBe(150)
  })

  it('is never more than 360px, however tall the window', () => {
    expect(peekScreenCap(1400, 250)).toBe(360)
  })
})
