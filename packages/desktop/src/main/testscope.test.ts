import { spawnSync } from 'node:child_process'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { decide, judge, OWNER_ASKED } from '../../../../scripts/test-scope.mjs'
import { REPO_ROOT } from '../../scripts/test-posix.mjs'

/**
 * `scripts/test-scope.mjs`, the hook that refuses an agent's command when it
 * runs a whole test suite (docs/TESTING.md).
 */

const SCRIPT = join(REPO_ROOT, 'scripts', 'test-scope.mjs')

describe('a command that runs a whole suite is refused', () => {
  it.each([
    'cargo nextest run --workspace',
    'cargo nextest run -p floti-host',
    "cargo nextest run -p floti-host -E 'test(activity)'",
    'cargo nextest run -p floti-host activity',
    'cargo test',
    'cargo test -p floti-host sessions',
    'cargo test --workspace --locked',
    'cargo nextest run -p floti-host --test it',
    'cargo nextest run -p floti-host --lib --tests',
    'cd /work/helm && CARGO_BUILD_JOBS=4 cargo nextest run -p floti-host 2>&1 | tail -5',
    'pnpm check',
    'pnpm run check',
    'pnpm test',
    'pnpm test packages/core/src/layout/panes.test.ts',
    'node scripts/test-leftovers.mjs pnpm check',
    'pnpm test:e2e',
    'pnpm --filter @helm/desktop test:e2e',
    'pnpm --filter @helm/desktop exec node scripts/playwright.mjs test -c e2e panes',
    'npx playwright test',
    'pnpm vitest run',
    'pnpm vitest run --project ui',
    'pnpm exec vitest --run --coverage',
    'npx vitest'
  ])('%s', (command) => {
    const reason = judge(command)
    expect(reason).toEqual(expect.any(String))
    expect(reason).toContain('docs/TESTING.md')
    expect(reason).toContain(OWNER_ASKED)
  })
})

describe('a command that runs the tests for what changed is let through', () => {
  it.each([
    "cargo nextest run -p floti-host --lib -E 'test(agent_tools::)'",
    'cargo nextest run -p floti-host --lib',
    "cargo nextest run -p floti-host --test it -E 'test(agents::)'",
    'cargo nextest run -p floti-host --test it agents::remote_control',
    'cargo test -p floti-host --test it -- agents::',
    'cargo nextest run -p floti-protocol',
    'cargo test --doc --workspace',
    'cargo nextest run --help',
    'cargo check -p floti-host --all-targets',
    'cargo build -p floti-host --bin floti --features testkit',
    'cargo run -p floti-protocol --bin generate',
    'pnpm vitest run packages/core/src/layout/panes.test.ts',
    'pnpm vitest run --project ui packages/ui/src/Button.test.tsx',
    'pnpm exec vitest related packages/core/src/layout/panes.ts',
    'pnpm vitest --version',
    'pnpm test:posix',
    'pnpm test:posix packages/core/src/links.test.ts',
    'pnpm typecheck',
    'pnpm lint',
    'pnpm dev --drive',
    'pnpm install',
    'git status'
  ])('%s', (command) => {
    expect(judge(command)).toBeNull()
  })
})

describe('what is only data in a command is not a suite run', () => {
  it.each([
    'git commit -m "pnpm check passes and cargo test is green"',
    "grep -rn 'cargo nextest run --workspace' docs",
    "git commit -q -F - <<'EOF'\nThe hook refuses pnpm test\nand cargo nextest run --workspace\nEOF",
    "git commit -m @'\nRefuse pnpm check\n'@"
  ])('%s', (command) => {
    expect(judge(command)).toBeNull()
  })

  it('still refuses a suite run beside data that names another', () => {
    expect(judge('echo "cargo test" && pnpm check')).toEqual(expect.any(String))
  })
})

describe('the owner asking for a suite', () => {
  it.each([
    `${OWNER_ASKED}=1 pnpm check`,
    `$env:${OWNER_ASKED} = '1'; cargo nextest run --workspace`
  ])('lets %s through', (command) => {
    expect(judge(command)).toBeNull()
  })
})

describe('the decision the hook prints', () => {
  it('denies a refused command with the reason', () => {
    const reason = judge('pnpm check')
    expect(reason).not.toBeNull()
    expect(decide({ tool_name: 'Bash', tool_input: { command: 'pnpm check' } })).toEqual({
      hookSpecificOutput: {
        hookEventName: 'PreToolUse',
        permissionDecision: 'deny',
        permissionDecisionReason: reason
      }
    })
  })

  it.each([
    [{ tool_name: 'Bash', tool_input: { command: 'git status' } }],
    [{ tool_name: 'Read', tool_input: { file_path: 'pnpm check' } }],
    [{}],
    [null]
  ])('is nothing for %j', (input) => {
    expect(decide(input)).toBeNull()
  })
})

describe('run as the hook', () => {
  function run(stdin: string): { status: number | null; stdout: string } {
    const result = spawnSync(process.execPath, [SCRIPT], { input: stdin, encoding: 'utf8' })
    return { status: result.status, stdout: result.stdout }
  }

  it('prints a deny decision for a suite run and exits 0', () => {
    const { status, stdout } = run(JSON.stringify({ tool_name: 'Bash', tool_input: { command: 'pnpm check' } }))
    expect(status).toBe(0)
    expect(JSON.parse(stdout)).toMatchObject({
      hookSpecificOutput: { hookEventName: 'PreToolUse', permissionDecision: 'deny' }
    })
  })

  it('prints nothing for a targeted run', () => {
    expect(
      run(JSON.stringify({ tool_name: 'Bash', tool_input: { command: 'pnpm vitest run a.test.ts' } }))
    ).toEqual({ status: 0, stdout: '' })
  })

  it('lets through input it cannot read', () => {
    expect(run('not json')).toEqual({ status: 0, stdout: '' })
  })
})
