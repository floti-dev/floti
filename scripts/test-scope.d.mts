// Types for test-scope.mjs, so what it refuses can be tested from TypeScript.

/** In a command, says the owner asked for this suite run. */
export const OWNER_ASKED: string

/** Why an agent may not run `command`, or null when it may. */
export function judge(command: unknown): string | null

export interface Decision {
  hookSpecificOutput: {
    hookEventName: 'PreToolUse'
    permissionDecision: 'deny'
    permissionDecisionReason: string
  }
}

/** What the hook prints for its input: a deny decision, or null for nothing. */
export function decide(input: unknown): Decision | null
