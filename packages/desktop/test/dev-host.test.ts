import { basename, dirname, join } from 'node:path'
import { describe, expect, it } from 'vitest'
import {
  asidePath,
  buildEnv,
  clientProtocol,
  compareVersions,
  decideBuild,
  devHostPath,
  ensureDevHost,
  hostFiles,
  isAside,
  moveAside,
  parseVersionLine,
  sweepAsides
} from '../scripts/dev-host.mjs'

/**
 * Whether `pnpm dev` builds the host before opening a window. Nothing here runs
 * cargo or floti: the file system, `floti version` and the build are fakes.
 */

const CLIENT = { protocol: { major: 4, minor: 1 }, version: '3.0.0-preview.1' }
const line = (version: string, protocol: string): string => `floti ${version} protocol ${protocol} windows x64\n`

describe('the dev host build decision', () => {
  it('builds when there is no host', () => {
    expect(decideBuild({ exists: false, output: null, client: CLIENT })).toMatchObject({ build: true })
  })

  it('builds when the host does not say its version', () => {
    expect(decideBuild({ exists: true, output: null, client: CLIENT }).build).toBe(true)
    expect(decideBuild({ exists: true, output: 'not a version line', client: CLIENT }).build).toBe(true)
  })

  it('builds when the host speaks an older protocol than the client, major or minor', () => {
    const major = decideBuild({ exists: true, output: line('3.0.0-preview.1', '3.1'), client: CLIENT })
    expect(major).toEqual({ build: true, reason: "it speaks protocol 3.1, older than the client's 4.1" })
    expect(decideBuild({ exists: true, output: line('3.0.0-preview.1', '4.0'), client: CLIENT }).build).toBe(true)
  })

  it('builds when the host is an older version than the client, prereleases in order', () => {
    expect(decideBuild({ exists: true, output: line('2.4.1', '4.1'), client: CLIENT }).build).toBe(true)
    expect(decideBuild({ exists: true, output: line('3.0.0-preview.0', '4.1'), client: CLIENT }).build).toBe(true)
  })

  it('leaves a host that is as new, or newer in protocol or version, alone', () => {
    expect(decideBuild({ exists: true, output: line('3.0.0-preview.1', '4.1'), client: CLIENT }).build).toBe(false)
    expect(decideBuild({ exists: true, output: line('3.0.0', '4.1'), client: CLIENT }).build).toBe(false)
    expect(decideBuild({ exists: true, output: line('3.0.0-preview.1', '4.2'), client: CLIENT }).build).toBe(false)
    expect(decideBuild({ exists: true, output: line('3.0.0-preview.1', '5.0'), client: CLIENT }).build).toBe(false)
  })
})

describe('the pieces it reads', () => {
  it('finds the host where the launcher does, in CARGO_TARGET_DIR or the checkout', () => {
    expect(devHostPath({ repoRoot: '/repo', env: {}, platform: 'linux' })).toBe(join('/repo', 'target', 'debug', 'floti'))
    expect(devHostPath({ repoRoot: '/repo', env: { CARGO_TARGET_DIR: '/t' }, platform: 'win32' })).toBe(
      join('/t', 'debug', 'floti.exe')
    )
  })

  it('parses floti version and the generated protocol', () => {
    expect(parseVersionLine(line('2.4.1-preview.11', '3.1'))).toEqual({
      version: '2.4.1-preview.11',
      protocol: { major: 3, minor: 1 }
    })
    expect(parseVersionLine('floti 2.4.1')).toBeNull()
    expect(clientProtocol('export const PROTOCOL_VERSION = { major: 4, minor: 1 } as const;')).toEqual({ major: 4, minor: 1 })
    expect(clientProtocol('nothing')).toBeNull()
  })

  it('orders versions as semver does', () => {
    expect(compareVersions('3.0.0-preview.1', '3.0.0')).toBe(-1)
    expect(compareVersions('3.0.0-preview.2', '3.0.0-preview.10')).toBe(-1)
    expect(compareVersions('2.4.1-preview.12', '3.0.0-preview.1')).toBe(-1)
    expect(compareVersions('3.0.0', '3.0.0')).toBe(0)
    expect(compareVersions('dev', '3.0.0')).toBeNull()
  })

  it('builds with at most 4 jobs, fewer when asked for fewer', () => {
    expect(buildEnv({}).CARGO_BUILD_JOBS).toBe('4')
    expect(buildEnv({ CARGO_BUILD_JOBS: '16' }).CARGO_BUILD_JOBS).toBe('4')
    expect(buildEnv({ CARGO_BUILD_JOBS: '2' }).CARGO_BUILD_JOBS).toBe('2')
  })
})

describe('the copies put aside', () => {
  const host = join('/t', 'debug', 'floti.exe')
  const deps = join('/t', 'debug', 'deps', 'floti.exe')

  it('names a copy beside its file and recognises only its own', () => {
    expect(asidePath(host, 7)).toBe(`${host}.old-7`)
    expect(hostFiles(host)).toEqual([host, deps])
    expect(isAside('floti.exe.old-1731700000000', 'floti.exe')).toBe(true)
    expect(isAside('floti.exe.old-', 'floti.exe')).toBe(false)
    expect(isAside('floti.exe.old-x', 'floti.exe')).toBe(false)
    expect(isAside('floti.exe', 'floti.exe')).toBe(false)
    expect(isAside('floti-3f2a.exe.old-5', 'floti.exe')).toBe(false)
  })

  it('moves both names aside, in their own folders, and only those that are there', () => {
    const renamed: [string, string][] = []
    const moved = moveAside({
      path: host,
      exists: (file) => file === host || file === deps,
      rename: (from, to) => renamed.push([from, to]),
      stamp: 9
    })
    expect(moved).toEqual([host, deps])
    expect(renamed).toEqual([
      [host, `${host}.old-9`],
      [deps, `${deps}.old-9`]
    ])
    const only: [string, string][] = []
    moveAside({ path: host, exists: (file) => file === host, rename: (from, to) => only.push([from, to]), stamp: 9 })
    expect(only).toEqual([[host, `${host}.old-9`]])
  })

  it('says which file could not be moved', () => {
    expect(() =>
      moveAside({
        path: host,
        exists: () => true,
        rename: () => {
          throw new Error('EPERM')
        },
        stamp: 1
      })
    ).toThrow(/could not move .*floti\.exe aside to build over it: EPERM/)
  })

  it('deletes leftover copies and skips one still in use', () => {
    const folders: Record<string, string[]> = {
      [join('/t', 'debug')]: ['floti.exe', 'floti.exe.old-1', 'floti.exe.old-2', 'floti.pdb'],
      [join('/t', 'debug', 'deps')]: ['floti.exe', 'floti.exe.old-1', 'floti-abc.exe.old-1']
    }
    const removed: string[] = []
    const count = sweepAsides({
      path: host,
      list: (folder) => folders[folder] ?? [],
      remove: (file) => {
        if (file === `${host}.old-2`) throw new Error('EBUSY: in use')
        removed.push(file)
      }
    })
    expect(removed).toEqual([`${host}.old-1`, `${deps}.old-1`])
    expect(count).toBe(2)
  })
})

describe('ensureDevHost', () => {
  const GENERATED = 'export const PROTOCOL_VERSION = { major: 4, minor: 1 } as const;'
  const files: Record<string, string> = {
    'generated.ts': GENERATED,
    'package.json': JSON.stringify({ version: '3.0.0-preview.1' })
  }
  const host = join('/t', 'debug', 'floti.exe')
  const deps = join('/t', 'debug', 'deps', 'floti.exe')

  /** A disk of host files, what `floti version` answers, and a build that fakes cargo. */
  function world(hostOutput: string | null, options: { buildStatus?: number; disk?: string[]; renameFails?: boolean } = {}) {
    const said: string[] = []
    const builds: Record<string, string | undefined>[] = []
    const disk = new Set(options.disk ?? (hostOutput === null ? [] : [host, deps]))
    const order: string[] = []
    return {
      said,
      builds,
      disk,
      order,
      run: () =>
        ensureDevHost({
          repoRoot: '/repo',
          env: { CARGO_TARGET_DIR: '/t' },
          platform: 'win32',
          read: (path) => {
            const found = Object.entries(files).find(([name]) => path.endsWith(name))
            if (found === undefined) throw new Error(`unexpected read ${path}`)
            return found[1]
          },
          exists: (path) => disk.has(path),
          version: () => hostOutput,
          list: (folder) => [...disk].filter((file) => dirname(file) === folder).map((file) => basename(file)),
          rename: (from, to) => {
            if (options.renameFails === true) throw new Error('EACCES')
            order.push('rename')
            disk.delete(from)
            disk.add(to)
          },
          remove: (path) => {
            disk.delete(path)
          },
          now: () => 42,
          build: (env) => {
            order.push('build')
            builds.push(env)
            return options.buildStatus ?? 0
          },
          say: (text) => said.push(text)
        })
    }
  }

  it('builds a host that speaks an older protocol, with at most 4 jobs and the cargo target folder', () => {
    const w = world(line('2.4.1-preview.11', '3.1'))
    expect(w.run()).toMatchObject({ built: true })
    expect(w.builds).toHaveLength(1)
    expect(w.builds[0]).toMatchObject({ CARGO_BUILD_JOBS: '4', CARGO_TARGET_DIR: '/t' })
  })

  it('moves a running host aside before it builds, so the build writes over nothing that is held', () => {
    const w = world(line('2.4.1-preview.11', '3.1'))
    w.run()
    expect(w.order).toEqual(['rename', 'rename', 'build'])
    expect(w.disk).toEqual(new Set([`${host}.old-42`, `${deps}.old-42`]))
  })

  it('builds when there is no host, with nothing to move aside', () => {
    const w = world(null)
    expect(w.run().built).toBe(true)
    expect(w.order).toEqual(['build'])
  })

  it('does not build, or move anything, for a current host', () => {
    const w = world(line('3.0.0-preview.1', '4.1'))
    expect(w.run()).toMatchObject({ built: false })
    expect(w.builds).toEqual([])
    expect(w.order).toEqual([])
  })

  it('deletes the copies an earlier build left at every check, even when no build is needed', () => {
    const w = world(line('3.0.0-preview.1', '4.1'), { disk: [host, deps, `${host}.old-1`, `${deps}.old-1`] })
    w.run()
    expect(w.disk).toEqual(new Set([host, deps]))
    expect(w.said.some((text) => text.includes('removed 2 old host copies'))).toBe(true)
  })

  it('fails with the build failure, so no window opens, when the build fails', () => {
    const w = world(line('2.4.1-preview.11', '3.1'), { buildStatus: 101 })
    expect(() => w.run()).toThrow(/host build failed \(cargo exited 101\)/)
  })

  it('stops before building, saying what failed, when a running copy cannot be moved aside', () => {
    const w = world(line('2.4.1-preview.11', '3.1'), { renameFails: true })
    expect(() => w.run()).toThrow(/could not move .* aside to build over it: EACCES/)
    expect(w.builds).toEqual([])
  })
})
