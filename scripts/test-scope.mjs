#!/usr/bin/env node
// The PreToolUse hook `.claude/settings.json` runs before every shell command
// an agent sends: it refuses a command that runs a whole test suite, because
// docs/TESTING.md has an agent run only the tests for what it changed. CI
// runs the suites; this hook exists only in agent sessions and never there.
//
// It reads the hook's JSON on standard input and, for a refused command,
// prints a `deny` decision with the reason and the command to run instead.
// Anything it cannot read is let through: a broken hook must not stop work.

import { pathToFileURL } from 'node:url'

/**
 * In a command, says the owner asked for this suite run. An agent adds it
 * only when the owner asked for that run in so many words.
 */
export const OWNER_ASKED = 'OWNER_ASKED_FULL_SUITE'

/**
 * Integration test targets that hold a crate's whole integration tier, so
 * naming one with no filter runs the tier.
 */
const WHOLE_TIER_TARGETS = new Set(['it'])

/** Crates whose every test builds and runs in seconds. */
const SMALL_CRATES = new Set(['floti-protocol'])

const CARGO_VALUE_FLAGS = new Set([
  '-p', '--package', '--test', '--bin', '--example', '--bench', '--profile', '--cargo-profile',
  '-E', '--filterset', '--filter-expr', '-j', '--jobs', '--build-jobs', '--test-threads',
  '--features', '-F', '--target', '--target-dir', '--manifest-path', '--color', '--partition',
  '--retries', '--config', '--config-file', '--tool-config-file', '--message-format',
  '--exclude', '--failure-output', '--success-output', '--status-level', '--final-status-level',
  '--max-fail', '--run-ignored', '--skip'
])

const VITEST_VALUE_FLAGS = new Set([
  '--project', '-c', '--config', '-t', '--testNamePattern', '--reporter', '--outputFile',
  '--pool', '--maxWorkers', '--minWorkers', '--testTimeout', '--hookTimeout', '--dir', '-r',
  '--root', '--environment', '--mode', '--bail', '--retry', '--shard', '--sequence',
  '--changed', '--exclude'
])

const SEE = 'docs/TESTING.md ("Running them") has the commands.'
const UNLESS = `Only when the owner asked for this run in so many words, set ${OWNER_ASKED}=1 in the command.`

/**
 * The command with what is only data taken out: heredoc bodies, PowerShell
 * here-strings and quoted strings (each left as the word `Q`, so a quoted
 * filter is still an argument). A commit message that mentions a suite is not
 * a suite run.
 */
function withoutData(command) {
  return command
    .replace(/<<-?\s*(['"]?)(\w+)\1[\s\S]*?\n[ \t]*\2\b/g, ' ')
    .replace(/@(['"])\r?\n[\s\S]*?\n\1@/g, ' Q ')
    .replace(/'[^']*'/g, ' Q ')
    .replace(/"(?:[^"\\]|\\.)*"/g, ' Q ')
}

/** The simple commands of a line: split at `&&`, `||`, `;`, `|`, `&` and line ends. */
function segments(command) {
  return withoutData(command)
    .split(/&&|\|\||[;|&\n\r]/)
    .map((part) => part.trim().split(/\s+/).filter(Boolean))
    .filter((words) => words.length > 0)
}

/** Flags, each with the values given for it, and the arguments that belong to no flag. */
function parseArgs(args, valueFlags) {
  const flags = new Map()
  const positional = []
  for (let i = 0; i < args.length; i++) {
    const arg = args[i]
    if (arg === '--') continue
    if (!arg.startsWith('-')) {
      positional.push(arg)
      continue
    }
    const equals = arg.indexOf('=')
    const name = equals === -1 ? arg : arg.slice(0, equals)
    const values = flags.get(name) ?? []
    if (equals !== -1) values.push(arg.slice(equals + 1))
    else if (valueFlags.has(name) && i + 1 < args.length) values.push(args[++i])
    flags.set(name, values)
  }
  return { flags, positional }
}

function judgeCargo(words) {
  const cargo = words.findIndex((word) => word === 'cargo' || word === 'cargo.exe')
  if (cargo === -1) return null
  const rest = words.slice(cargo + 1).filter((word) => !word.startsWith('+'))
  let args
  if (rest[0] === 'nextest' && rest[1] === 'run') args = rest.slice(2)
  else if (rest[0] === 'test' || rest[0] === 't') args = rest.slice(1)
  else return null

  const { flags, positional } = parseArgs(args, CARGO_VALUE_FLAGS)
  if (flags.has('--help') || flags.has('-h') || flags.has('--doc') || flags.has('--no-run')) return null

  const packages = [...(flags.get('-p') ?? []), ...(flags.get('--package') ?? [])]
  if (packages.length > 0 && packages.every((name) => SMALL_CRATES.has(name))) return null

  const filtered =
    positional.length > 0 || flags.has('-E') || flags.has('--filterset') || flags.has('--filter-expr')
  const targets = flags.get('--test') ?? []
  if (targets.some((name) => WHOLE_TIER_TARGETS.has(name)) && !filtered) {
    return (
      'This runs the whole integration tier. Name the tests for what changed: ' +
      "`cargo nextest run -p floti-host --test it -E 'test(<module>::)'`."
    )
  }
  const namesBinary =
    targets.length > 0 || flags.has('--lib') || flags.has('--bin') || flags.has('--bins')
  if (namesBinary && !flags.has('--tests') && !flags.has('--all-targets')) return null
  return (
    'This builds and links every test binary of the crate before it runs anything. Name the ' +
    "binary: `--lib -E 'test(<name>)'` for a module's tests, `--test it -E 'test(<module>::)'` " +
    'for integration tests; `cargo check -p <crate> --all-targets` finds compile errors without linking.'
  )
}

function judgePnpm(words) {
  const pnpm = words.findIndex((word) => word === 'pnpm' || word === 'pnpm.cmd' || word === 'npm')
  if (pnpm === -1) return null
  const args = words.slice(pnpm + 1)
  const script = args.find((word, i) => {
    if (word.startsWith('-')) return false
    const before = args[i - 1]
    return before !== '--filter' && before !== '-F' && before !== '-C' && before !== '--dir'
  })
  const name = script === 'run' ? args[args.indexOf('run') + 1] : script
  if (name === 'check') {
    return '`pnpm check` is the whole gate, which CI runs on every push. Run `pnpm vitest run <file>` for what changed.'
  }
  if (name === 'test') {
    return '`pnpm test` runs every vitest project with coverage. Run `pnpm vitest run <file>` for what changed.'
  }
  if (name === 'test:e2e') {
    return (
      'End-to-end tests run only before a release is cut, never to build, check a fix or finish a ' +
      'task. Add the unit or integration test that shows it instead.'
    )
  }
  return null
}

function judgePlaywright(words) {
  const at = words.findIndex((word) => /(^|[\\/])playwright(\.mjs|\.cmd)?$/.test(word))
  if (at === -1 || words[at + 1] !== 'test') return null
  if (words.includes('--help') || words.includes('--list')) return null
  return (
    'End-to-end tests run only before a release is cut, never to build, check a fix or finish a ' +
    'task. Add the unit or integration test that shows it instead.'
  )
}

function judgeVitest(words) {
  const at = words.findIndex((word) => /(^|[\\/])vitest(\.mjs|\.cmd)?$/.test(word))
  if (at === -1) return null
  const { flags, positional } = parseArgs(words.slice(at + 1), VITEST_VALUE_FLAGS)
  if (flags.has('--help') || flags.has('-h') || flags.has('--version') || flags.has('-v')) return null
  const [first, ...files] = positional
  if (first === 'list' || first === 'init') return null
  const named = ['run', 'watch', 'dev', 'related', 'bench'].includes(first) ? files : positional
  if (named.length > 0) return null
  return 'This runs a whole vitest project or every one. Name the test files for what changed: `pnpm vitest run <file>`.'
}

/**
 * Why an agent may not run `command`, or null when it may. A command is
 * refused when any of its simple commands runs a whole suite.
 */
export function judge(command) {
  if (typeof command !== 'string' || command.includes(OWNER_ASKED)) return null
  for (const words of segments(command)) {
    const reason =
      judgeCargo(words) ?? judgePnpm(words) ?? judgePlaywright(words) ?? judgeVitest(words)
    if (reason) return `${reason} ${SEE} ${UNLESS}`
  }
  return null
}

/** What the hook prints for a command: a deny decision, or nothing. */
export function decide(input) {
  const reason = judge(input?.tool_input?.command)
  if (!reason) return null
  return {
    hookSpecificOutput: {
      hookEventName: 'PreToolUse',
      permissionDecision: 'deny',
      permissionDecisionReason: reason
    }
  }
}

async function main() {
  let text = ''
  for await (const chunk of process.stdin) text += chunk
  let decision = null
  try {
    decision = decide(JSON.parse(text))
  } catch {
    // Not the hook's JSON: let the command through.
  }
  if (decision) process.stdout.write(JSON.stringify(decision))
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) await main()
