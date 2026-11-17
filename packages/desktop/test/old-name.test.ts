import { spawnSync } from 'node:child_process'
import { describe, expect, it } from 'vitest'
import { REPO_ROOT } from '../scripts/test-posix.mjs'

/**
 * The name Floti had before is not used anywhere in this repository: not in a
 * tracked file's text, binary files included, and not in a tracked file's
 * path. Nothing is allowed. The pattern is written with a bracket so this file
 * does not match it.
 */

const OLD_NAME = 'h[e]lm'

const REFUSAL = 'This repository does not use the name Floti had before; say Floti, the desktop or the host instead.'

/** Runs git in the repository root with no colour and paths as they are. */
function git(args: readonly string[]): { status: number | null; stdout: string; stderr: string } {
  const result = spawnSync('git', ['-c', 'core.quotePath=false', '-c', 'color.ui=never', ...args], {
    cwd: REPO_ROOT,
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
    windowsHide: true
  })
  if (result.error) throw result.error
  return { status: result.status, stdout: result.stdout, stderr: result.stderr }
}

describe('the old name', () => {
  it('is in no tracked file', () => {
    // Exit 1 is "nothing found"; 0 is a match, anything else a git failure.
    const { status, stdout, stderr } = git(['grep', '-i', '-n', '--full-name', '-e', OLD_NAME])
    if (status !== 0 && status !== 1) throw new Error(`git grep failed (${status}): ${stderr.trim()}`)
    expect(stdout.trim(), `${REFUSAL}\n${stdout}`).toBe('')
  })

  it('is in no tracked path', () => {
    const { status, stdout, stderr } = git(['ls-files', '-z'])
    if (status !== 0) throw new Error(`git ls-files failed (${status}): ${stderr.trim()}`)
    const pattern = new RegExp(OLD_NAME, 'i')
    const named = stdout.split('\0').filter((path) => pattern.test(path))
    expect(named, `${REFUSAL}\n${named.join('\n')}`).toEqual([])
  })
})
