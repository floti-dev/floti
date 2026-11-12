import { describe, expect, it, vi } from 'vitest'
import { FLOTI_TARGETS, SYSTEM_LINE, standaloneProbeLine, type HostTransport } from '@helm/core/types'
import { RELEASE_TARGETS } from '../../../scripts/floti-release.mjs'
import type { ChildProcess } from 'node:child_process'
import { EventEmitter } from 'node:events'
import { FakeConnection } from '../../../test/fake-connection'
import type { HostSubscription, SubscriptionHandlers } from '../../shared/host/client'
import type * as P from '../../shared/host/protocol'
import type { EventChannels } from '../../shared/host/protocol'
import {
  createRemoteInstaller,
  decodeOutput,
  followUpdate,
  updateStateOf,
  installLine,
  lastLineOf,
  spawnTransportRun,
  windowsArgument,
  wslLine,
  type RemoteInstallDeps,
  type RunHow,
  type TransportRun,
  type TransportSpawnOptions
} from './remote-install'

/**
 * The desktop's check, offer and install on a host's computer, with the
 * transport faked: each run is recorded as the argv this PC would start, and
 * answered as that computer would answer the line.
 */

const SSH: HostTransport = { kind: 'ssh', target: 'me@build-box' }
const BATCH = ['ssh', '-o', 'BatchMode=yes', 'me@build-box']
const OWN_SHA = 'c'.repeat(64)
const LINUX_SHA = 'a'.repeat(64)

type Answer = Partial<TransportRun> | ((line: string, stdinFile: string | null) => Partial<TransportRun>)

interface World {
  installer: ReturnType<typeof createRemoteInstaller>
  /** Every run, in order: its argv and the file sent on its standard input. */
  runs: { argv: readonly string[]; line: string; stdinFile: string | null }[]
  lines: () => string[]
}

/** A computer that answers each line by the first rule whose prefix it starts with. */
function world(rules: [prefix: string, answer: Answer][], options: Partial<RemoteInstallDeps> = {}): World {
  const runs: World['runs'] = []
  const installer = createRemoteInstaller({
    restartState: () => Promise.reject(new Error('Nothing asks what a restart ends here.')),
    keeperLink: 1,
    hostVersionAfter: () => Promise.reject(new Error('Nothing waits for a host to connect again here.')),
    run: async (argv, stdinFile) => {
      const line = argv.at(-1) ?? ''
      runs.push({ argv, line, stdinFile })
      const rule = rules.find(([prefix]) => line.startsWith(prefix))
      if (rule === undefined) return { code: 127, stdout: '', stderr: `sh: 1: ${line.split(' ')[0] ?? ''}: not found\n` }
      const answer = typeof rule[1] === 'function' ? rule[1](line, stdinFile) : rule[1]
      return { code: 0, stdout: '', stderr: '', ...answer }
    },
    platform: 'linux',
    arch: 'x64',
    version: '2.6.0',
    protocol: { major: 1, minor: 2 },
    ownBuild: async () => ({ path: '/opt/Helm/resources/floti', sha256: OWN_SHA }),
    sums: async () => new Map([['floti-2.6.0-linux-arm64', LINUX_SHA]]),
    script: async (os) => (os === 'windows' ? '# install.ps1\nInstall-Floti @args\n' : '# install.sh\nfloti_install "$@"\n'),
    releaseBase: (version) => `https://github.com/floti-dev/floti/releases/download/v${version}`,
    newId: () => 'offer-1',
    log: () => undefined,
    ...options
  })
  return { installer, runs, lines: () => runs.map((run) => run.line) }
}

const NO_FLOTI: [string, Answer] = ['floti ', { code: 127, stderr: 'sh: 1: floti: not found\n' }]
const INSTALLED: [string, Answer] = ['sh -c ', { stdout: '/home/me/.local/bin/floti\n' }]
const SET_UP: [string, Answer] = ['/home/me/.local/bin/floti setup', { stdout: 'The Floti host is running, and starts at each sign-in.\n' }]

describe('installing floti on a host’s computer, from the desktop', () => {
  it('names each release target as the release script does', () => {
    expect(FLOTI_TARGETS.map(({ name, os, cpu, platform, arch }) => ({ name, os, cpu, platform, arch }))).toEqual(
      RELEASE_TARGETS.map(({ name, os, cpu, platform, arch }) => ({ name, os, cpu, platform, arch }))
    )
  })

  it('S8: checks in batch mode with floti version, then names the OS and CPU, and offers an install saying what and where, changing nothing', async () => {
    const w = world([NO_FLOTI, [SYSTEM_LINE, { stdout: 'Linux x86_64\n' }]])
    const check = await w.installer.check({ transport: SSH })
    expect(w.runs.map((run) => run.argv)).toEqual([
      [...BATCH, 'floti version'],
      [...BATCH, SYSTEM_LINE],
      // Not on the login's PATH, and not where the install script puts it either.
      [...BATCH, standaloneProbeLine('linux')]
    ])
    expect(w.runs.every((run) => run.stdinFile === null)).toBe(true)
    expect(check).toEqual({
      kind: 'offer',
      id: 'offer-1',
      offer: {
        system: { os: 'linux', cpu: 'x64', label: 'Linux · x86_64' },
        found: null,
        action: 'install',
        how: 'copy',
        version: '2.6.0',
        folder: '~/.local/bin',
        sessions: null,
        talks: false
      }
    })
  })

  it('S8: through a command, with the line as its last argument and floti’s missing line from Windows’ cmd', async () => {
    const command: HostTransport = { kind: 'command', program: 'wsl.exe', args: ['-d', 'Ubuntu', '--'] }
    const w = world([
      [
        'floti ',
        { code: 1, stderr: "'floti' is not recognized as an internal or external command,\r\noperable program or batch file.\r\n" }
      ],
      [SYSTEM_LINE, { stdout: 'Windows_NT ARM64\r\n' }]
    ])
    const check = await w.installer.check({ transport: command })
    expect(w.runs.map((run) => run.argv)).toEqual([
      ['wsl.exe', '-d', 'Ubuntu', '--', 'floti version'],
      ['wsl.exe', '-d', 'Ubuntu', '--', SYSTEM_LINE],
      ['wsl.exe', '-d', 'Ubuntu', '--', standaloneProbeLine('windows')]
    ])
    expect(check).toMatchObject({ kind: 'noBuild' })
  })

  it('S9: a cancelled offer runs nothing more there, and cannot be installed after', async () => {
    const w = world([NO_FLOTI, [SYSTEM_LINE, { stdout: 'Linux x86_64\n' }]])
    const check = await w.installer.check({ transport: SSH })
    w.installer.cancel(check.kind === 'offer' ? check.id : '')
    await expect(w.installer.install('offer-1')).rejects.toThrow('no longer open')
    expect(w.lines()).toEqual(['floti version', SYSTEM_LINE, standaloneProbeLine('linux')])
  })

  it('S10: copies this Helm’s own floti over the same login, with its SHA-256 for the script to check there, then runs setup there', async () => {
    const w = world([SET_UP, NO_FLOTI, [SYSTEM_LINE, { stdout: 'Linux x86_64\n' }], INSTALLED])
    await w.installer.check({ transport: SSH })
    const outcome = await w.installer.install('offer-1')
    expect(outcome).toEqual({ ok: true, floti: '/home/me/.local/bin/floti', startsAtNextSignIn: false })
    const [put, setup] = w.runs.slice(3)
    expect(put?.argv.slice(0, 4)).toEqual(BATCH)
    expect(put?.stdinFile).toBe('/opt/Helm/resources/floti')
    expect(put?.line).toBe(
      `sh -c '# install.sh\nfloti_install "$@"\n' floti-install '2.6.0' 'floti-2.6.0-linux-x64' '${OWN_SHA}' '-'`
    )
    expect(setup?.argv).toEqual([...BATCH, '/home/me/.local/bin/floti setup'])
    expect(w.runs.length).toBe(5)
  })

  it('S11: a computer of another OS or CPU is sent the release’s address and SHA-256 to fetch from itself, and this PC runs nothing but the transport', async () => {
    const w = world([SET_UP, NO_FLOTI, [SYSTEM_LINE, { stdout: 'Linux aarch64\n' }], INSTALLED])
    const check = await w.installer.check({ transport: SSH })
    expect(check).toMatchObject({ kind: 'offer', offer: { how: 'download', system: { label: 'Linux · aarch64' } } })
    expect(await w.installer.install('offer-1')).toMatchObject({ ok: true })
    const put = w.runs[3]
    expect(put?.stdinFile).toBeNull()
    expect(put?.line).toContain(
      `'2.6.0' 'floti-2.6.0-linux-arm64' '${LINUX_SHA}' 'https://github.com/floti-dev/floti/releases/download/v2.6.0'`
    )
    // Every program this PC started was the user's ssh.
    expect(w.runs.every((run) => run.argv.slice(0, 4).join(' ') === BATCH.join(' '))).toBe(true)
  })

  it('S12: says there is no floti it can install for an OS no floti is built for, and keeps no offer', async () => {
    const w = world([NO_FLOTI, [SYSTEM_LINE, { stdout: 'FreeBSD arm64\n' }]])
    expect(await w.installer.check({ transport: SSH })).toEqual({
      kind: 'noBuild',
      system: { os: null, cpu: 'arm64', label: 'FreeBSD · arm64' },
      reason: 'There is no floti for FreeBSD · arm64 yet.'
    })
    await expect(w.installer.install('offer-1')).rejects.toThrow()
  })

  it('S12: a build with no published floti offers nothing for a computer its own floti does not fit', async () => {
    const w = world([NO_FLOTI, [SYSTEM_LINE, { stdout: 'Darwin arm64\n' }]], { sums: async () => null })
    const check = await w.installer.check({ transport: SSH })
    expect(check).toMatchObject({ kind: 'noBuild', reason: expect.stringContaining('no published floti') })
  })

  it('a floti where the install script puts it, off the login’s PATH, is ready and kept by its full path', async () => {
    const probe = standaloneProbeLine('macos')
    const w = world([
      NO_FLOTI,
      [SYSTEM_LINE, { stdout: 'Darwin arm64\n' }],
      [probe, { stdout: '/Users/me/.local/bin/floti\nfloti 2.6.0 protocol 1.2 macos arm64\n' }]
    ])
    expect(await w.installer.check({ transport: SSH })).toEqual({
      kind: 'ready',
      found: { version: '2.6.0', protocol: '1.2' },
      system: { os: 'macos', cpu: 'arm64', label: 'macOS · arm64' },
      floti: '/Users/me/.local/bin/floti'
    })
    expect(w.lines()).toEqual(['floti version', SYSTEM_LINE, probe])

    // One too old there is offered an update at that path.
    const old = world([
      NO_FLOTI,
      [SYSTEM_LINE, { stdout: 'Linux aarch64\n' }],
      [standaloneProbeLine('linux'), { stdout: '/home/me/.local/bin/floti\nfloti 2.1.0 protocol 1.0 linux arm64\n' }],
      ['/home/me/.local/bin/floti update-stop --ask', { stdout: '0\n' }]
    ])
    expect(await old.installer.check({ transport: SSH })).toMatchObject({
      kind: 'offer',
      offer: { action: 'update', found: { version: '2.1.0' }, sessions: 0 }
    })

    // A host that keeps its floti's path is asked by that path alone.
    const kept = world([['/opt/floti version', { code: 127, stderr: 'sh: 1: /opt/floti: not found\n' }], [SYSTEM_LINE, { stdout: 'Darwin arm64\n' }]])
    await kept.installer.check({ transport: SSH, floti: '/opt/floti' })
    expect(kept.lines()).not.toContain(probe)
  })

  it('a floti this Helm can talk to is ready: nothing is offered', async () => {
    const w = world([['floti version', { stdout: 'floti 2.5.0 protocol 1.2 linux x64\n' }]])
    expect(await w.installer.check({ transport: SSH })).toEqual({
      kind: 'ready',
      found: { version: '2.5.0', protocol: '1.2' },
      system: { os: 'linux', cpu: 'x64', label: 'Linux · x64' }
    })
    expect(w.lines()).toEqual(['floti version'])
  })

  it('S13: a floti too old to talk to is offered an update naming the version found, with how many sessions it ends', async () => {
    const w = world([
      ['floti version', { stdout: 'floti 2.1.0 protocol 1.0 linux x64\n' }],
      ['floti update-stop --ask', { stdout: '2\n' }]
    ])
    const check = await w.installer.check({ transport: SSH })
    expect(check).toMatchObject({
      kind: 'offer',
      offer: {
        action: 'update',
        found: { version: '2.1.0', protocol: '1.0' },
        system: { os: 'linux', cpu: 'x64' },
        how: 'copy',
        sessions: 2,
        talks: false
      }
    })
    expect(w.lines()).toEqual(['floti version', 'floti update-stop --ask'])
  })

  it('S13: a floti from before floti version is too old, its version from --version, its sessions unknown', async () => {
    const w = world([
      ['floti version', { code: 2, stderr: 'usage: floti host [--desktop <program>]\n' }],
      [SYSTEM_LINE, { stdout: 'Linux x86_64\n' }],
      ['floti --version', { stdout: 'floti 0.1.0\n' }],
      ['floti update-stop --ask', { code: 2, stderr: 'usage: floti host\n' }]
    ])
    expect(await w.installer.check({ transport: SSH })).toMatchObject({
      kind: 'offer',
      offer: { action: 'update', found: { version: '0.1.0', protocol: null }, sessions: null }
    })
  })

  it('S19: a newer floti says to update this Helm and offers nothing', async () => {
    const w = world([['floti version', { stdout: 'floti 3.0.0 protocol 2.0 linux x64\n' }]])
    expect(await w.installer.check({ transport: SSH })).toEqual({
      kind: 'newer',
      found: { version: '3.0.0', protocol: '2.0' },
      ours: '1.2',
      helm: '2.6.0'
    })
  })

  it('S14: an update stops the old host for the update before anything is replaced, by the path the host keeps', async () => {
    const floti = '/home/me/.local/bin/floti'
    const w = world([
      [`${floti} version`, { stdout: 'floti 2.1.0 protocol 1.0 linux x64\n' }],
      [`${floti} update-stop --ask`, { stdout: '3\n' }],
      [`${floti} update-stop --stop`, {}],
      SET_UP,
      INSTALLED
    ])
    const check = await w.installer.check({ transport: SSH, floti })
    expect(check).toMatchObject({ offer: { sessions: 3 } })
    // Nothing is stopped or copied before the yes.
    expect(w.lines()).toEqual([`${floti} version`, `${floti} update-stop --ask`])
    expect(await w.installer.install('offer-1')).toMatchObject({ ok: true, floti })
    expect(w.lines().slice(2).map((line) => line.split(' ').slice(0, 2).join(' '))).toEqual([
      `${floti} update-stop`,
      "sh -c",
      `${floti} setup`
    ])
  })

  it('S14: without a yes, an update stops nothing and copies nothing', async () => {
    const w = world([
      ['floti version', { stdout: 'floti 2.1.0 protocol 1.0 linux x64\n' }],
      ['floti update-stop --ask', { stdout: '1\n' }]
    ])
    await w.installer.check({ transport: SSH })
    w.installer.cancel('offer-1')
    expect(w.lines()).not.toContain('floti update-stop --stop')
    expect(w.runs.some((run) => run.stdinFile !== null)).toBe(false)
  })

  it('S14: a stop that fails replaces nothing, and says the last line written there', async () => {
    const w = world([
      ['floti version', { stdout: 'floti 2.1.0 protocol 1.0 linux x64\n' }],
      ['floti update-stop --ask', { stdout: '1\n' }],
      ['floti update-stop --stop', { code: 1, stderr: 'floti update-stop: the host did not stop within 60 s\n' }]
    ])
    await w.installer.check({ transport: SSH })
    expect(await w.installer.install('offer-1')).toEqual({
      ok: false,
      step: 'stop',
      lastLine: 'floti update-stop: the host did not stop within 60 s'
    })
    expect(w.lines().some((line) => line.startsWith('sh -c'))).toBe(false)
  })

  it('S15: an install that fails there ends with its last line, unedited, and runs no setup', async () => {
    const w = world([
      NO_FLOTI,
      [SYSTEM_LINE, { stdout: 'Linux x86_64\n' }],
      ['sh -c ', { code: 1, stderr: 'floti install: floti-2.6.0-linux-x64 did not match its SHA-256, so it was not installed.\n' }]
    ])
    await w.installer.check({ transport: SSH })
    expect(await w.installer.install('offer-1')).toEqual({
      ok: false,
      step: 'install',
      lastLine: 'floti install: floti-2.6.0-linux-x64 did not match its SHA-256, so it was not installed.'
    })
    expect(w.lines().some((line) => line.endsWith(' setup'))).toBe(false)
  })

  it('S15: a setup that fails there says its last line', async () => {
    const w = world([
      ['/home/me/.local/bin/floti setup', { code: 1, stderr: 'floti setup: schtasks: Access is denied.\n' }],
      NO_FLOTI,
      [SYSTEM_LINE, { stdout: 'Linux x86_64\n' }],
      INSTALLED
    ])
    await w.installer.check({ transport: SSH })
    expect(await w.installer.install('offer-1')).toEqual({
      ok: false,
      step: 'setup',
      lastLine: 'floti setup: schtasks: Access is denied.'
    })
  })

  it('S17: a setup that registered the host with nobody signed in there is an install that worked, starting at the next sign-in', async () => {
    const w = world(
      [
        [
          '"C:\\Users\\me\\AppData\\Local\\Programs\\Floti\\bin\\floti.exe" setup',
          { code: 10, stdout: 'The Floti host starts at the next sign-in.\r\n' }
        ],
        ['floti ', { code: 1, stderr: "'floti' is not recognized as an internal or external command,\r\n" }],
        [SYSTEM_LINE, { stdout: 'Windows_NT AMD64\r\n' }],
        ['powershell ', { stdout: 'C:\\Users\\me\\AppData\\Local\\Programs\\Floti\\bin\\floti.exe\r\n' }]
      ],
      { platform: 'win32', arch: 'x64' }
    )
    expect(await w.installer.check({ transport: SSH })).toMatchObject({
      offer: { how: 'copy', folder: '%LOCALAPPDATA%\\Programs\\Floti\\bin' }
    })
    expect(await w.installer.install('offer-1')).toEqual({
      ok: true,
      floti: 'C:\\Users\\me\\AppData\\Local\\Programs\\Floti\\bin\\floti.exe',
      startsAtNextSignIn: true
    })
  })

  it('a check whose transport fails says ssh’s own last line', async () => {
    const w = world([['floti ', { code: 255, stderr: 'me@build-box: Permission denied (publickey,password).\n' }]])
    expect(await w.installer.check({ transport: SSH })).toEqual({
      kind: 'failed',
      lastLine: 'me@build-box: Permission denied (publickey,password).',
      said: 'me@build-box: Permission denied (publickey,password).'
    })
    expect(w.lines()).toEqual(['floti version'])
  })

  it('S15: a login that prints text before floti version’s line fails the check, with the first line it printed', async () => {
    const w = world([['floti version', { stdout: 'Welcome to build-box!\nfloti 2.5.0 protocol 1.2 linux x64\n' }]])
    expect(await w.installer.check({ transport: SSH })).toEqual({
      kind: 'failed',
      lastLine: 'Welcome to build-box!',
      cause: 'loginOutput'
    })
  })

  it('S10: a transport whose program is not on this PC fails the check as missing, and runs nothing more', async () => {
    const w = world([['floti version', { code: null, error: 'Helm could not run ssh on this PC (ENOENT).', notFound: true }]])
    expect(await w.installer.check({ transport: SSH })).toEqual({
      kind: 'failed',
      lastLine: 'Helm could not run ssh on this PC (ENOENT).',
      cause: 'transportMissing'
    })
    expect(w.lines()).toEqual(['floti version'])
  })

  it('S8: the check is given the add’s signal, so Cancel stops the run under way', async () => {
    const signals: (AbortSignal | undefined)[] = []
    const w = world([], {
      run: async (_argv, _stdin, signal) => {
        signals.push(signal)
        return { code: null, stdout: '', stderr: '' }
      }
    })
    const controller = new AbortController()
    await w.installer.check({ transport: SSH }, controller.signal)
    expect(signals[0]).toBe(controller.signal)
  })
})

describe('the last line written there', () => {
  it('passes over the CLIXML PowerShell writes to a redirected standard error', () => {
    const stderr =
      'floti install: floti.exe did not match its SHA-256, so it was not installed.\r\n#< CLIXML\r\n<Objs Version="1.1.0.1"><Obj S="progress" RefId="0"></Obj></Objs>'
    expect(lastLineOf({ code: 1, stdout: '', stderr })).toBe(
      'floti install: floti.exe did not match its SHA-256, so it was not installed.'
    )
  })
})

describe('the install line', () => {
  it('quotes the script and its arguments for a POSIX shell, so sh -c gets them as they are', () => {
    expect(installLine('linux', "echo 'it'\n", ['a b', "it's"])).toBe(
      `sh -c 'echo '\\''it'\\''\n' floti-install 'a b' 'it'\\''s'`
    )
  })

  it('sends PowerShell the script with its comments left out, encoded, inside cmd’s limit', async () => {
    const { readFile } = await import('node:fs/promises')
    const { resolve } = await import('node:path')
    const script = await readFile(resolve(import.meta.dirname, '../../../../../scripts/install/install.ps1'), 'utf8')
    const line = installLine('windows', script, ['2.6.0', 'floti-2.6.0-windows-x64.exe', 'f'.repeat(64), '-'])
    expect(line.startsWith('powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand ')).toBe(true)
    expect(line.length).toBeLessThan(8000)
    const decoded = Buffer.from(line.split(' ').at(-1) ?? '', 'base64').toString('utf16le')
    expect(decoded).not.toContain('# ')
    expect(decoded.trimEnd().endsWith(`} '2.6.0' 'floti-2.6.0-windows-x64.exe' '${'f'.repeat(64)}' '-'`)).toBe(true)
  })
})
describe('installing floti in a WSL distro', () => {
  /** The line a distro's shell runs, from what `wslLine` made of it. */
  const unwrapped = (line: string): string => {
    const wrapped = /^eval "\$\(echo ([A-Za-z0-9+/=]+) \| base64 -d\)"$/.exec(line)
    return wrapped?.[1] === undefined ? line : Buffer.from(wrapped[1], 'base64').toString('utf8')
  }

  function distroWorld(transport: HostTransport) {
    const runs: {
      argv: readonly string[]
      line: string
      how: RunHow | undefined
      stdinFile: string | null
    }[] = []
    const installer = createRemoteInstaller({
      restartState: () => Promise.reject(new Error('Nothing asks what a restart ends here.')),
      keeperLink: 1,
      hostVersionAfter: () => Promise.reject(new Error('Nothing waits for a host to connect again here.')),
      run: async (argv, stdinFile, _signal, how) => {
        const line = unwrapped(argv.at(-1) ?? '')
        runs.push({ argv, line, how, stdinFile })
        if (line.startsWith('floti ')) return { code: 127, stdout: '', stderr: 'sh: 1: floti: not found\n' }
        if (line === SYSTEM_LINE) return { code: 0, stdout: 'Linux x86_64\n', stderr: '' }
        if (line.startsWith('sh -c ')) return { code: 0, stdout: '/home/me/.local/bin/floti\n', stderr: '' }
        return {
          code: 0,
          stdout: 'The Floti host starts when a client connects.\n',
          stderr: ''
        }
      },
      programs: { wsl: ['C:\\Windows\\System32\\wsl.exe'] },
      platform: 'win32',
      arch: 'x64',
      version: '2.6.0',
      protocol: { major: 1, minor: 2 },
      ownBuild: async () => null,
      sums: async () => new Map([['floti-2.6.0-linux-x64', LINUX_SHA]]),
      script: async () => '# install.sh\nfloti_install "$@"\n',
      releaseBase: (version) => `https://github.com/floti-dev/floti/releases/download/v${version}`,
      newId: () => 'offer-1',
      log: () => undefined
    })
    return { installer, runs, transport }
  }

  it('S3: this PC’s distro is checked, offered the Linux build and installed through wsl.exe -d Debian --, each line run as written and reaching the shell unaltered', async () => {
    const w = distroWorld({ kind: 'wsl', distro: 'Debian' })
    const check = await w.installer.check({ transport: w.transport })
    expect(check).toMatchObject({
      kind: 'offer',
      offer: { how: 'download', system: { os: 'linux', cpu: 'x64' } }
    })
    expect(await w.installer.install('offer-1')).toMatchObject({
      ok: true,
      floti: '/home/me/.local/bin/floti'
    })
    expect(w.runs.map((run) => run.argv.slice(0, 4))).toEqual(
      Array.from({ length: 5 }, () => ['C:\\Windows\\System32\\wsl.exe', '-d', 'Debian', '--'])
    )
    expect(w.runs.every((run) => run.how?.asWritten === true)).toBe(true)
    expect(w.runs.map((run) => run.line)).toEqual([
      'floti version',
      SYSTEM_LINE,
      standaloneProbeLine('linux'),
      expect.stringMatching(/^sh -c /),
      '/home/me/.local/bin/floti setup'
    ])
    // The system line holds characters cmd reads, so it went encoded.
    expect(w.runs[1]?.argv.at(-1)).toBe(wslLine(SYSTEM_LINE))
    expect(w.runs[3]?.line).toContain(`'floti-2.6.0-linux-x64' '${LINUX_SHA}'`)
  })

  it('S5: a distro on win-box is installed through ssh -o BatchMode=yes win-box wsl.exe -d Ubuntu --, the line as one argument', async () => {
    const w = distroWorld({
      kind: 'wsl',
      distro: 'Ubuntu',
      through: 'win-box'
    })
    await w.installer.check({ transport: w.transport })
    expect(await w.installer.install('offer-1')).toMatchObject({ ok: true })
    expect(w.runs).toHaveLength(5)
    expect(
      w.runs.every((run) => run.argv.slice(0, -1).join(' ') === 'ssh -o BatchMode=yes win-box wsl.exe -d Ubuntu --')
    ).toBe(true)
    expect(w.runs.every((run) => run.how === undefined)).toBe(true)
  })

  it('wraps only a line holding characters cmd reads', () => {
    expect(wslLine('floti connect')).toBe('floti connect')
    expect(wslLine('a | b')).toMatch(/^eval "\$\(echo [A-Za-z0-9+/=]+ \| base64 -d\)"$/)
    expect(unwrapped(wslLine('echo %OS% & x > y'))).toBe('echo %OS% & x > y')
  })

  it('S10: reads what wsl.exe wrote as UTF-16LE, with or without a byte-order mark, and UTF-8 as it is', () => {
    const text = 'There is no distribution with the supplied name.\r\n'
    expect(decodeOutput(Buffer.from(text, 'utf16le'))).toBe(text)
    expect(decodeOutput(Buffer.concat([Buffer.from([0xff, 0xfe]), Buffer.from(text, 'utf16le')]))).toBe(text)
    expect(decodeOutput(Buffer.from('Linux x86_64 – ünïcode\n', 'utf8'))).toBe('Linux x86_64 – ünïcode\n')
    expect(decodeOutput(new Uint8Array())).toBe('')
  })

  it('quotes a word as a Windows program’s runtime reads it back', () => {
    expect(windowsArgument('-d')).toBe('-d')
    expect(windowsArgument('')).toBe('""')
    expect(windowsArgument('C:\\Program Files\\nodejs\\node.exe')).toBe('"C:\\Program Files\\nodejs\\node.exe"')
    expect(windowsArgument('say "hi"')).toBe('"say \\"hi\\""')
    expect(windowsArgument('C:\\a b\\')).toBe('"C:\\a b\\\\"')
  })

  it('runs this PC’s wsl.exe as written on Windows: WSL_UTF8=1, the words before the line quoted, the line appended as it is', async () => {
    const calls: {
      command: string
      args: string[]
      options: TransportSpawnOptions
    }[] = []
    const spawn = (command: string, args: string[], options: TransportSpawnOptions): ChildProcess => {
      calls.push({ command, args, options })
      const child = Object.assign(new EventEmitter(), {
        stdout: new EventEmitter(),
        stderr: new EventEmitter(),
        stdin: null,
        kill: () => true
      })
      queueMicrotask(() => {
        child.stdout.emit('data', Buffer.from('Debian\r\n', 'utf16le'))
        child.stdout.emit('end')
        child.stderr.emit('end')
        child.emit('exit', 0, null)
        child.emit('close', 0, null)
      })
      return child as unknown as ChildProcess
    }
    const run = spawnTransportRun(spawn, { PATH: 'x' }, 'win32')
    const said = await run(['C:\\Program Files\\wsl.exe', '-d', 'My Distro', '--', 'floti connect'], null, undefined, {
      asWritten: true
    })
    expect(said.stdout).toBe('Debian\r\n')
    expect(calls[0]?.options).toMatchObject({
      windowsVerbatimArguments: true,
      env: { PATH: 'x', WSL_UTF8: '1' }
    })
    expect(calls[0]?.args).toEqual(['-d', '"My Distro"', '--', 'floti connect'])
    expect(calls[0]?.options.argv0).toBe('"C:\\Program Files\\wsl.exe"')
  })

  it('tells a run’s progress the last line it has written each time it changes, a line split across chunks once whole', async () => {
    const spawn = (): ChildProcess => {
      const child = Object.assign(new EventEmitter(), {
        stdout: new EventEmitter(),
        stderr: new EventEmitter(),
        stdin: null,
        kill: () => true
      })
      queueMicrotask(() => {
        child.stdout.emit('data', Buffer.from('Downloading floti-2.6.0-linux-x64.\n'))
        child.stdout.emit('data', Buffer.from('\n'))
        child.stdout.emit('data', Buffer.from('floti is updated'))
        child.stdout.emit('data', Buffer.from(' from 2.5.0 to 2.6.0.\n'))
        child.emit('close', 0, null)
      })
      return child as unknown as ChildProcess
    }
    const said: string[] = []
    const run = spawnTransportRun(spawn, {}, 'linux')
    await run(['ssh', 'me@build-box', 'floti update --floti-only'], null, undefined, { progress: (line) => said.push(line) })
    expect(said).toEqual([
      'Downloading floti-2.6.0-linux-x64.',
      'floti is updated from 2.5.0 to 2.6.0.'
    ])
  })
})

/** Timers a test steps by hand: nothing waits on the clock. */
function steppedTimers() {
  const pending = new Map<number, { run: () => void; ms: number }>()
  let next = 1
  return {
    setTimeout: (run: () => void, ms: number): unknown => {
      const id = next++
      pending.set(id, { run, ms })
      return id
    },
    clearTimeout: (timer: unknown): void => {
      pending.delete(timer as number)
    },
    /** Whether a timer of `ms` is waiting. */
    waiting: (ms: number): boolean => [...pending.values()].some((timer) => timer.ms === ms),
    /** Runs every waiting timer of `ms`, as if that long had passed. */
    pass(ms: number): void {
      for (const [id, timer] of [...pending]) {
        if (timer.ms !== ms) continue
        pending.delete(id)
        timer.run()
      }
    }
  }
}

const PATIENCE = 600_000
const GRACE = 15_000

/**
 * An added host's Update floti, with its connection, its login and the clock
 * faked: a floti older than this Helm's is offered an update even while this
 * Helm can talk to it, and one with `floti update` runs that, over the
 * host's connection when its channel has a newer floti and over its login
 * otherwise.
 */
describe('updating an added host’s floti from the desktop', () => {
  const PROTOCOL_3_1 = { major: 3, minor: 1 }
  const HOST = { id: 'h-build', transport: SSH }
  const IDLE: P.UpdateState = {
    running: '2.5.0',
    channel: 'stable',
    unavailable: null,
    newest: '2.6.0',
    checkedAt: '2026-11-11T08:00:00Z',
    failed: null,
    checking: false,
    installing: null,
    installFailed: null
  }
  const step = (line: string): P.UpdateState => ({ ...IDLE, installing: { line, waitingForApp: false } })

  /** A host on a connection that answers `update:state` with `state` the moment it is subscribed to. */
  class UpdatingHost extends FakeConnection {
    constructor(private readonly state: P.UpdateState) {
      super()
    }

    override subscribe<C extends keyof EventChannels>(channel: C, handlers: SubscriptionHandlers<C>): HostSubscription {
      const subscription = super.subscribe(channel, handlers)
      if (channel === 'update:state') this.snapshot('update:state', this.state)
      return subscription
    }
  }

  /** What the host's restart frame answers: its sessions and its keeper's link. */
  const restart = (sessions: number, keeperLink: number | null) => async (): Promise<P.RestartState> => ({
    hostVersion: '2.5.0',
    protocol: PROTOCOL_3_1,
    sessions,
    keeperLink
  })

  const OLDER = ['floti version', { stdout: 'floti 2.5.0 protocol 3.1 linux x64\n' }] as [string, Answer]

  it('offers a floti older than this Helm’s, which it can talk to, floti update over its login, for floti alone, saying it cannot tell the sessions', async () => {
    const w = world([OLDER], { protocol: PROTOCOL_3_1, restartState: restart(3, 1) })
    const check = await w.installer.checkUpdate(HOST, null)
    expect(check).toEqual({
      kind: 'offer',
      id: 'offer-1',
      offer: {
        system: { os: 'linux', cpu: 'x64', label: 'Linux · x64' },
        found: { version: '2.5.0', protocol: '3.1' },
        action: 'update',
        how: 'flotiUpdate',
        via: 'login',
        updatesTo: null,
        version: '2.6.0',
        folder: '~/.local/bin',
        // Which floti the login's update installs is not known beforehand.
        sessions: null,
        talks: true
      }
    })
    // Nothing is stopped or counted for an install: only floti version ran.
    expect(w.lines()).toEqual(['floti version'])
  })

  it('adding the same host finds it ready: only an added host’s Update floti offers an update to a floti it can talk to', async () => {
    const w = world([OLDER], { protocol: PROTOCOL_3_1 })
    expect(await w.installer.check({ transport: SSH })).toMatchObject({ kind: 'ready' })
  })

  it('a floti as new as this Helm’s is ready, and nothing is offered', async () => {
    const w = world([['floti version', { stdout: 'floti 2.6.0 protocol 3.1 linux x64\n' }]], { protocol: PROTOCOL_3_1 })
    expect(await w.installer.checkUpdate(HOST, null)).toMatchObject({ kind: 'ready' })
  })

  it('runs floti update --floti-only over the same login by the path the host keeps, showing its output as it goes', async () => {
    const said: string[] = []
    const runs: { line: string; how: RunHow | undefined }[] = []
    const w = world([], {
      protocol: PROTOCOL_3_1,
      run: async (argv, _stdin, _signal, how) => {
        const line = argv.at(-1) ?? ''
        runs.push({ line, how })
        if (line.endsWith(' version')) return { code: 0, stdout: 'floti 2.5.0 protocol 3.1 linux x64\n', stderr: '' }
        how?.progress?.('Downloading floti-2.6.0-linux-x64.')
        how?.progress?.('floti is updated from 2.5.0 to 2.6.0.')
        return { code: 0, stdout: 'Downloading floti-2.6.0-linux-x64.\nfloti is updated from 2.5.0 to 2.6.0.\n', stderr: '' }
      }
    })
    await w.installer.checkUpdate({ ...HOST, floti: '/home/me/.local/bin/floti' }, null)
    const outcome = await w.installer.install('offer-1', (line) => said.push(line))
    expect(outcome).toEqual({ ok: true, floti: '/home/me/.local/bin/floti', startsAtNextSignIn: false })
    expect(runs.map((run) => run.line)).toEqual([
      '/home/me/.local/bin/floti version',
      '/home/me/.local/bin/floti update --floti-only'
    ])
    expect(runs[1]?.how?.progress).toBeDefined()
    expect(said).toEqual(['Downloading floti-2.6.0-linux-x64.', 'floti is updated from 2.5.0 to 2.6.0.'])
  })

  it('a floti already up to date on its channel is its own outcome, not a failure', async () => {
    const w = world(
      [OLDER, ['floti update --floti-only', { code: 4, stdout: 'floti 2.5.0 is up to date on stable (its newest is 2.5.0).\n' }]],
      { protocol: PROTOCOL_3_1 }
    )
    await w.installer.checkUpdate(HOST, null)
    expect(await w.installer.install('offer-1')).toEqual({
      ok: null,
      ended: 'upToDate',
      lastLine: 'floti 2.5.0 is up to date on stable (its newest is 2.5.0).'
    })
  })

  it('after an update that found nothing newer, an install is offered over the floti there, saying the sessions it ends', async () => {
    const w = world([OLDER, ['floti update-stop --ask', { stdout: '2\n' }]], { protocol: PROTOCOL_3_1 })
    expect(await w.installer.checkUpdate(HOST, null, { install: true })).toMatchObject({
      offer: { how: 'copy', action: 'update', sessions: 2, talks: true }
    })
  })

  it('a floti update that fails there says its last line, at the update step', async () => {
    const w = world(
      [OLDER, ['floti update', { code: 1, stderr: 'floti update: floti-2.6.0-linux-x64 does not match its signature.\n' }]],
      { protocol: PROTOCOL_3_1 }
    )
    await w.installer.checkUpdate(HOST, null)
    expect(await w.installer.install('offer-1')).toEqual({
      ok: false,
      step: 'update',
      lastLine: 'floti update: floti-2.6.0-linux-x64 does not match its signature.'
    })
  })

  it('stops waiting for a floti update over the login at Stop waiting, or once the patience runs out, ending the run on this PC', async () => {
    for (const how of ['cancel', 'patience'] as const) {
      const timers = steppedTimers()
      const stopped: AbortSignal[] = []
      const w = world([], {
        protocol: PROTOCOL_3_1,
        patienceMs: PATIENCE,
        setTimeout: timers.setTimeout,
        clearTimeout: timers.clearTimeout,
        run: (argv, _stdin, signal) => {
          if ((argv.at(-1) ?? '').endsWith(' version')) {
            return Promise.resolve({ code: 0, stdout: 'floti 2.5.0 protocol 3.1 linux x64\n', stderr: '' })
          }
          // A run that never ends by itself, as one stuck on its network.
          return new Promise((resolve) => {
            signal?.addEventListener('abort', () => {
              stopped.push(signal)
              resolve({ code: null, stdout: 'Downloading floti-2.6.0-linux-x64.\n', stderr: '' })
            })
          })
        }
      })
      await w.installer.checkUpdate(HOST, null)
      const outcome = w.installer.install('offer-1')
      expect(timers.waiting(PATIENCE)).toBe(true)
      if (how === 'cancel') w.installer.cancel('offer-1')
      else timers.pass(PATIENCE)
      await expect(outcome).resolves.toEqual({
        ok: null,
        ended: 'stoppedWaiting',
        lastLine: 'Downloading floti-2.6.0-linux-x64.'
      })
      expect(stopped).toHaveLength(1)
      expect(timers.waiting(PATIENCE)).toBe(false)
    }
  })

  it('a floti too old to talk to that has floti update runs it over its login, where adding it would install', async () => {
    const w = world([OLDER, ['floti update-stop --ask', { stdout: '2\n' }]], { protocol: { major: 3, minor: 2 } })
    expect(await w.installer.checkUpdate(HOST, null)).toMatchObject({
      offer: { how: 'flotiUpdate', via: 'login', sessions: null, talks: false }
    })
    expect(await w.installer.check({ transport: SSH })).toMatchObject({
      offer: { how: 'copy', action: 'update', sessions: 2, talks: false }
    })
  })

  it('a floti older than this Helm’s without floti update is updated by an install, saying the sessions it ends', async () => {
    const w = world(
      [
        ['floti version', { stdout: 'floti 2.5.0 protocol 3.0 linux x64\n' }],
        ['floti update-stop --ask', { stdout: '2\n' }]
      ],
      { protocol: { major: 3, minor: 0 } }
    )
    expect(await w.installer.checkUpdate(HOST, null)).toMatchObject({
      offer: { how: 'copy', action: 'update', sessions: 2, talks: true }
    })
    expect(w.lines()).toEqual(['floti version', 'floti update-stop --ask'])
  })

  it('a connected host whose channel has this Helm’s version is asked over its connection, naming that version and the sessions its keeper hands over', async () => {
    const host = new UpdatingHost(IDLE)
    const w = world([OLDER], { protocol: PROTOCOL_3_1, restartState: restart(4, 1) })
    expect(await w.installer.checkUpdate(HOST, host.asConnection())).toMatchObject({
      offer: { how: 'flotiUpdate', via: 'connection', updatesTo: '2.6.0', sessions: 0, talks: true }
    })
    // Only floti version ran there, and only the state was heard.
    expect(w.lines()).toEqual(['floti version'])
    expect(host.subscriptions.has('update:state')).toBe(false)
    expect(host.payloads('update:install')).toEqual([])

    const ends = world([OLDER], { protocol: PROTOCOL_3_1, restartState: restart(4, 2) })
    expect(await ends.installer.checkUpdate(HOST, new UpdatingHost(IDLE).asConnection())).toMatchObject({
      offer: { updatesTo: '2.6.0', sessions: 4 }
    })
  })

  it('S13: an update to any other version than this Helm’s says it cannot tell the sessions, and asks nothing', async () => {
    let asked = 0
    const w = world([OLDER], {
      protocol: PROTOCOL_3_1,
      restartState: async () => {
        asked += 1
        return restart(4, 1)()
      }
    })
    expect(await w.installer.checkUpdate(HOST, new UpdatingHost({ ...IDLE, newest: '2.7.0' }).asConnection())).toMatchObject({
      offer: { how: 'flotiUpdate', updatesTo: '2.7.0', sessions: null }
    })
    expect(asked).toBe(0)
  })

  it('a connected host whose channel has nothing newer, or that does not keep floti up to date, is updated by an install', async () => {
    for (const state of [
      { ...IDLE, newest: '2.5.0' },
      { ...IDLE, newest: null },
      { ...IDLE, unavailable: 'notStandalone' as const }
    ]) {
      const w = world([OLDER, ['floti update-stop --ask', { stdout: '1\n' }]], { protocol: PROTOCOL_3_1 })
      expect(await w.installer.checkUpdate(HOST, new UpdatingHost(state).asConnection())).toMatchObject({
        offer: { how: 'copy', action: 'update', sessions: 1, talks: true }
      })
    }
  })

  /** A connected host's update under way: its offer taken, `update:install` answered. */
  async function updatingOverConnection(hostVersionAfter: RemoteInstallDeps['hostVersionAfter']) {
    const host = new UpdatingHost(IDLE)
    const timers = steppedTimers()
    const said: string[] = []
    const w = world([OLDER], {
      protocol: PROTOCOL_3_1,
      restartState: restart(4, 1),
      hostVersionAfter,
      patienceMs: PATIENCE,
      graceMs: GRACE,
      setTimeout: timers.setTimeout,
      clearTimeout: timers.clearTimeout
    })
    await w.installer.checkUpdate(HOST, host.asConnection())
    const asked = new Promise<void>((resolve) =>
      host.answer('update:install', () => {
        resolve()
        return null
      })
    )
    const outcome = w.installer.install('offer-1', (line) => said.push(line))
    await asked
    return { host, timers, said, outcome, installer: w.installer }
  }

  it('follows the update over the connection, showing each step, and it went well once the host connects again on a newer floti', async () => {
    const after: string[] = []
    const u = await updatingOverConnection(async (id) => {
      after.push(id)
      return '2.6.0'
    })
    expect(u.host.payloads('update:install')).toEqual([null])
    u.host.push('update:state', step('Downloading floti-2.6.0-linux-x64.'))
    u.host.push('update:state', step('floti is updated from 2.5.0 to 2.6.0.'))
    u.host.close()
    await expect(u.outcome).resolves.toEqual({ ok: true, startsAtNextSignIn: false })
    expect(after).toEqual(['h-build'])
    expect(u.said).toEqual([
      'Downloading floti-2.6.0-linux-x64.',
      'floti is updated from 2.5.0 to 2.6.0.',
      'The host there is starting the new floti; waiting for it to connect again.'
    ])
    expect(u.host.subscriptions.has('update:state')).toBe(false)
    expect(u.timers.waiting(PATIENCE)).toBe(false)
  })

  it('an update that ends its run before the connection ends, as the old host does while it stops, still waits for the connection', async () => {
    const u = await updatingOverConnection(async () => '2.6.0')
    u.host.push('update:state', step('floti is updated from 2.5.0 to 2.6.0.'))
    u.host.push('update:state', IDLE)
    expect(u.timers.waiting(GRACE)).toBe(true)
    u.host.close()
    await expect(u.outcome).resolves.toEqual({ ok: true, startsAtNextSignIn: false })
    expect(u.timers.waiting(GRACE)).toBe(false)
  })

  it('an update that ends its run and keeps the connection past the grace is unconfirmed, with its last line', async () => {
    const u = await updatingOverConnection(async () => '2.6.0')
    u.host.push('update:state', step('floti update could not ask the host to start floti 2.6.0.'))
    u.host.push('update:state', IDLE)
    u.timers.pass(GRACE)
    await expect(u.outcome).resolves.toEqual({
      ok: null,
      ended: 'unconfirmed',
      lastLine: 'floti update could not ask the host to start floti 2.6.0.'
    })
  })

  it('a host that connects again on the same floti, as after a dropped link, is unconfirmed, naming what it runs', async () => {
    const u = await updatingOverConnection(async () => '2.5.0')
    u.host.push('update:state', step('Downloading floti-2.6.0-linux-x64.'))
    u.host.close()
    await expect(u.outcome).resolves.toEqual({
      ok: null,
      ended: 'unconfirmed',
      lastLine: 'The host there connected again on floti 2.5.0.'
    })
  })

  it('a host that has not connected again when the patience runs out, or at Stop waiting, is stopped waiting', async () => {
    for (const how of ['cancel', 'patience'] as const) {
      const u = await updatingOverConnection(
        (_id, signal) => new Promise((resolve) => signal.addEventListener('abort', () => resolve(null)))
      )
      u.host.push('update:state', step('floti is updated from 2.5.0 to 2.6.0.'))
      u.host.close()
      await vi.waitFor(() => expect(u.said.at(-1)).toContain('waiting for it to connect again'))
      if (how === 'cancel') u.installer.cancel('offer-1')
      else u.timers.pass(PATIENCE)
      await expect(u.outcome).resolves.toEqual({
        ok: null,
        ended: 'stoppedWaiting',
        lastLine: 'floti is updated from 2.5.0 to 2.6.0.'
      })
    }
  })

  it('a failed update says the line it failed with', async () => {
    const u = await updatingOverConnection(async () => '2.6.0')
    u.host.push('update:state', step('Starting floti update.'))
    u.host.push('update:state', { ...IDLE, installFailed: 'floti-2.6.0-linux-x64 does not match its SHA-256 in update.json.' })
    await expect(u.outcome).resolves.toEqual({
      ok: false,
      step: 'update',
      lastLine: 'floti-2.6.0-linux-x64 does not match its SHA-256 in update.json.'
    })
  })
})

describe('following a floti update over a host’s connection', () => {
  const IDLE: P.UpdateState = {
    running: '2.5.0',
    channel: 'stable',
    unavailable: null,
    newest: '2.6.0',
    checkedAt: null,
    failed: null,
    checking: false,
    installing: null,
    installFailed: null
  }
  const step = (line: string): P.UpdateState => ({ ...IDLE, installing: { line, waitingForApp: false } })

  function following(host: FakeConnection) {
    const timers = steppedTimers()
    const stop = new AbortController()
    const said: string[] = []
    const end = followUpdate(host.asConnection(), {
      signal: stop.signal,
      progress: (line) => said.push(line),
      graceMs: GRACE,
      setTimeout: timers.setTimeout,
      clearTimeout: timers.clearTimeout
    })
    return { end, timers, stop, said }
  }

  it('a state from before the update started is not its end', async () => {
    const host = new FakeConnection()
    const asked = new Promise<void>((resolve) =>
      host.answer('update:install', () => {
        resolve()
        return null
      })
    )
    const f = following(host)
    host.snapshot('update:state', IDLE)
    expect(f.timers.waiting(GRACE)).toBe(false)
    await asked
    host.push('update:state', step('Starting floti update.'))
    host.close()
    await expect(f.end).resolves.toEqual({ kind: 'ended', lastLine: 'Starting floti update.' })
    expect(f.said).toEqual(['Starting floti update.'])
  })

  it('a refused update:install is the host’s reason', async () => {
    const host = new FakeConnection().answer('update:install', () => {
      throw new Error('This host does not update itself: it runs a floti from inside the desktop app.')
    })
    await expect(following(host).end).resolves.toEqual({
      kind: 'failed',
      lastLine: 'This host does not update itself: it runs a floti from inside the desktop app.'
    })
  })

  it('a connection that ended before the host answered ran nothing', async () => {
    const host = new FakeConnection()
    host.close()
    host.answer('update:install', () => new Promise<null>(() => undefined) as unknown as null)
    await expect(following(host).end).resolves.toEqual({
      kind: 'failed',
      lastLine: 'The connection to the local host was closed.'
    })
  })

  it('a stopped wait ends at once, unsubscribed', async () => {
    const host = new FakeConnection()
    const f = following(host)
    host.push('update:state', step('Downloading floti-2.6.0-linux-x64.'))
    f.stop.abort()
    await expect(f.end).resolves.toEqual({ kind: 'stopped', lastLine: 'Downloading floti-2.6.0-linux-x64.' })
    expect(host.subscriptions.has('update:state')).toBe(false)
  })

  it('reads the host’s update state from the first state it is told, and nothing when it cannot be heard', async () => {
    const host = new FakeConnection()
    const heard = updateStateOf(host.asConnection())
    host.snapshot('update:state', IDLE)
    await expect(heard).resolves.toEqual(IDLE)
    expect(host.subscriptions.has('update:state')).toBe(false)
    const deaf = new FakeConnection()
    const unheard = updateStateOf(deaf.asConnection())
    deaf.subscriptions.get('update:state')?.error?.(new Error('No such channel: update:state.') as never)
    await expect(unheard).resolves.toBeNull()
  })
})
