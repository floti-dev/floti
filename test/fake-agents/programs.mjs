// The fake agents as programs: each written into a folder in the form an
// install leaves it on this OS, so whatever starts it (a test's host, a
// `pnpm dev --drive` window) starts it exactly as it would start the real one.
//
// The tests' worlds (`packages/desktop/test/world.ts`) and `pnpm dev --drive`
// (`packages/desktop/scripts/drive-agents.ts`) both write theirs here.

import { writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

/**
 * Every backend with a fake in this folder that Helm can be pointed at: the
 * program name an install gives it and the script that stands in for it.
 */
export const FAKE_AGENTS = Object.freeze({
  'claude-code': Object.freeze({ program: 'claude', script: 'claude-code.mjs' })
})

const HERE = dirname(fileURLToPath(import.meta.url))

/**
 * The fake for `backend` in `bin`: a `<program>.cmd` batch shim on Windows,
 * the way npm installs one, and an executable `<program>` shell script on
 * macOS and Linux. Either runs the fake's script with `node` and hands it
 * every argument unchanged. Returns the program's path.
 */
export function writeFakeAgent(bin, backend, { node = process.execPath, platform = process.platform } = {}) {
  const agent = FAKE_AGENTS[backend]
  if (agent === undefined) throw new Error(`no fake agent for the ${backend} backend`)
  const script = join(HERE, agent.script)
  if (platform === 'win32') {
    const program = join(bin, `${agent.program}.cmd`)
    writeFileSync(program, ['@echo off', `"${node}" "${script}" %*`, 'exit /b %ERRORLEVEL%', ''].join('\r\n'))
    return program
  }
  const program = join(bin, agent.program)
  writeFileSync(program, ['#!/bin/sh', `exec ${shellQuote(node)} ${shellQuote(script)} "$@"`, ''].join('\n'), {
    mode: 0o755
  })
  return program
}

/** One word for `sh`, whatever it holds: single quotes, with any inside closed and reopened. */
function shellQuote(text) {
  return `'${text.replace(/'/g, `'\\''`)}'`
}
