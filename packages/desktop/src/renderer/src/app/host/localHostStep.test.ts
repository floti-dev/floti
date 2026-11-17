import { describe, expect, it } from 'vitest'
import type { DesktopBridge, LocalHostStep } from '../../../../shared/ipc'
import { createLocalHostStep } from './localHostStep'

/**
 * Main played by the test: the step it answers when asked, settled when the
 * test says, and each step it sends on `hostLink:step`.
 */
function world() {
  let send: (step: LocalHostStep) => void = () => undefined
  let answer: (step: LocalHostStep | null) => void = () => undefined
  let fail: (error: Error) => void = () => undefined
  const asked: string[] = []
  const bridge = {
    on: (channel: string, listener: (step: LocalHostStep) => void) => {
      if (channel === 'hostLink:step') send = listener
      return () => undefined
    },
    invoke: (channel: string) => {
      asked.push(channel)
      return new Promise<LocalHostStep | null>((resolve, reject) => {
        answer = resolve
        fail = reject
      })
    }
  } as unknown as Pick<DesktopBridge, 'invoke' | 'on'>
  const logged: string[] = []
  const store = createLocalHostStep(bridge, (line) => logged.push(line))
  let changes = 0
  store.subscribe(() => changes++)
  return {
    store,
    asked,
    logged,
    changes: () => changes,
    send: (step: LocalHostStep) => send(step),
    answer: async (step: LocalHostStep | null) => {
      answer(step)
      await Promise.resolve()
    },
    fail: async (error: Error) => {
      fail(error)
      await Promise.resolve()
    }
  }
}

describe('the local host’s step, as the window keeps it', () => {
  it('starts from the step under way when the window loads after the first ones were sent', async () => {
    const w = world()
    expect(w.asked).toEqual(['hostLink:currentStep'])
    expect(w.store.get()).toBeNull()
    await w.answer({ step: 'updating', since: 10, from: 'protocol 3.1', to: '3.0.0' })
    expect(w.store.get()).toEqual({ step: 'updating', since: 10, from: 'protocol 3.1', to: '3.0.0' })
    expect(w.changes()).toBe(1)
  })

  it('follows each step main sends, and drops an answer that came after one', async () => {
    const w = world()
    w.send({ step: 'starting', since: 20 })
    w.send({ step: 'ready', since: 30 })
    await w.answer({ step: 'connecting', since: 5 })
    expect(w.store.get()).toEqual({ step: 'ready', since: 30 })
    expect(w.changes()).toBe(2)
  })

  it('stays null when main has not begun a connect, and says so in the log when it could not ask', async () => {
    const quiet = world()
    await quiet.answer(null)
    expect(quiet.store.get()).toBeNull()
    expect(quiet.changes()).toBe(0)
    const failing = world()
    await failing.fail(new Error('no handler'))
    await Promise.resolve()
    expect(failing.store.get()).toBeNull()
    expect(failing.logged).toEqual(["host: the local host's step was not read (no handler)"])
  })
})
