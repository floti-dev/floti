import { describe, expect, it, vi } from 'vitest'
import { PATIENCE } from '../../../../../../../test/patience'
import { FakeConnection, reconnectingReach } from '../../../../../test/fake-connection'
import type * as P from '../../../../shared/host/protocol'
import { WINDOW_STARTING, createDesktopTools, type DesktopCall, type DesktopToolResult } from './desktopTools'

const opened: DesktopToolResult = { content: [{ type: 'text', text: 'Opened main.rs beside your tab.' }] }

const call = (callId: number, tool = 'desktop_open'): P.AgentDesktopToolCall => ({
  callId,
  session: 7,
  tool,
  arguments: { view: 'history', search: 'login' }
})

/** The answers the window sent, once each has been sent. */
async function answers(connection: FakeConnection, count: number): Promise<unknown[]> {
  await vi.waitFor(() => expect(connection.payloads('agentDesktop:answer')).toHaveLength(count), { timeout: PATIENCE })
  return connection.payloads('agentDesktop:answer')
}

describe('the desktop tools, answered on the window’s own connection to each host', () => {
  it('hands each call to the handler for its tool, naming its host and session, and sends back what it answered', async () => {
    const tools = createDesktopTools(() => undefined)
    const connection = new FakeConnection(['terminal', 'desktop'])
    const link = reconnectingReach(connection)
    tools.listen('build-box', link)
    const seen: DesktopCall[] = []
    tools.answerWith({
      desktop_open: (each) => {
        seen.push(each)
        return Promise.resolve(opened)
      }
    })
    await link.connection()

    connection.push('agentDesktop:call', call(3))
    expect(await answers(connection, 1)).toEqual([{ callId: 3, outcome: { status: 'done', value: opened } }])
    expect(seen).toEqual([{ host: 'build-box', session: 7, tool: 'desktop_open', arguments: { view: 'history', search: 'login' } }])
  })

  it('says the window is still starting before anything is registered, and names a tool it has no handler for', async () => {
    const tools = createDesktopTools(() => undefined)
    const connection = new FakeConnection(['terminal', 'desktop'])
    const link = reconnectingReach(connection)
    tools.listen('local', link)
    await link.connection()

    connection.push('agentDesktop:call', call(1))
    expect(await answers(connection, 1)).toEqual([{ callId: 1, outcome: { status: 'failed', message: WINDOW_STARTING } }])

    const stop = tools.answerWith({ desktop_open: () => Promise.resolve(opened) })
    connection.push('agentDesktop:call', call(2, 'desktop_status'))
    const [, unknown] = await answers(connection, 2)
    expect(unknown).toEqual({
      callId: 2,
      outcome: { status: 'failed', message: "Floti's window has no tool named desktop_status." }
    })

    // A handler that throws is a failed call the agent can read.
    tools.answerWith({ desktop_open: () => Promise.reject(new Error('the layout is gone')) })
    connection.push('agentDesktop:call', call(3))
    const [, , thrown] = await answers(connection, 3)
    expect(thrown).toEqual({
      callId: 3,
      outcome: { status: 'failed', message: 'desktop_open failed inside Floti: the layout is gone' }
    })

    // The old registration's stop does not take away the new one.
    stop()
    connection.push('agentDesktop:call', call(4))
    const [, , , after] = await answers(connection, 4)
    expect(after).toMatchObject({ callId: 4, outcome: { status: 'failed' } })
  })

  it('listens only on a host that took desktop, and again on each connect', async () => {
    const tools = createDesktopTools(() => undefined)
    const older = new FakeConnection(['terminal'])
    const link = reconnectingReach(older)
    tools.listen('old-box', link)
    await link.connection()
    expect(older.subscriptions.has('agentDesktop:call')).toBe(false)

    const newer = new FakeConnection(['terminal', 'desktop'])
    await link.reconnect(newer)
    expect(newer.subscriptions.has('agentDesktop:call')).toBe(true)
  })

  it('logs an answer the host no longer takes, and sends nothing more', async () => {
    const lines: string[] = []
    const tools = createDesktopTools((line) => lines.push(line))
    const connection = new FakeConnection(['terminal', 'desktop'])
    connection.answer('agentDesktop:answer', () => {
      throw new Error('the connection ended')
    })
    const link = reconnectingReach(connection)
    tools.listen('local', link)
    tools.answerWith({ desktop_open: () => Promise.resolve(opened) })
    await link.connection()
    connection.push('agentDesktop:call', call(5))
    await vi.waitFor(() => expect(lines).toEqual(['host: a desktop tool answer was not taken (the connection ended)']), {
      timeout: PATIENCE
    })
  })
})
