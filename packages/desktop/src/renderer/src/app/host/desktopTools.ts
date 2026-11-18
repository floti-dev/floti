import type { HostConnection, HostSubscription } from '../../../../shared/host/client'
import type * as P from '../../../../shared/host/protocol'
import type { PageLink } from './link'

/**
 * The floti-desktop tools, answered in this window.
 *
 * Each host serves `floti-desktop` to the sessions it runs, but Floti's views
 * are here, so a call a session's agent makes reaches the window as an event
 * on `agentDesktop:call`, on the window's own connection to that host, and
 * goes back as the tool's answer on `agentDesktop:answer`. The window
 * declares `desktop` on every one of its host connections (`link.ts`), so a
 * session on another computer's host reaches the window attached to it; a host
 * that does not know the capability never sends a call, and nothing listens
 * there.
 *
 * A call names its tool and carries its arguments, and the window answers by
 * the tool's name with what is registered for it (`answerWith`), so a tool
 * added to the family adds a handler and nothing here. Until the window has
 * drawn and registered its handlers, a call is answered that the window is
 * still starting; a tool the window has no handler for is answered saying so.
 */

/** One call, as a handler takes it: the host it came from and the session that made it. */
export interface DesktopCall {
  host: string
  session: number
  tool: string
  arguments: unknown
}

/** An MCP tool result, as the agent reads it. */
export interface DesktopToolResult {
  content: { type: 'text'; text: string }[]
  isError?: boolean
}

/** What the window does for each tool, by its name. */
export type DesktopToolHandlers = Readonly<Record<string, (call: DesktopCall) => Promise<DesktopToolResult>>>

export interface DesktopTools {
  /** Answers calls with `handlers` from now on, in place of any before. Returns the stop. */
  answerWith(handlers: DesktopToolHandlers): () => void
  /** Hears `host`'s calls on `link` on every connect that took `desktop`. */
  listen(host: string, link: Pick<PageLink, 'onConnected'>): void
}

/** The answer while the window has not registered what it opens yet. */
export const WINDOW_STARTING = "Floti's window is still starting. Try again in a moment."

const failed = (message: string): P.AgentDesktopOutcome => ({ status: 'failed', message })

export function createDesktopTools(log: (line: string) => void): DesktopTools {
  let handlers: DesktopToolHandlers | null = null

  const outcomeOf = async (call: DesktopCall): Promise<P.AgentDesktopOutcome> => {
    if (handlers === null) return failed(WINDOW_STARTING)
    const handler = handlers[call.tool]
    if (handler === undefined) return failed(`Floti's window has no tool named ${call.tool}.`)
    try {
      return { status: 'done', value: await handler(call) }
    } catch (error) {
      return failed(`${call.tool} failed inside Floti: ${(error as Error).message}`)
    }
  }

  const answer = async (connection: HostConnection, host: string, call: P.AgentDesktopToolCall): Promise<void> => {
    const outcome = await outcomeOf({ host, session: call.session, tool: call.tool, arguments: call.arguments })
    try {
      await connection.request('agentDesktop:answer', { callId: call.callId, outcome })
    } catch (error) {
      // The connection ended, and the host has failed the call itself.
      log(`host: a desktop tool answer was not taken (${(error as Error).message})`)
    }
  }

  return {
    answerWith(next) {
      handlers = next
      return () => {
        if (handlers === next) handlers = null
      }
    },
    listen(host, link) {
      let subscription: HostSubscription | null = null
      link.onConnected((connection) => {
        subscription?.unsubscribe()
        subscription = null
        if (!connection.welcome.capabilities.includes('desktop')) return
        subscription = connection.subscribe('agentDesktop:call', {
          event: (call) => void answer(connection, host, call),
          error: (error) => log(`host: agentDesktop:call ended (${error.message})`)
        })
      })
    }
  }
}
