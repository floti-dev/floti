import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import {
  buildEnv,
  clientProtocol,
  compareVersions,
  decideBuild,
  devHostPath,
  ensureDevHost,
  parseVersionLine
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

describe('ensureDevHost', () => {
  const GENERATED = 'export const PROTOCOL_VERSION = { major: 4, minor: 1 } as const;'
  const files: Record<string, string> = {
    'generated.ts': GENERATED,
    'package.json': JSON.stringify({ version: '3.0.0-preview.1' })
  }

  function world(hostOutput: string | null, buildStatus = 0) {
    const said: string[] = []
    const builds: Record<string, string | undefined>[] = []
    const present = hostOutput !== undefined
    return {
      said,
      builds,
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
          exists: () => present && hostOutput !== 'absent',
          version: () => (hostOutput === 'absent' ? null : hostOutput),
          build: (env) => {
            builds.push(env)
            return buildStatus
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

  it('builds when there is no host', () => {
    const w = world('absent')
    expect(w.run().built).toBe(true)
  })

  it('does not build a current host', () => {
    const w = world(line('3.0.0-preview.1', '4.1'))
    expect(w.run()).toMatchObject({ built: false })
    expect(w.builds).toEqual([])
  })

  it('fails with the build failure, so no window opens, when the build fails', () => {
    const w = world(line('2.4.1-preview.11', '3.1'), 101)
    expect(() => w.run()).toThrow(/host build failed \(cargo exited 101\)/)
  })
})

