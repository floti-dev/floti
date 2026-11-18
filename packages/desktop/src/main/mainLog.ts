import * as nodeFs from 'node:fs'
import { join } from 'node:path'
import { format } from 'node:util'

/**
 * Main's log: `main.log` in the host's logs folder, beside `host.log`
 * (`hostLogsFolder`), so the installed app keeps what main prints, where
 * nothing shows its console.
 *
 * Main's `console` writes there as well as to its own output once
 * `teeConsole` is called: every line stamped with the time and, for a warning
 * or an error, its level. Each run starts with a line naming the desktop's
 * version, its process and when it started, as the host's log does.
 *
 * It is bounded as `host.log` is (`host_log.rs`): a log past `MAIN_LOG_LIMIT`
 * becomes `main.log.1`, the one before it `main.log.2`, up to
 * `KEPT_MAIN_LOGS` older files, and a new `main.log` is begun. On macOS and
 * Linux a file it makes is the owner's alone (mode 0600); on Windows the mode
 * means nothing, and the file takes the access of the folder it is in, which
 * is in the user's own data folder.
 *
 * Nothing it writes names a cookie, a token or a key: main logs none
 * (cookies are handled only in `browserCookies.ts`, which logs no name or
 * value), and every line also has what is shaped like a secret hidden
 * (`hideSecrets`) before it is written, and again when it is read back.
 *
 * `tail` reads the latest lines back for `desktop_log`, which a session's
 * agent asks the window for and the window asks main for on `ownLog:tail`.
 */

/** The log's name in the logs folder. */
export const MAIN_LOG_FILE = 'main.log'

/** How large the log may grow before it is rotated: `host.log`'s limit. */
export const MAIN_LOG_LIMIT = 5 * 1024 * 1024

/** How many rotated logs are kept beside `main.log`: `main.log.1` (the most recent) to `main.log.3`. */
export const KEPT_MAIN_LOGS = 3

/** The most lines `tail` answers. */
export const TAIL_MOST = 500

/** What a hidden secret reads as: the host's `HIDDEN`. */
export const HIDDEN = '[hidden]'

/**
 * What is shaped like a secret: the value after `Bearer ` or `Bearer:`, what
 * follows a known key's prefix at the start of a word (Anthropic's `sk-ant-`,
 * GitHub's `ghp_`, `gho_`, `ghu_`, `ghs_` and `github_pat_`), and a run of 64
 * or more hex digits (a token, a hash, a whole key). A key's fingerprint, a
 * few bytes, stays. The host's `hide_secrets` hides the same.
 */
const BEARER = /(bearer(?::|:? +))(\S+)/gi
const KEY = /(?<![A-Za-z0-9_])(sk-ant-|gh[pous]_|github_pat_)[A-Za-z0-9_-]+/g
const HEX_RUN = /[0-9a-f]{64,}/gi

/** `line` with what is shaped like a secret hidden. */
export function hideSecrets(line: string): string {
  return line
    .replace(BEARER, `$1${HIDDEN}`)
    .replace(KEY, `$1${HIDDEN}`)
    .replace(HEX_RUN, HIDDEN)
}

/** The file calls the log makes: node's, or a test's. */
export type MainLogFs = Pick<typeof nodeFs, 'mkdirSync' | 'statSync' | 'renameSync' | 'appendFileSync'> & {
  promises: Pick<typeof nodeFs.promises, 'readFile'>
}

export interface MainLogOptions {
  fs?: MainLogFs
  limit?: number
  /** Told once when a write fails; the log writes nothing more after. */
  onFailure?: (error: Error) => void
}

export interface MainLog {
  /** The log file. */
  readonly path: string
  /** Appends `line`, with its secrets hidden, rotating first when the log is past its limit. */
  write(line: string): void
  /** The last `lines` (at most `TAIL_MOST`) non-empty lines, oldest first, only those holding `word` in any case when one is given; null when there is no log yet; none, read from nothing, for 0. */
  tail(lines: number, word?: string): Promise<string[] | null>
}

const errorOf = (error: unknown): Error => (error instanceof Error ? error : new Error(String(error)))

const isMissing = (error: unknown): boolean => (error as NodeJS.ErrnoException | null)?.code === 'ENOENT'

export function createMainLog(folder: string, options: MainLogOptions = {}): MainLog {
  const fs = options.fs ?? nodeFs
  const limit = options.limit ?? MAIN_LOG_LIMIT
  const path = join(folder, MAIN_LOG_FILE)
  /** The file's size as this run has seen it; null until the first write looks. */
  let size: number | null = null
  let stuck = false

  const numbered = (n: number): string => `${path}.${String(n)}`

  const rotate = (): void => {
    for (let n = KEPT_MAIN_LOGS - 1; n >= 1; n -= 1) {
      try {
        fs.renameSync(numbered(n), numbered(n + 1))
      } catch (error) {
        if (!isMissing(error)) throw error
      }
    }
    fs.renameSync(path, numbered(1))
    size = 0
  }

  const sizeNow = (): number => {
    try {
      return fs.statSync(path).size
    } catch (error) {
      if (isMissing(error)) return 0
      throw error
    }
  }

  return {
    path,
    write(line) {
      if (stuck) return
      try {
        if (size === null) {
          fs.mkdirSync(folder, { recursive: true })
          size = sizeNow()
        }
        if (size > limit) rotate()
        const text = `${hideSecrets(line)}\n`
        fs.appendFileSync(path, text, { mode: 0o600 })
        size += Buffer.byteLength(text)
      } catch (error) {
        stuck = true
        options.onFailure?.(errorOf(error))
      }
    },
    async tail(lines, word) {
      const most = Math.max(0, Math.min(Math.floor(lines), TAIL_MOST))
      // Asked for none, as `desktop_status` asks to learn the path: nothing is read.
      if (most === 0) return []
      let text: string
      try {
        text = await fs.promises.readFile(path, 'utf8')
      } catch (error) {
        if (isMissing(error)) return null
        throw error
      }
      const wanted = word?.trim().toLowerCase() ?? ''
      const kept = text
        .split(/\r?\n/)
        .filter((line) => line !== '' && (wanted === '' || line.toLowerCase().includes(wanted)))
      return kept.slice(-most).map(hideSecrets)
    }
  }
}

/** The line that opens each run's part of the log, in UTC. */
export function startLine(version: string, now: Date, pid: number): string {
  return `floti desktop ${version} started at ${now.toISOString()}, process ${String(pid)}`
}

type ConsoleLevel = 'log' | 'info' | 'debug' | 'warn' | 'error'

const LEVELS: readonly ConsoleLevel[] = ['log', 'info', 'debug', 'warn', 'error']

/**
 * Makes every `target` call also write its line to `log`, stamped by `now`,
 * a warning or an error with its level. Returns what puts `target` back.
 */
export function teeConsole(log: Pick<MainLog, 'write'>, target: Console = console, now: () => Date = () => new Date()): () => void {
  const originals = new Map<ConsoleLevel, (...args: unknown[]) => void>()
  for (const level of LEVELS) {
    const original = target[level] as (...args: unknown[]) => void
    originals.set(level, original)
    target[level] = (...args: unknown[]) => {
      original.apply(target, args)
      const marked = level === 'warn' || level === 'error' ? `${level} ` : ''
      log.write(`${now().toISOString()} ${marked}${format(...args)}`)
    }
  }
  return () => {
    for (const [level, original] of originals) target[level] = original
  }
}
