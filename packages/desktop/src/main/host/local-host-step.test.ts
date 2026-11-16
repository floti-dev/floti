import { describe, expect, it } from 'vitest'
import type { LocalHostStep } from '../../shared/ipc'
import { createLocalHostSteps } from './local-host-step'

const VERSIONS = { host: { major: 3, minor: 1 }, client: { major: 4, minor: 1 }, update: 'host' as const }

function world() {
  let clock = 1000
  const steps = createLocalHostSteps(() => clock)
  const told: LocalHostStep[] = []
  steps.onStep((step) => told.push(step))
  return {
    steps,
    told,
    tick: (ms: number) => {
      clock += ms
    }
  }
}

describe('the local host’s steps, as main publishes them', () => {
  it('has none before the first connect, then each as it begins, with when it began', () => {
    const { steps, told, tick } = world()
    expect(steps.current()).toBeNull()
    steps.begin({ step: 'preparing' })
    tick(5)
    steps.begin({ step: 'connecting' })
    tick(10)
    steps.begin({ step: 'ready' })
    expect(told).toEqual([
      { step: 'preparing', since: 1000 },
      { step: 'connecting', since: 1005 },
      { step: 'ready', since: 1015 }
    ])
    expect(steps.current()).toEqual({ step: 'ready', since: 1015 })
  })

  it('keeps saying the host is updating through the connect and start of its restart, until it ends', () => {
    const { steps, told, tick } = world()
    steps.begin({ step: 'connecting' })
    steps.begin({ step: 'updating', from: 'protocol 3.1', to: '3.0.0' })
    tick(100)
    steps.begin({ step: 'preparing' })
    steps.begin({ step: 'connecting' })
    steps.begin({ step: 'starting' })
    expect(steps.current()).toEqual({ step: 'updating', since: 1000, from: 'protocol 3.1', to: '3.0.0' })
    steps.begin({ step: 'ready' })
    expect(told.map((step) => step.step)).toEqual(['connecting', 'updating', 'ready'])
  })

  it('ends an update that failed with the refusal or the reason, and connects as usual after it', () => {
    const { steps, told } = world()
    steps.begin({ step: 'updating', from: null, to: '3.0.0' })
    steps.begin({ step: 'refused', sentence: 'Update the host.', versions: VERSIONS })
    steps.begin({ step: 'connecting' })
    steps.begin({ step: 'updating', from: null, to: '3.0.0' })
    steps.begin({ step: 'unreachable', reason: 'No host answered.' })
    steps.begin({ step: 'starting' })
    expect(told.map((step) => step.step)).toEqual(['updating', 'refused', 'connecting', 'updating', 'unreachable', 'starting'])
  })

  it('puts back the step from before a restart that changed nothing, keeping when it began', () => {
    const { steps, told, tick } = world()
    steps.begin({ step: 'ready' })
    const before = steps.current()
    tick(50)
    steps.begin({ step: 'updating', from: null, to: '3.0.0' })
    steps.restore(before)
    expect(steps.current()).toEqual({ step: 'ready', since: 1000 })
    expect(told.at(-1)).toEqual({ step: 'ready', since: 1000 })
    steps.restore(null)
    expect(steps.current()).toEqual({ step: 'ready', since: 1000 })
  })

  it('stops telling a listener that unsubscribed', () => {
    const steps = createLocalHostSteps(() => 0)
    const told: string[] = []
    const off = steps.onStep((step) => told.push(step.step))
    steps.begin({ step: 'connecting' })
    off()
    steps.begin({ step: 'ready' })
    expect(told).toEqual(['connecting'])
  })
})
