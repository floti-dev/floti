import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMainLog, HIDDEN, hideSecrets, KEPT_MAIN_LOGS, MAIN_LOG_FILE, startLine, TAIL_MOST, teeConsole } from './mainLog'

let root: string
let logs: string

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), 'floti main log-'))
  logs = join(root, 'logs dir')
})

afterEach(() => {
  rmSync(root, { recursive: true, force: true })
})

const names = (): string[] => readdirSync(logs).sort()

describe('main.log', () => {
  it('is made in the logs folder, owner-only on macOS and Linux, with each line appended', () => {
    const log = createMainLog(logs)
    expect(log.path).toBe(join(logs, MAIN_LOG_FILE))
    log.write('one')
    log.write('two')
    expect(readFileSync(log.path, 'utf8')).toBe('one\ntwo\n')
    if (process.platform !== 'win32') expect(statSync(log.path).mode & 0o777).toBe(0o600)
  })

  it('rotates past its limit as host.log does, keeping three older files', () => {
    const log = createMainLog(logs, { limit: 10 })
    for (let n = 0; n < 6; n += 1) log.write(`line ${String(n)} past the limit`)
    expect(names()).toEqual(['main.log', 'main.log.1', 'main.log.2', 'main.log.3'])
    expect(KEPT_MAIN_LOGS).toBe(3)
    expect(readFileSync(join(logs, 'main.log'), 'utf8')).toBe('line 5 past the limit\n')
    expect(readFileSync(join(logs, 'main.log.1'), 'utf8')).toBe('line 4 past the limit\n')
    expect(readFileSync(join(logs, 'main.log.3'), 'utf8')).toBe('line 2 past the limit\n')
  })

  it('rotates a log a run before left past the limit before its first line', () => {
    mkdirSync(logs, { recursive: true })
    writeFileSync(join(logs, MAIN_LOG_FILE), 'x'.repeat(50))
    createMainLog(logs, { limit: 10 }).write('fresh')
    expect(readFileSync(join(logs, MAIN_LOG_FILE), 'utf8')).toBe('fresh\n')
    expect(readFileSync(join(logs, 'main.log.1'), 'utf8')).toBe('x'.repeat(50))
  })

  it('hides what is shaped like a secret when written and when read back', async () => {
    const token = '0123456789abcdef'.repeat(4)
    const log = createMainLog(logs)
    log.write(`sent Authorization: Bearer ${token} and ${token}`)
    expect(readFileSync(log.path, 'utf8')).not.toContain(token)
    writeFileSync(log.path, `bearer abc.def\n`, { flag: 'a' })
    const lines = await log.tail(10)
    expect(lines).toEqual([`sent Authorization: Bearer ${HIDDEN} and ${HIDDEN}`, `bearer ${HIDDEN}`])
    // A fingerprint, a few bytes, stays.
    expect(hideSecrets('device 1a2b3c4d5e6f7a8b attached')).toBe('device 1a2b3c4d5e6f7a8b attached')
  })

  it('hides known key shapes and a bearer with a colon, as the host does', () => {
    const cases: [string, string][] = [
      ['Bearer: abc.def next', `Bearer: ${HIDDEN} next`],
      ['authorization bearer:xyz', `authorization bearer:${HIDDEN}`],
      ['key sk-ant-api03-AbC_d-9 was set', `key sk-ant-${HIDDEN} was set`],
      ['GH_TOKEN=ghp_A1b2C3', `GH_TOKEN=ghp_${HIDDEN}`],
      ['(gho_x) (ghu_y) (ghs_z)', `(gho_${HIDDEN}) (ghu_${HIDDEN}) (ghs_${HIDDEN})`],
      ['pat github_pat_11AB_cd, then', `pat github_pat_${HIDDEN}, then`]
    ]
    for (const [line, said] of cases) expect(hideSecrets(line)).toBe(said)
    for (const kept of ['mysk-ant-x', 'the ghp_ prefix', 'bearers carry', 'Bearer:']) expect(hideSecrets(kept)).toBe(kept)
  })

  it('tails the latest lines oldest first, at most 500, only those holding the word in any case', async () => {
    const log = createMainLog(logs)
    expect(await log.tail(5)).toBeNull()
    for (let n = 0; n < 700; n += 1) log.write(n % 2 === 0 ? `even ${String(n)} Plugin` : `odd ${String(n)}`)
    writeFileSync(log.path, '\r\n\r\n', { flag: 'a' })
    expect(await log.tail(2)).toEqual(['even 698 Plugin', 'odd 699'])
    expect(await log.tail(2, ' plugin ')).toEqual(['even 696 Plugin', 'even 698 Plugin'])
    const most = await log.tail(10_000)
    expect(most).toHaveLength(TAIL_MOST)
    expect(most?.[0]).toBe('even 200 Plugin')
    expect(await log.tail(0)).toEqual([])
  })

  it('stops writing after a failure, saying so once', () => {
    writeFileSync(join(root, 'a file'), '')
    const failures: string[] = []
    const log = createMainLog(join(root, 'a file', 'logs'), { onFailure: (error) => failures.push(error.message) })
    log.write('one')
    log.write('two')
    expect(failures).toHaveLength(1)
    expect(existsSync(join(root, 'a file', 'logs'))).toBe(false)
  })

  it('opens each run with its version, process and start in UTC', () => {
    expect(startLine('2.4.0', new Date(Date.UTC(2026, 10, 17, 9, 30)), 4242)).toBe(
      'floti desktop 2.4.0 started at 2026-11-17T09:30:00.000Z, process 4242'
    )
  })

  it('takes every console line, stamped, a warning or an error with its level, and puts the console back', () => {
    const written: string[] = []
    const printed: unknown[][] = []
    const target = {
      log: (...args: unknown[]) => printed.push(args),
      info: (...args: unknown[]) => printed.push(args),
      debug: (...args: unknown[]) => printed.push(args),
      warn: (...args: unknown[]) => printed.push(args),
      error: (...args: unknown[]) => printed.push(args)
    } as unknown as Console
    const original = target.log
    const at = new Date(Date.UTC(2026, 0, 2, 3, 4, 5))
    const restore = teeConsole({ write: (line) => written.push(line) }, target, () => at)
    target.log('host: %s ready', 'local', 3)
    target.warn(new Error('slow'))
    expect(printed[0]).toEqual(['host: %s ready', 'local', 3])
    expect(written[0]).toBe('2026-01-02T03:04:05.000Z host: local ready 3')
    expect(written[1]).toMatch(/^2026-01-02T03:04:05\.000Z warn Error: slow/)
    restore()
    expect(target.log).toBe(original)
  })
})
