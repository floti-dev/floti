import { AGENT_FIXES, type AppSettings, type Walkthrough, type WalkthroughStep } from '@floti/core/types'
import type { WalkthroughAgent } from '@floti/ui'
import type { ClaudeStatus } from '../../../../shared/ipc'

/**
 * What the walkthrough's two windows share: how an agent's own check reads as
 * a row, and the step each move goes to.
 */

const CLAUDE_CODE = 'claude-code'

/**
 * Claude Code's row, from what the host's check answered: installed when its
 * version command ran, signed in as its sign-in artefacts say. The check never
 * opens one, and a sign-in it cannot tell stays Unknown. `windows` picks the
 * install line for this PC.
 */
export function claudeAgent(status: ClaudeStatus, windows: boolean): WalkthroughAgent {
  const fix = AGENT_FIXES[CLAUDE_CODE]
  const installed = status.path !== null && status.version !== null
  return {
    id: CLAUDE_CODE,
    name: fix?.name ?? 'Claude Code',
    version: installed ? versionLine(status.semver ?? status.version ?? '', fix?.command ?? 'claude') : null,
    installed,
    signedIn: !installed ? 'unknown' : status.auth === 'authenticated' ? true : status.auth === 'unauthenticated' ? false : 'unknown',
    install: windows ? (fix?.install.windows ?? '') : (fix?.install.posix ?? ''),
    login: fix?.login ?? 'claude'
  }
}

/** `claude 2.1.319`: the program and the version, however the version command worded it. */
function versionLine(version: string, command: string): string {
  const bare = version.trim().split(/\s+/)[0] ?? ''
  return bare === '' ? command : `${command} ${bare}`
}

/** The step after `step`, or the step before it. */
export function stepAfter(step: WalkthroughStep): WalkthroughStep {
  return step >= 6 ? 6 : ((step + 1) as WalkthroughStep)
}
export function stepBefore(step: WalkthroughStep): WalkthroughStep {
  return step <= 1 ? 1 : ((step - 1) as WalkthroughStep)
}

/** The walkthrough at `step`, keeping the workspace it chose. */
export function atStep(current: Walkthrough | null, step: WalkthroughStep): Walkthrough {
  return { step, workspace: current?.workspace ?? null }
}

/** The folder's own name, for "Start an agent in my-work." */
export function folderName(path: string): string {
  const parts = path.split(/[\\/]+/).filter((part) => part !== '')
  return parts.at(-1) ?? path
}

/** The patch that leaves the walkthrough, done: it shows again only from Settings. */
export const WALKTHROUGH_DONE: Pick<AppSettings, 'walkthrough'> = { walkthrough: null }
