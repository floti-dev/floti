import { act, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  LONG_STEP_MS,
  StartupSteps,
  StartupWaiting,
  startupTreeStep,
  startupView,
  type StartupStep
} from './StartupSteps'

const T0 = Date.UTC(2026, 10, 15, 12, 0, 0)

const at = (step: Exclude<StartupStep['step'], 'updating'>, since = T0): StartupStep => ({ step, since })
const updating = (to: string, since = T0): StartupStep => ({ step: 'updating', since, to })

/** Each listed step as `<key>:<state>`, top to bottom. */
function rows(container: HTMLElement): string[] {
  return [...container.querySelectorAll<HTMLElement>('[data-startup-step]')].map(
    (row) => `${row.dataset.startupStep ?? ''}:${row.dataset.state ?? ''}`
  )
}

const current = (container: HTMLElement): HTMLElement | null => container.querySelector('[aria-current="step"]')
const waiting = (container: HTMLElement): string | null =>
  container.querySelector('[data-startup-waiting]')?.textContent ?? null

describe('startupView', () => {
  const before = { layoutArrived: false, layoutEmpty: true, hostUnusable: false }
  const laidOut = { layoutArrived: true, layoutEmpty: false, hostUnusable: false }
  const nothingOpen = { layoutArrived: true, layoutEmpty: true, hostUnusable: false }

  it('shows the steps from the first frame, before main has said one, until the layout arrives', () => {
    expect(startupView(null, before)).toBe('steps')
    expect(startupView(null, laidOut)).toBeNull()
  })

  it('shows every start-up step, and ready, while the layout has not arrived, so no empty pane flashes before it', () => {
    for (const step of [at('preparing'), at('connecting'), at('starting'), updating('3.0.0'), at('ready')]) {
      expect(startupView(step, before)).toBe('steps')
      expect(startupView(step, laidOut)).toBeNull()
      // Nothing open once laid out: the welcome page, not the steps.
      expect(startupView(step, nothingOpen)).toBeNull()
    }
  })

  it('steps aside for a host that cannot be used, through main’s retries, and waits where there are no panes', () => {
    const unusable = { ...before, hostUnusable: true }
    // A refusal, then main trying again: its steps never bring the start-up view back over the notice.
    for (const step of [at('refused'), at('unreachable'), at('preparing'), at('connecting'), at('starting')]) {
      expect(startupView(step, unusable)).toBe('waiting')
      expect(startupView(step, { ...nothingOpen, hostUnusable: true })).toBe('waiting')
      // Saved tabs to draw: they say they wait, each in its own way.
      expect(startupView(step, { ...laidOut, hostUnusable: true })).toBeNull()
    }
  })

  it('takes a refused or unreachable step for a host that cannot be used before the usable store has caught up', () => {
    expect(startupView(at('refused'), before)).toBe('waiting')
    expect(startupView(at('unreachable'), laidOut)).toBeNull()
  })

  it('goes back to the steps once main updates the host it could not use', () => {
    expect(startupView(updating('3.0.0'), before)).toBe('steps')
  })
})

describe('StartupWaiting', () => {
  it('says it waits for the host, and repeats no reason and offers no fix', () => {
    const { container } = render(<StartupWaiting hostName="this PC’s host" />)
    expect(screen.getByRole('status').textContent).toBe(
      'Waiting for this PC’s host…Your projects and sessions show here once it answers.'
    )
    expect(container.querySelector('button')).toBeNull()
  })
})

describe('startupTreeStep', () => {
  it('names the step the Sessions tree says while this PC’s host has not answered, and preparing before any', () => {
    expect(startupTreeStep(null)).toBe('preparing')
    expect(startupTreeStep(at('preparing'))).toBe('preparing')
    expect(startupTreeStep(at('connecting'))).toBe('connecting')
    expect(startupTreeStep(at('starting'))).toBe('starting')
    expect(startupTreeStep(updating('3.0.0'))).toBeNull()
    expect(startupTreeStep(at('ready'))).toBeNull()
    expect(startupTreeStep(at('refused'))).toBeNull()
    expect(startupTreeStep(at('unreachable'))).toBeNull()
  })
})

describe('StartupSteps', () => {
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'Date'] })
    vi.setSystemTime(T0)
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it('marks preparing as under way before main has said a step', () => {
    const { container } = render(<StartupSteps step={null} />)
    expect(screen.getByRole('status', { name: 'Starting Floti' })).toBeTruthy()
    expect(rows(container)).toEqual(['preparing:current', 'connecting:next'])
    expect(current(container)?.textContent).toBe('Preparing floti')
  })

  it('lists each step with the one under way marked and the ones before it checked', () => {
    const { container, rerender } = render(<StartupSteps step={at('preparing')} />)
    expect(rows(container)).toEqual(['preparing:current', 'connecting:next'])

    rerender(<StartupSteps step={at('connecting')} />)
    expect(rows(container)).toEqual(['preparing:done', 'connecting:current'])
    expect(current(container)?.textContent).toBe('Connecting to this PC’s host')

    rerender(<StartupSteps step={at('starting')} />)
    expect(rows(container)).toEqual(['preparing:done', 'connecting:done', 'starting:current'])
    expect(current(container)?.textContent).toBe('Starting floti host')
  })

  it('says which floti an update restarts the host onto', () => {
    const { container } = render(<StartupSteps step={updating('3.0.0-preview.2')} />)
    expect(rows(container)).toEqual(['preparing:done', 'connecting:done', 'updating:current'])
    expect(current(container)?.textContent).toBe('Updating this PC’s host to floti 3.0.0-preview.2')
  })

  it('checks every step on ready, keeping the host it started listed, while the layout is on its way', () => {
    const { container, rerender } = render(<StartupSteps step={at('starting')} />)
    rerender(<StartupSteps step={at('ready')} />)
    expect(rows(container)).toEqual(['preparing:done', 'connecting:done', 'starting:done'])
    expect(current(container)).toBeNull()
    expect(waiting(container)).toBeNull()
  })

  it('checks preparing and connecting on ready when the window opened after the connect', () => {
    const { container } = render(<StartupSteps step={at('ready')} />)
    expect(rows(container)).toEqual(['preparing:done', 'connecting:done'])
  })

  it.each([
    ['preparing', at('preparing'), 'Copying floti into place'],
    ['connecting', at('connecting'), 'Waiting for this PC’s host to answer'],
    ['starting', at('starting'), 'Waiting for floti host to start'],
    ['updating', updating('3.0.0'), 'Waiting for the old host to hand over its sessions']
  ])('says what %s waits on, and for how long, once it has run a few seconds', (_name, step, words) => {
    const { container } = render(<StartupSteps step={step} />)
    expect(waiting(container)).toBeNull()

    act(() => vi.advanceTimersByTime(LONG_STEP_MS - 1000))
    expect(waiting(container)).toBeNull()

    act(() => vi.advanceTimersByTime(1000))
    expect(waiting(container)).toBe(`${words} · 4 s`)

    act(() => vi.advanceTimersByTime(3000))
    expect(waiting(container)).toBe(`${words} · 7 s`)
  })

  it('starts counting again from each new step', () => {
    const { container, rerender } = render(<StartupSteps step={at('connecting')} />)
    act(() => vi.advanceTimersByTime(5000))
    expect(waiting(container)).toBe('Waiting for this PC’s host to answer · 5 s')

    rerender(<StartupSteps step={at('starting', T0 + 5000)} />)
    expect(waiting(container)).toBeNull()
    act(() => vi.advanceTimersByTime(LONG_STEP_MS))
    expect(waiting(container)).toBe('Waiting for floti host to start · 4 s')
  })

  it('counts from when main began the step, for a window that opened partway through it', () => {
    vi.setSystemTime(T0 + 12_000)
    const { container } = render(<StartupSteps step={at('starting')} />)
    expect(waiting(container)).toBe('Waiting for floti host to start · 12 s')
  })
})
