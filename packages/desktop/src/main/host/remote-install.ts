import type { ChildProcess } from 'node:child_process'
import { randomUUID } from 'node:crypto'
import { createReadStream } from 'node:fs'
import {
  chooseSource,
  compareVersions,
  flotiLine,
  flotiPathProblem,
  formatProtocol,
  hasFlotiUpdate,
  hostFlotiOlder,
  installFolderFor,
  judgeProtocol,
  parseShortVersion,
  parseSystemLine,
  parseVersionLine,
  releaseFileName,
  parseStandaloneProbe,
  saysNotFound,
  standaloneProbeLine,
  SETUP_AT_NEXT_SIGN_IN_EXIT,
  SYSTEM_LINE,
  UPDATE_PATIENCE_MS,
  UPDATE_UP_TO_DATE_EXIT,
  systemOfBuild,
  type FlotiFound,
  type FlotiOs,
  type FlotiUpdateVia,
  type HostCheck,
  type HostTransport,
  type InstallOutcome,
  type ProtocolVersionNumbers,
  type RemoteSystem
} from '@helm/core/types'
import type { HostConnection, HostSubscription } from './client'
import type * as P from './protocol'
import { runsAsWritten, systemWsl, transportArgv, type TransportPrograms } from './remote-launcher'

/**
 * Installing and updating floti on a host's computer, over the same login
 * this device reaches the host by: `ssh -o BatchMode=yes <target> <line>`, or
 * the user's command with the line last, run without a shell on this PC.
 *
 * - The check changes nothing on either computer. It runs `floti version`
 *   there (the floti path the host keeps, when it keeps one) and, only when
 *   that says floti is not there, `SYSTEM_LINE` for its OS and CPU, then
 *   `standaloneProbeLine`, which finds a floti where the install scripts
 *   put it on a login whose PATH lacks that folder (a non-interactive ssh
 *   login on macOS) and keeps it by its full path; for a
 *   floti too old to talk to, also `floti update-stop --ask`, which counts the
 *   sessions an update would end. Compatibility is judged from the protocol
 *   `floti version` names, as the host judges a hello (P6).
 * - What it found is kept here under an id, with the plan of what to do; the
 *   window gets the offer and answers with that id, or cancels it. The
 *   window never says what to run.
 * - Nothing is copied or run there before `install`. Then an update first
 *   stops the old host for the update (`floti update-stop --stop`, which
 *   keeps its sessions' rows to offer back), then the install script is sent
 *   with this Helm's own floti on its standard input when that computer's OS
 *   and CPU are this PC's, or with the release's address otherwise, which
 *   that computer downloads from itself. Either way the script checks the
 *   file's SHA-256 there before it moves it into place, so a floti that was
 *   there keeps working when anything fails. Then `<path> setup` there.
 * - Every failure is the last line written there, unedited, with the step it
 *   failed at.
 *
 * An added host's Update floti (`checkUpdate`) offers the same, and more:
 * a host whose floti is older than this Helm's gets the offer even while
 * this Helm can talk to it. Where the floti there has `floti update`
 * (`hasFlotiUpdate`), the update is that, which relaunches the host through
 * its session keeper, so it stops nothing first and ends no session the new
 * host can take back:
 *
 * - A host this Helm is connected to, which keeps floti up to date
 *   (`update:state` names no reason it does not) and whose own check found
 *   a newer floti on its channel, is asked over that connection
 *   (`update:install`), and the offer names that version. It runs
 *   `floti update --from-desktop`, which, on a computer that also has the
 *   desktop app, closes that app (keeping its sessions), replaces it,
 *   relaunches the host once and opens the app again. Its steps are heard on
 *   `update:state` and shown as it goes. It went well only once the host's
 *   link connects again on a newer floti (`hostVersionAfter`).
 * - Any other host whose floti has it runs `floti update --floti-only` over
 *   the same login, which replaces floti alone, never waits for an app, and
 *   exits `UPDATE_UP_TO_DATE_EXIT` when its channel has nothing newer. Its
 *   output is shown as it goes.
 * - A floti without it, a connected host that does not keep floti up to
 *   date (a floti inside a portable app, a development build) or whose
 *   channel has nothing newer, and a host whose `floti update` found nothing
 *   newer (`install`), are updated by the install above.
 *
 * Every wait on a `floti update` ends: at the user's Stop waiting
 * (`cancel`), or after `UPDATE_PATIENCE_MS`. Either says it stopped
 * waiting, and the update may still run there.
 *
 * A `floti update` offer starts at once: Settings > Hosts runs it as the
 * check answers, with no question, and shows it running. Only an install,
 * which stops the host there and ends its sessions, says first what it does
 * and waits for a yes. The offer carries how many sessions a `floti update`
 * ends, where it can be told, for the update's first line: for an update to
 * this Helm's own version, none when the host's session keeper speaks the
 * keeper link this Helm's floti takes back and every running one when it
 * does not, as Restart host to update says it for this PC's host
 * (`restartAsk`, `askRemoteHostRestart`). Otherwise it says nothing of them.
 */

/** How one run of the transport ended, with what it wrote. */
export interface TransportRun {
  code: number | null
  stdout: string
  stderr: string
  /** Why the transport's program could not be run on this PC at all. */
  error?: string
  /** The transport's program is not on this PC (`ENOENT`): no ssh, or no such command. */
  notFound?: boolean
}

/** How a run's last argument goes on its command line. */
export interface RunHow {
  /**
   * The last argument is appended to the command line as written, never
   * quoted as one argument, and the program runs with `WSL_UTF8=1`: for this
   * PC's `wsl.exe` (`runsAsWritten`).
   */
  asWritten?: boolean
  /** Told the last whole line the program has written on its standard output, each time it changes. */
  progress?: (line: string) => void
}

/**
 * Runs `argv` without a shell, with the file at `stdinFile` as its standard
 * input, or none. When `signal` aborts, the run is stopped: its program ends.
 */
export type RunTransport = (
  argv: readonly string[],
  stdinFile: string | null,
  signal?: AbortSignal,
  how?: RunHow
) => Promise<TransportRun>

export interface RemoteInstallDeps {
  run: RunTransport
  /** The programs that run this PC's ssh and `wsl.exe`: `ssh` and System32's when absent. */
  programs?: TransportPrograms
  /** This process's environment, where System32 is found when `wsl` is absent. */
  env?: NodeJS.ProcessEnv
  /** This PC's `process.platform` and `process.arch`. */
  platform: string
  arch: string
  /** This Helm's version, which is floti's. */
  version: string
  /** The protocol this Helm speaks. */
  protocol: ProtocolVersionNumbers
  /** This Helm's own floti and its SHA-256, or null when it carries none. */
  ownBuild: () => Promise<{ path: string; sha256: string } | null>
  /** The release's SHA-256 list this Helm carries (`floti-SHA256SUMS`), or null when it carries none. */
  sums: () => Promise<ReadonlyMap<string, string> | null>
  /** The install script for a computer of `os`: install.ps1 on Windows, install.sh elsewhere. */
  script: (os: FlotiOs) => Promise<string>
  /** The folder of the release of `version` its files are downloaded from. */
  releaseBase: (version: string) => string
  newId?: () => string
  /**
   * What restarting `host` would mean, asked in place of a `hello`
   * (`askRemoteHostRestart`): its sessions and its session keeper's link.
   */
  restartState: (host: UpdateTarget) => Promise<P.RestartState>
  /** The keeper link the floti this Helm carries takes back (`KEEPER_LINK_VERSION`). */
  keeperLink: number
  /**
   * The floti version host `id`'s link next connects with, or null once
   * `signal` aborts first (`RemoteHostLinks.reconnected`).
   */
  hostVersionAfter: (id: string, signal: AbortSignal) => Promise<string | null>
  /** How long a `floti update` is waited on: `UPDATE_PATIENCE_MS` unless a test says. */
  patienceMs?: number
  /**
   * How long an update that ended its run with the host still connected is
   * given for that connection to end, as the host's relaunch ends it:
   * `RELAUNCH_GRACE_MS` unless a test says.
   */
  graceMs?: number
  setTimeout?: (run: () => void, ms: number) => unknown
  clearTimeout?: (timer: unknown) => void
  log: (line: string) => void
}

/** How long an update that ended its run is given for its host's relaunch to end the connection. */
export const RELAUNCH_GRACE_MS = 15_000

/** An added host, as Update floti checks it. */
export interface UpdateTarget {
  id: string
  transport: HostTransport
  floti?: string | undefined
}

export interface RemoteInstaller {
  /**
   * What is on that computer, and the offer when floti is missing or too old
   * there. `signal` stops a check under way: the add-a-host dialog's Cancel.
   */
  check(target: { transport: HostTransport; floti?: string | undefined }, signal?: AbortSignal): Promise<HostCheck>
  /**
   * What updating added host `host` offers: as `check`, and an update for a
   * floti older than this Helm's that it can still talk to, through
   * `floti update` wherever the floti there has it. `connection` is main's
   * connection to that host now, or null. With `install`, an install is
   * offered over the floti there, never `floti update`.
   */
  checkUpdate(host: UpdateTarget, connection: HostConnection | null, options?: { install?: boolean }): Promise<HostCheck>
  /**
   * Does what the check with `id` offered. Rejects for an id no check gave,
   * or one used or cancelled. A `floti update` tells each line it says to
   * `progress` as it goes.
   */
  install(id: string, progress?: (line: string) => void): Promise<InstallOutcome>
  /** Forgets the offer: nothing is run there. For a `floti update` under way, stops waiting for it. */
  cancel(id: string): void
}

/** An install, or an update by installing over the floti there. */
interface InstallPlan {
  kind: 'install'
  transport: HostTransport
  /** The floti an update replaces, by the path the host keeps or on `PATH`. */
  floti: string | undefined
  action: 'install' | 'update'
  os: FlotiOs
  cpu: string
  source: { how: 'copy' } | { how: 'download'; file: string; sha256: string }
}

/** A `floti update` there, over the host's connection or its login. */
type UpdatePlan = { kind: 'flotiUpdate'; host: UpdateTarget; floti: string | undefined; found: FlotiFound } & (
  | { via: 'connection'; connection: HostConnection }
  | { via: 'login' }
)

type Plan = InstallPlan | UpdatePlan

/** What an update for an added host is checked with. */
interface ForUpdate {
  host: UpdateTarget
  connection: HostConnection | null
  /** Never `floti update`: an install over the floti there. */
  install: boolean
}

/** `update-stop`'s status when no host is running there, and `floti`'s for usage it does not know. */
const NO_HOST_EXIT = 3
const USAGE_EXIT = 2

/** What a run said last: the program's failure, its last line on standard error, or on standard output. */
const CLIXML = /^(#< CLIXML|<Objs[ >])/

export function lastLineOf(run: TransportRun): string {
  if (run.error !== undefined) return run.error
  for (const text of [run.stderr, run.stdout]) {
    const lines = text.split(/\r?\n/).map((line) => line.trimEnd())
    // PowerShell writes its progress and other streams to a redirected
    // standard error as CLIXML records, `#< CLIXML` and then `<Objs ...>`:
    // not a line anybody wrote.
    const last = lines.filter((line) => line.trim() !== '' && !CLIXML.test(line)).at(-1)
    if (last !== undefined) return last
  }
  return run.code === null ? 'The command was stopped before it said anything.' : `It exited with status ${String(run.code)}.`
}

/**
 * A check that could not run there: the last line, everything written for
 * what the last line alone cannot tell (a changed host key and an unknown one
 * end on the same line), and whether the transport's program is missing here.
 */
function failedCheck(run: TransportRun): HostCheck {
  const said = `${run.stderr}\n${run.stdout}`.trim()
  return {
    kind: 'failed',
    lastLine: lastLineOf(run),
    ...(said !== '' ? { said } : {}),
    ...(run.notFound === true ? { cause: 'transportMissing' as const } : {})
  }
}

/** The first line the login printed before `floti version`'s own line, or null when that line came first. */
export function firstLineBeforeVersion(stdout: string): string | null {
  const first = stdout
    .split(/\r?\n/)
    .map((line) => line.trimEnd())
    .find((line) => line.trim() !== '')
  if (first === undefined || parseVersionLine(first) !== null) return null
  return first
}

/** A word for a POSIX shell: single-quoted, each quote in it closed, escaped and reopened. */
function shellWord(word: string): string {
  return `'${word.replace(/'/g, `'\\''`)}'`
}

/**
 * The line that runs the install script there with `args`: through `sh -c`
 * with the script as its one argument on a POSIX computer, and through
 * PowerShell's `-EncodedCommand` on Windows, whose line `cmd` (the shell
 * Windows' OpenSSH runs it in) passes on untouched. The script's comment lines
 * are left out there, to keep the line inside `cmd`'s limit.
 */
export function installLine(os: FlotiOs, script: string, args: readonly string[]): string {
  const text = script.replace(/\r\n/g, '\n')
  if (os !== 'windows') return `sh -c ${shellWord(text)} floti-install ${args.map(shellWord).join(' ')}`
  const body = text
    .split('\n')
    .filter((line) => !line.trimStart().startsWith('#') && line.trim() !== '')
    .join('\n')
  const call = `& {\n${body}\n} ${args.map((arg) => `'${arg.replace(/'/g, "''")}'`).join(' ')}`
  const encoded = Buffer.from(call, 'utf16le').toString('base64')
  return `powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand ${encoded}`
}

/** Characters `cmd` reads rather than passes on, outside double quotes. */
const CMD_READS = /[|&<>^%()]/

/**
 * The line a distro's shell is given: as it is when `cmd` (which Windows'
 * OpenSSH runs a line in, before `wsl.exe`) would pass it on unaltered, and
 * otherwise as `eval "$(echo <base64> | base64 -d)"`, whose only `|` is in
 * double quotes, so the line reaches the distro's shell as it was written,
 * with standard input left for the line. floti's own lines go as they are.
 */
export function wslLine(line: string): string {
  if (!CMD_READS.test(line)) return line
  return `eval "$(echo ${Buffer.from(line, 'utf8').toString('base64')} | base64 -d)"`
}

/**
 * What a program wrote, as text: UTF-16LE when it is (what `wsl.exe` writes to
 * a pipe without `WSL_UTF8=1`, as it always does through ssh), else UTF-8.
 * The same judgement as `decode_output` in crates/host/src/bridge.rs: a
 * byte-order mark, or every other byte mostly zero, as ASCII is in UTF-16LE
 * and never in UTF-8.
 */
export function decodeOutput(bytes: Uint8Array): string {
  const pairs = Math.floor(bytes.length / 2)
  let utf16 = bytes[0] === 0xff && bytes[1] === 0xfe
  if (!utf16 && pairs > 0) {
    let highZero = 0
    let lowZero = 0
    for (let i = 0; i + 1 < bytes.length; i += 2) {
      if (bytes[i + 1] === 0) highZero += 1
      if (bytes[i] === 0) lowZero += 1
    }
    utf16 = highZero * 2 > pairs && lowZero * 4 < pairs
  }
  if (!utf16) return new TextDecoder('utf-8').decode(bytes)
  return new TextDecoder('utf-16le').decode(bytes.subarray(0, pairs * 2)).replace(/^\uFEFF/, '')
}

export function createRemoteInstaller(deps: RemoteInstallDeps): RemoteInstaller {
  const plans = new Map<string, Plan>()
  const newId = deps.newId ?? randomUUID

  const programs: TransportPrograms = {
    ...deps.programs,
    wsl: deps.programs?.wsl ?? systemWsl(deps.env ?? {})
  }
  const runThere = (
    transport: HostTransport,
    line: string,
    stdinFile: string | null = null,
    signal?: AbortSignal,
    progress?: (line: string) => void
  ): Promise<TransportRun> => {
    const argv = [...transportArgv(transport, programs), transport.kind === 'wsl' ? wslLine(line) : line]
    const how: RunHow = {
      ...(runsAsWritten(transport) ? { asWritten: true } : {}),
      ...(progress !== undefined ? { progress } : {})
    }
    if (Object.keys(how).length > 0) return deps.run(argv, stdinFile, signal, how)
    return signal === undefined ? deps.run(argv, stdinFile) : deps.run(argv, stdinFile, signal)
  }

  /** How many sessions the floti there says an update ends, or null when it cannot say. */
  const sessionsThere = async (transport: HostTransport, floti: string | undefined): Promise<number | null> => {
    const asked = await runThere(transport, flotiLine(floti, 'update-stop --ask'))
    if (asked.code === NO_HOST_EXIT) return 0
    const count = asked.stdout.trim()
    return asked.code === 0 && /^\d+$/.test(count) ? Number(count) : null
  }

  /** The OS and CPU there, or what the line said instead. */
  const systemThere = async (transport: HostTransport, signal?: AbortSignal): Promise<RemoteSystem | HostCheck> => {
    const said = await runThere(transport, SYSTEM_LINE, null, signal)
    const system = said.code === 0 ? parseSystemLine(said.stdout) : null
    return system ?? failedCheck(said)
  }

  const offer = async (
    transport: HostTransport,
    floti: string | undefined,
    system: RemoteSystem,
    found: FlotiFound | null,
    talks = false
  ): Promise<HostCheck> => {
    const own = await deps.ownBuild()
    const source = chooseSource({
      system,
      desktop: { platform: deps.platform, arch: deps.arch },
      ownBuild: own !== null,
      version: deps.version,
      sums: await deps.sums()
    })
    if (source.how === 'none' || system.os === null) {
      return { kind: 'noBuild', system, reason: source.how === 'none' ? source.reason : `There is no floti for ${system.label} yet.` }
    }
    const action = found === null ? 'install' : 'update'
    const id = newId()
    plans.set(id, { kind: 'install', transport, floti, action, os: system.os, cpu: system.cpu, source })
    return {
      kind: 'offer',
      id,
      offer: {
        system,
        found,
        action,
        how: source.how,
        version: deps.version,
        folder: installFolderFor(system.os),
        sessions: action === 'update' ? await sessionsThere(transport, floti) : null,
        talks
      }
    }
  }

  /**
   * How many sessions a `floti update` to `updatesTo` ends on `host`: for
   * this Helm's own version, none when the host's keeper speaks the link
   * this Helm's floti takes back and every running one when it does not;
   * null for any other version, whose keeper link this Helm cannot know, or
   * when the host could not be asked.
   */
  const endsThere = async (host: UpdateTarget, updatesTo: string | null): Promise<number | null> => {
    if (updatesTo === null || compareVersions(updatesTo, deps.version) !== 0) return null
    try {
      const state = await deps.restartState(host)
      return state.keeperLink === deps.keeperLink ? 0 : state.sessions
    } catch (error) {
      deps.log(`host: could not ask what updating floti there ends (${(error as Error).message})`)
      return null
    }
  }

  /** The offer to run `floti update` on the host, over `connection` when given, otherwise over its login. */
  const flotiUpdateOffer = async (
    host: UpdateTarget,
    connection: HostConnection | null,
    updatesTo: string | null,
    floti: string | undefined,
    system: RemoteSystem,
    found: FlotiFound,
    talks: boolean
  ): Promise<HostCheck> => {
    const id = newId()
    const via: FlotiUpdateVia = connection === null ? 'login' : 'connection'
    plans.set(
      id,
      connection === null
        ? { kind: 'flotiUpdate', via: 'login', host, floti, found }
        : { kind: 'flotiUpdate', via: 'connection', connection, host, floti, found }
    )
    return {
      kind: 'offer',
      id,
      offer: {
        system,
        found,
        action: 'update',
        how: 'flotiUpdate',
        via,
        updatesTo,
        version: deps.version,
        folder: system.os === null ? '' : installFolderFor(system.os),
        sessions: await endsThere(host, updatesTo),
        talks
      }
    }
  }

  /**
   * The update a floti found there is offered, too old to talk to or, for
   * an added host's Update floti, older than this Helm's: `floti update`
   * where it has it (over the host's connection when that host keeps floti
   * up to date and its channel has a newer one, over its login when there is
   * no connection), otherwise an install over it.
   */
  const updateOffer = async (
    transport: HostTransport,
    floti: string | undefined,
    system: RemoteSystem,
    found: FlotiFound,
    protocol: ProtocolVersionNumbers,
    talks: boolean,
    forUpdate: ForUpdate | null
  ): Promise<HostCheck> => {
    if (forUpdate === null || forUpdate.install || !hasFlotiUpdate(protocol)) {
      return offer(transport, floti, system, found, talks)
    }
    const { host, connection } = forUpdate
    if (connection === null) return flotiUpdateOffer(host, null, null, floti, system, found, talks)
    // Over the connection only for a host that keeps floti up to date and
    // whose own check found a newer floti: anything else would replace nothing.
    const state = await updateStateOf(connection)
    const newer =
      state !== null &&
      state.unavailable === null &&
      state.newest !== null &&
      (compareVersions(state.newest, state.running) ?? 0) > 0
    if (state !== null && newer) {
      return flotiUpdateOffer(host, connection, state.newest, floti, system, found, talks)
    }
    return offer(transport, floti, system, found, talks)
  }

  /** Whether a floti this Helm talks to is offered an update: only for an added host's Update floti, and only when older. */
  const olderForUpdate = (found: FlotiFound, forUpdate: ForUpdate | null): boolean =>
    forUpdate !== null && hostFlotiOlder(found.version, deps.version)

  /**
   * floti where the install scripts put it, on a login whose PATH does not
   * have it: judged as a floti on PATH is, and kept by its full path. Null
   * when there is none there.
   */
  const standaloneThere = async (
    transport: HostTransport,
    os: FlotiOs,
    signal: AbortSignal | undefined,
    forUpdate: ForUpdate | null
  ): Promise<HostCheck | null> => {
    const probed = await runThere(transport, standaloneProbeLine(os), null, signal)
    if (probed.error !== undefined || probed.code !== 0) return null
    const there = parseStandaloneProbe(probed.stdout)
    if (there === null || flotiPathProblem(there.path) !== null) return null
    const { protocolNumbers, os: builtFor, cpu, ...named } = there.found
    const verdict = judgeProtocol(protocolNumbers, deps.protocol)
    const system = systemOfBuild(builtFor, cpu)
    if (verdict === 'newer') return { kind: 'newer', found: named, ours: formatProtocol(deps.protocol), helm: deps.version }
    if (verdict === 'ok' && !olderForUpdate(named, forUpdate)) {
      return { kind: 'ready', found: named, system, floti: there.path }
    }
    return updateOffer(transport, there.path, system, named, protocolNumbers, verdict === 'ok', forUpdate)
  }

  /** What is on that computer; with `forUpdate`, an older floti there is offered an update too. */
  const inspect = async (
    { transport, floti }: { transport: HostTransport; floti?: string | undefined },
    signal: AbortSignal | undefined,
    forUpdate: ForUpdate | null
  ): Promise<HostCheck> => {
    const asked = await runThere(transport, flotiLine(floti, 'version'), null, signal)
    if (asked.error !== undefined) return failedCheck(asked)
    const found = asked.code === 0 ? parseVersionLine(asked.stdout) : null
    if (found !== null) {
      // floti's connection goes where its version line went: anything the
      // login printed before it would land in the middle of the handshake.
      const printed = firstLineBeforeVersion(asked.stdout)
      if (printed !== null) return { kind: 'failed', lastLine: printed, cause: 'loginOutput' }
      const { protocolNumbers, os, cpu, ...named } = found
      const verdict = judgeProtocol(protocolNumbers, deps.protocol)
      if (verdict === 'newer') {
        return { kind: 'newer', found: named, ours: formatProtocol(deps.protocol), helm: deps.version }
      }
      if (verdict === 'ok' && !olderForUpdate(named, forUpdate)) {
        return { kind: 'ready', found: named, system: systemOfBuild(os, cpu) }
      }
      return updateOffer(transport, floti, systemOfBuild(os, cpu), named, protocolNumbers, verdict === 'ok', forUpdate)
    }
    const said = `${asked.stderr}\n${asked.stdout}`
    // A floti from before `floti version`: too old, whatever it is.
    const old = asked.code === USAGE_EXIT && /^usage: floti /m.test(said)
    if (!old && !saysNotFound(asked.code, said)) return failedCheck(asked)
    const system = await systemThere(transport, signal)
    if ('kind' in system) return system
    if (!old) {
      if (floti === undefined && system.os !== null) {
        const there = await standaloneThere(transport, system.os, signal, forUpdate)
        if (there !== null) return there
      }
      return offer(transport, floti, system, null)
    }
    const short = await runThere(transport, flotiLine(floti, '--version'))
    const version = (short.code === 0 ? parseShortVersion(short.stdout) : null) ?? 'of unknown version'
    return offer(transport, floti, system, { version, protocol: null })
  }

  /** Updates waited on now, by offer id: `cancel` stops waiting. */
  const waiting = new Map<string, AbortController>()
  const setTimer = deps.setTimeout ?? ((run, ms) => setTimeout(run, ms))
  const clearTimer = deps.clearTimeout ?? ((timer) => clearTimeout(timer as ReturnType<typeof setTimeout>))

  /**
   * `floti update --floti-only` over the host's login, its output told as it
   * goes: it went well, the host relaunched on the new floti, when it exits
   * 0; nothing was newer when it exits `UPDATE_UP_TO_DATE_EXIT`.
   */
  const updateOverLogin = async (
    plan: UpdatePlan,
    signal: AbortSignal,
    progress: (line: string) => void
  ): Promise<InstallOutcome> => {
    const ran = await runThere(plan.host.transport, flotiLine(plan.floti, 'update --floti-only'), null, signal, progress)
    if (signal.aborted) return { ok: null, ended: 'stoppedWaiting', lastLine: lastSaid(ran) }
    if (ran.code === UPDATE_UP_TO_DATE_EXIT) return { ok: null, ended: 'upToDate', lastLine: lastLineOf(ran) }
    if (ran.code !== 0) return { ok: false, step: 'update', lastLine: lastLineOf(ran) }
    deps.log('host: floti update ran there over its login')
    return { ok: true, ...(plan.floti !== undefined ? { floti: plan.floti } : {}), startsAtNextSignIn: false }
  }

  /**
   * `floti update` over the host's connection (`update:install`), its steps
   * told as they come: it went well only once the host's link connects again
   * on a floti newer than the one found there.
   */
  const updateOverConnection = async (
    plan: UpdatePlan & { via: 'connection' },
    signal: AbortSignal,
    progress: (line: string) => void
  ): Promise<InstallOutcome> => {
    const end = await followUpdate(plan.connection, {
      signal,
      progress,
      graceMs: deps.graceMs ?? RELAUNCH_GRACE_MS,
      setTimeout: setTimer,
      clearTimeout: clearTimer
    })
    if (end.kind === 'failed') return { ok: false, step: 'update', lastLine: end.lastLine }
    if (end.kind === 'stopped') return { ok: null, ended: 'stoppedWaiting', lastLine: end.lastLine }
    if (end.kind === 'quiet') return { ok: null, ended: 'unconfirmed', lastLine: end.lastLine }
    progress('The host there is starting the new floti; waiting for it to connect again.')
    const version = await deps.hostVersionAfter(plan.host.id, signal)
    if (version === null) return { ok: null, ended: 'stoppedWaiting', lastLine: end.lastLine }
    if ((compareVersions(version, plan.found.version) ?? 0) <= 0) {
      return { ok: null, ended: 'unconfirmed', lastLine: `The host there connected again on floti ${version}.` }
    }
    deps.log(`host: floti update ran there; its host connected again on floti ${version}`)
    return { ok: true, ...(plan.floti !== undefined ? { floti: plan.floti } : {}), startsAtNextSignIn: false }
  }

  /** Runs a `floti update` plan, waiting on it until it ends, `cancel` stops waiting, or the patience runs out. */
  const runUpdate = async (id: string, plan: UpdatePlan, progress: (line: string) => void): Promise<InstallOutcome> => {
    const stop = new AbortController()
    waiting.set(id, stop)
    const timer = setTimer(() => stop.abort(), deps.patienceMs ?? UPDATE_PATIENCE_MS)
    try {
      return plan.via === 'connection'
        ? await updateOverConnection(plan, stop.signal, progress)
        : await updateOverLogin(plan, stop.signal, progress)
    } finally {
      clearTimer(timer)
      waiting.delete(id)
    }
  }

  return {
    check: (target, signal) => inspect(target, signal, null),

    checkUpdate: (host, connection, options) =>
      inspect(host, undefined, { host, connection, install: options?.install === true }),

    async install(id, progress = () => undefined) {
      const plan = plans.get(id)
      if (plan === undefined) throw new Error('That offer is no longer open. Check the host again.')
      plans.delete(id)
      if (plan.kind === 'flotiUpdate') return runUpdate(id, plan, progress)
      const { transport } = plan
      if (plan.action === 'update') {
        const stop = await runThere(transport, flotiLine(plan.floti, 'update-stop --stop'))
        // No host running there is nothing to stop; a floti too old to stop
        // itself for an update is replaced as it runs.
        if (stop.code !== 0 && stop.code !== NO_HOST_EXIT && stop.code !== USAGE_EXIT) {
          return { ok: false, step: 'stop', lastLine: lastLineOf(stop) }
        }
      }
      const script = await deps.script(plan.os)
      let args: string[]
      let stdinFile: string | null = null
      if (plan.source.how === 'copy') {
        const own = await deps.ownBuild()
        if (own === null) return { ok: false, step: 'install', lastLine: 'This Floti no longer has its own floti to send.' }
        args = [deps.version, releaseFileName(deps.version, plan.os, plan.cpu), own.sha256, '-']
        stdinFile = own.path
      } else {
        args = [deps.version, plan.source.file, plan.source.sha256, deps.releaseBase(deps.version)]
      }
      const put = await runThere(transport, installLine(plan.os, script, args), stdinFile)
      if (put.code !== 0) return { ok: false, step: 'install', lastLine: lastLineOf(put) }
      const path = put.stdout.split(/\r?\n/).map((line) => line.trim()).filter((line) => line !== '').at(-1) ?? ''
      const problem = flotiPathProblem(path)
      if (problem !== null) return { ok: false, step: 'install', lastLine: `floti was installed, but ${problem}.` }
      deps.log(`host: installed floti ${deps.version} at ${path} there`)
      const setup = await runThere(transport, flotiLine(path, 'setup'))
      if (setup.code === 0) return { ok: true, floti: path, startsAtNextSignIn: false }
      if (setup.code === SETUP_AT_NEXT_SIGN_IN_EXIT) return { ok: true, floti: path, startsAtNextSignIn: true }
      return { ok: false, step: 'setup', lastLine: lastLineOf(setup) }
    },

    cancel(id) {
      plans.delete(id)
      waiting.get(id)?.abort()
    }
  }
}

/** What a run that was stopped had said last, or null when it said nothing. */
function lastSaid(run: TransportRun): string | null {
  const said = `${run.stderr}\n${run.stdout}`.trim()
  return said === '' ? null : lastLineOf(run)
}

/**
 * The host's `update:state` as it stands, from a subscription's first state;
 * null when it could not be heard (a host from before the channel, or a
 * connection that ended).
 */
export function updateStateOf(connection: Pick<HostConnection, 'subscribe'>): Promise<P.UpdateState | null> {
  return new Promise((done) => {
    let settled = false
    let subscription: HostSubscription | null = null
    const settle = (state: P.UpdateState | null): void => {
      if (settled) return
      settled = true
      subscription?.unsubscribe()
      done(state)
    }
    subscription = connection.subscribe('update:state', {
      snapshot: settle,
      event: settle,
      error: () => settle(null)
    })
    if (settled) subscription.unsubscribe()
  })
}

/** How following a `floti update` over a host's connection ended. */
export type FollowEnd =
  /** The connection ended once the host had answered: as the host's relaunch ends it. */
  | { kind: 'ended'; lastLine: string | null }
  /** The update failed (`installFailed`), or the host refused or never answered `update:install`. */
  | { kind: 'failed'; lastLine: string }
  /** The update ended its run and the connection outlived the grace: no relaunch was seen. */
  | { kind: 'quiet'; lastLine: string | null }
  /** The wait was stopped. */
  | { kind: 'stopped'; lastLine: string | null }

export interface FollowOptions {
  /** Stops the wait. */
  signal: AbortSignal
  /** Told each step the update says, as it says it. */
  progress: (line: string) => void
  /** How long an update that ended its run is given for the connection to end. */
  graceMs: number
  setTimeout: (run: () => void, ms: number) => unknown
  clearTimeout: (timer: unknown) => void
}

/**
 * Asks the host to run `floti update` (`update:install`) and follows it on
 * `update:state` until it ends:
 *
 * - The connection ending once the host has answered is `ended`: the host
 *   relaunching on the new floti ends it, and so would a link that dropped,
 *   which only the next connection tells apart.
 * - `installFailed` is `failed`, with its line; so are a refused
 *   `update:install` and a connection that ended before the host answered.
 * - `installing` going back to none with no failure is not an end by
 *   itself: the old host still tells its state while it stops for the
 *   relaunch, and may tell the update's end before the connection ends. It
 *   is given `graceMs` for the connection to end, and is `quiet` after that.
 * - `signal` aborting is `stopped`.
 */
export function followUpdate(
  connection: Pick<HostConnection, 'subscribe' | 'request' | 'ended'>,
  options: FollowOptions
): Promise<FollowEnd> {
  return new Promise((done) => {
    let settled = false
    let started = false
    let answered = false
    let lastLine: string | null = null
    let grace: unknown = null
    let subscription: HostSubscription | null = null
    const settle = (end: FollowEnd): void => {
      if (settled) return
      settled = true
      subscription?.unsubscribe()
      if (grace !== null) options.clearTimeout(grace)
      options.signal.removeEventListener('abort', stop)
      done(end)
    }
    const stop = (): void => settle({ kind: 'stopped', lastLine })
    const hear = (state: P.UpdateState): void => {
      if (state.installing !== null) {
        started = true
        if (state.installing.line !== lastLine) {
          lastLine = state.installing.line
          options.progress(lastLine)
        }
        return
      }
      if (!started) return
      if (state.installFailed !== null) {
        settle({ kind: 'failed', lastLine: state.installFailed })
        return
      }
      grace ??= options.setTimeout(() => settle({ kind: 'quiet', lastLine }), options.graceMs)
    }
    if (options.signal.aborted) {
      stop()
      return
    }
    options.signal.addEventListener('abort', stop, { once: true })
    // Subscribed first, so no step between the start and the first state is missed.
    subscription = connection.subscribe('update:state', { snapshot: hear, event: hear, error: () => undefined })
    if (settled) subscription.unsubscribe()
    // Only a started update relaunches the host: a connection that ended
    // before the host answered, or had ended already, ran nothing.
    void connection.ended.then((why) => settle(answered ? { kind: 'ended', lastLine } : { kind: 'failed', lastLine: why }))
    connection.request('update:install', null).then(
      () => {
        answered = true
      },
      (error: unknown) => settle({ kind: 'failed', lastLine: error instanceof Error ? error.message : String(error) })
    )
  })
}

/** The most of a run's output kept: its last lines are what is shown. */
const KEPT = 64 * 1024

/**
 * A word on a Windows command line, quoted the way a program's own C runtime
 * splits it back: as it is when it holds no blank or quote, else in double
 * quotes with each quote, and the backslashes before one, escaped.
 */
export function windowsArgument(word: string): string {
  if (word !== '' && !/[\s"]/.test(word)) return word
  let quoted = '"'
  let slashes = 0
  for (const char of word) {
    if (char === '\\') {
      slashes += 1
      continue
    }
    quoted += char === '"' ? '\\'.repeat(slashes * 2 + 1) + '"' : '\\'.repeat(slashes) + char
    slashes = 0
  }
  return `${quoted}${'\\'.repeat(slashes * 2)}"`
}

/** How `spawnTransportRun` starts a program. */
export interface TransportSpawnOptions {
  env: NodeJS.ProcessEnv
  stdio: ['pipe', 'pipe', 'pipe']
  windowsHide: true
  /** Set for a run as written: the command line is the arguments as given. */
  windowsVerbatimArguments?: true
  argv0?: string
}

/**
 * Runs a transport for real: the program and its arguments without a shell,
 * windowless, with `env` as it is (the user's ssh finds their config, keys and
 * agent), and a file piped to its standard input when one is given.
 *
 * A run as written (this PC's `wsl.exe`) gets `WSL_UTF8=1` and, on Windows, a
 * command line built here: every word before the last quoted as its program's
 * runtime reads it (`windowsArgument`), and the last, the distro's line,
 * appended as it is. Elsewhere the line is one more argument. What it writes
 * is read as UTF-16LE when it is (`decodeOutput`).
 */
export function spawnTransportRun(
  spawn: (command: string, args: string[], options: TransportSpawnOptions) => ChildProcess,
  env: NodeJS.ProcessEnv,
  platform: NodeJS.Platform = process.platform
): RunTransport {
  return (argv, stdinFile, signal, how) =>
    new Promise((done) => {
      const [program, ...args] = argv as [string, ...string[]]
      const out: Buffer[] = []
      const err: Buffer[] = []
      const text = (chunks: Buffer[]): string => {
        const all = Buffer.concat(chunks)
        // The last lines are what is shown; an even cut keeps UTF-16 whole.
        return decodeOutput(all.length > KEPT ? all.subarray(all.length - KEPT) : all)
      }
      const keep = (chunks: Buffer[], chunk: Buffer | string): void => {
        chunks.push(typeof chunk === 'string' ? Buffer.from(chunk) : chunk)
        let size = chunks.reduce((sum, each) => sum + each.length, 0)
        while (chunks.length > 1 && size - (chunks[0]?.length ?? 0) >= KEPT) size -= chunks.shift()?.length ?? 0
      }
      let settled = false
      const finish = (run: TransportRun): void => {
        if (settled) return
        settled = true
        signal?.removeEventListener('abort', stop)
        done(run)
      }
      const failedToRun = (error: unknown): TransportRun => ({
        code: null,
        stdout: text(out),
        stderr: text(err),
        error: couldNotRun(program, error),
        ...((error as NodeJS.ErrnoException).code === 'ENOENT' ? { notFound: true } : {})
      })
      let child: ChildProcess | null = null
      // The run this PC started, by its own process: nothing else is stopped.
      const stop = (): void => {
        child?.kill()
      }
      if (signal?.aborted === true) {
        finish({ code: null, stdout: '', stderr: '' })
        return
      }
      signal?.addEventListener('abort', stop, { once: true })
      const options: TransportSpawnOptions = { env, stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true }
      let spawnArgs = args
      if (how?.asWritten === true) {
        options.env = { ...env, WSL_UTF8: '1' }
        if (platform === 'win32') {
          // Node quotes nothing with verbatim arguments, so each word but the
          // line is quoted here, the program's own name included.
          options.windowsVerbatimArguments = true
          options.argv0 = windowsArgument(program)
          spawnArgs = args.map((word, at) => (at === args.length - 1 ? word : windowsArgument(word)))
        }
      }
      try {
        child = spawn(program, spawnArgs, options)
      } catch (error) {
        finish(failedToRun(error))
        return
      }
      let told: string | null = null
      child.stdout?.on('data', (chunk: Buffer | string) => {
        keep(out, chunk)
        if (how?.progress === undefined) return
        // Whole lines only: a line still being written is told once it ends.
        const written = text(out)
        const last = written
          .slice(0, written.lastIndexOf('\n') + 1)
          .split(/\r?\n/)
          .map((line) => line.trim())
          .filter((line) => line !== '')
          .at(-1)
        if (last !== undefined && last !== told) {
          told = last
          how.progress(last)
        }
      })
      child.stderr?.on('data', (chunk: Buffer | string) => keep(err, chunk))
      child.once('error', (error) => finish(failedToRun(error)))
      child.once('close', (code: number | null) => finish({ code, stdout: text(out), stderr: text(err) }))
      const stdin = child.stdin
      if (stdin === null) return
      // A program that exits before reading all of it closes the pipe.
      stdin.on('error', () => undefined)
      if (stdinFile === null) {
        stdin.end()
        return
      }
      const file = createReadStream(stdinFile)
      file.once('error', () => stdin.destroy())
      file.pipe(stdin)
    })
}

function couldNotRun(program: string, error: unknown): string {
  const code = (error as NodeJS.ErrnoException).code
  return `Floti could not run ${program} on this PC (${code ?? (error as Error).message}).`
}
