import type { SetupClaudeStatus } from '../components/SetupPane'

/**
 * Why the Claude Code banner is up. `missing` is the only kind that offers
 * "Locate claude…": the others are not cured by picking a different file.
 *
 * - `missing`: the host answered and found no claude.
 * - `unanswered`: claude was found but did not report a version.
 * - `outside-range`: it answered with a version this build was not tested against.
 */
export type ClaudeBannerKind = 'missing' | 'unanswered' | 'outside-range'

/**
 * What the banner says for a status, or null for nothing.
 *
 * `null` is "no answer yet" or "could not look", and a banner is never shown
 * for it: a host that cannot be reached is the host notice's to say, and
 * "nothing there" is a claim only an answer can make (CLAUDE.md).
 */
export function claudeBanner(status: SetupClaudeStatus | null): ClaudeBannerKind | null {
  if (status === null) return null
  if (status.path === null) return 'missing'
  if (status.version === null) return 'unanswered'
  if (!status.tested) return 'outside-range'
  return null
}
