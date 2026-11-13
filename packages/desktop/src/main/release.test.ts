import { readFileSync, writeFileSync } from 'node:fs'
import { mkdtemp } from 'node:fs/promises'
import { join } from 'node:path'
import { describe, expect, inject, it } from 'vitest'
import { RUN_ROOT } from '../../../../test/run-root'
import { parseSums, sha256Of } from '../../scripts/floti-release.mjs'
import {
  assetsProblem,
  compareVersions,
  desktopFileNames,
  desktopSumsName,
  gate,
  highestVersion,
  parseVersion,
  releaseAssetNames,
  signatureName,
  signedFileNames,
  summaryTable,
  taggedVersions,
  writeDesktopSums
} from '../../scripts/release.mjs'

/**
 * What a release is (scripts/release.mjs): the version gate release.yml asks
 * before building, and the exact files a release carries.
 */

/** `git ls-remote --tags origin 'refs/tags/v*'` output for these tags, annotated ones peeled. */
function lsRemote(tags: string[], { peeled = [] as string[] } = {}): string {
  const sha = 'a'.repeat(40)
  return tags.flatMap((tag) => [`${sha}\trefs/tags/${tag}`, ...(peeled.includes(tag) ? [`${sha}\trefs/tags/${tag}^{}`] : [])]).join('\n') + '\n'
}

describe('versions', () => {
  it('reads <major>.<minor>.<patch> with an optional pre-release part, and nothing else', () => {
    expect(parseVersion('2.4.1')).toEqual({ major: 2, minor: 4, patch: 1, pre: [] })
    expect(parseVersion('2.4.1-preview.11')).toEqual({ major: 2, minor: 4, patch: 1, pre: ['preview', '11'] })
    for (const bad of ['2.4', 'v2.4.1', '2.4.1-', '2.4.01', '2.4.1-preview.01', '2.4.1+build.1', '2.4.1 ', '']) {
      expect(parseVersion(bad), bad).toBeNull()
    }
  })

  it('orders them by semver precedence, pre-release parts included', () => {
    const ascending = [
      '1.0.0-alpha',
      '1.0.0-alpha.1',
      '1.0.0-alpha.beta',
      '1.0.0-beta',
      '1.0.0-beta.2',
      '1.0.0-beta.11',
      '1.0.0-rc.1',
      '1.0.0',
      '1.0.1',
      '1.1.0',
      '2.4.1-preview.9',
      '2.4.1-preview.10',
      '2.4.1',
      '10.0.0'
    ]
    for (let i = 0; i < ascending.length - 1; i++) {
      expect(compareVersions(ascending[i]!, ascending[i + 1]!), `${ascending[i]!} < ${ascending[i + 1]!}`).toBeLessThan(0)
      expect(compareVersions(ascending[i + 1]!, ascending[i]!), `${ascending[i + 1]!} > ${ascending[i]!}`).toBeGreaterThan(0)
    }
    expect(compareVersions('2.4.1-preview.11', '2.4.1-preview.11')).toBe(0)
    expect(highestVersion(['2.4.1-preview.9', '2.4.0', '2.4.1-preview.11', '2.4.1-preview.10'])).toBe('2.4.1-preview.11')
    expect(highestVersion([])).toBeNull()
  })

  it('reads the tagged versions from git ls-remote, skipping peeled lines and setting aside tags that are not versions', () => {
    const text = lsRemote(['v2.4.0', 'v2.4.1-preview.1', 'v1.0', 'nightly'], { peeled: ['v2.4.0'] })
    expect(taggedVersions(text)).toEqual({ versions: ['2.4.0', '2.4.1-preview.1'], others: ['v1.0', 'nightly'] })
    expect(taggedVersions('')).toEqual({ versions: [], others: [] })
  })
})

describe('the version gate', () => {
  const tags = lsRemote(['v2.3.3', 'v2.4.0', 'v2.4.1-preview.9', 'v2.4.1-preview.10', 'v2.4.1-preview.11'])

  it('builds a version greater than every tagged one, and says whether it is a preview', () => {
    expect(gate({ version: '2.4.1-preview.12', lsRemote: tags, dryRun: false })).toMatchObject({
      tag: 'v2.4.1-preview.12',
      preview: true,
      release: true,
      dryRun: false,
      highest: '2.4.1-preview.11'
    })
    expect(gate({ version: '2.4.1', lsRemote: tags, dryRun: false })).toMatchObject({ preview: false, release: true })
    expect(gate({ version: '3.0.0', lsRemote: '', dryRun: false })).toMatchObject({ release: true, highest: null })
  })

  it('does not build a version already tagged, unless this is a dry run', () => {
    expect(gate({ version: '2.4.1-preview.11', lsRemote: tags, dryRun: false })).toMatchObject({ release: false })
    expect(gate({ version: '2.4.1-preview.11', lsRemote: tags, dryRun: true })).toMatchObject({ release: true, dryRun: true })
  })

  it('refuses a version that is not tagged but is not greater than the highest tagged one, in a dry run too', () => {
    for (const dryRun of [false, true]) {
      expect(() => gate({ version: '2.4.1-preview.2', lsRemote: tags, dryRun })).toThrow(
        /2\.4\.1-preview\.2 is not greater than v2\.4\.1-preview\.11/
      )
      expect(() => gate({ version: '2.3.4', lsRemote: tags, dryRun })).toThrow(/not greater than v2\.4\.1-preview\.11/)
    }
  })

  it('refuses a version it cannot read', () => {
    expect(() => gate({ version: '2.4', lsRemote: tags, dryRun: false })).toThrow(/is not <major>\.<minor>\.<patch>/)
  })
})

describe('the files a release carries', () => {
  it('signs the five desktop files and their list, the protocol types and the six floti files, sorted as jq sorts them', () => {
    expect(signedFileNames('3.0.0')).toEqual([
      'Floti-3.0.0-SHA256SUMS',
      'Floti-3.0.0-arm64-mac.zip',
      'Floti-3.0.0-arm64.dmg',
      'Floti-3.0.0-linux-x64.AppImage',
      'Floti-3.0.0-portable.exe',
      'Floti-3.0.0-setup.exe',
      'floti-3.0.0-SHA256SUMS',
      'floti-3.0.0-linux-arm64',
      'floti-3.0.0-linux-x64',
      'floti-3.0.0-macos-arm64',
      'floti-3.0.0-windows-arm64.exe',
      'floti-3.0.0-windows-x64.exe',
      'floti-protocol-3.0.0.tar.gz'
    ])
  })

  it('carries each signed file with its minisign signature beside it, sorted as jq and LC_ALL=C sort sort them', () => {
    const names = releaseAssetNames('3.0.0')
    expect(names).toHaveLength(26)
    expect(names.slice(0, 4)).toEqual([
      'Floti-3.0.0-SHA256SUMS',
      'Floti-3.0.0-SHA256SUMS.minisig',
      'Floti-3.0.0-arm64-mac.zip',
      'Floti-3.0.0-arm64-mac.zip.minisig'
    ])
    expect(names.slice(-2)).toEqual(['floti-protocol-3.0.0.tar.gz', 'floti-protocol-3.0.0.tar.gz.minisig'])
    expect(names.filter((name) => !name.endsWith('.minisig'))).toEqual(signedFileNames('3.0.0'))
    expect(names.filter((name) => name.endsWith('.minisig'))).toEqual(signedFileNames('3.0.0').map(signatureName))
    expect(names).toEqual([...names].sort((a, b) => Buffer.compare(Buffer.from(a), Buffer.from(b))))
    expect(releaseAssetNames('3.0.0', { signed: false })).toEqual(signedFileNames('3.0.0'))
  })

  it('refuses a folder missing one or holding another, naming each', () => {
    const names = releaseAssetNames('3.0.0')
    expect(assetsProblem(names, '3.0.0')).toBeNull()
    expect(assetsProblem([...names.filter((name) => !name.endsWith('.AppImage')), 'notes.md'], '3.0.0')).toBe(
      'missing Floti-3.0.0-linux-x64.AppImage; not a release file: notes.md'
    )
    expect(assetsProblem(releaseAssetNames('2.9.9'), '3.0.0')).toMatch(/^missing Floti-3\.0\.0-SHA256SUMS, .*; not a release file: Floti-2\.9\.9-SHA256SUMS/)
  })

  it('refuses a signed folder missing a signature, and an unsigned one holding one', () => {
    const names = releaseAssetNames('3.0.0')
    expect(assetsProblem(names.filter((name) => name !== 'floti-3.0.0-SHA256SUMS.minisig'), '3.0.0')).toBe('missing floti-3.0.0-SHA256SUMS.minisig')
    expect(assetsProblem(signedFileNames('3.0.0'), '3.0.0', { signed: false })).toBeNull()
    expect(assetsProblem(names, '3.0.0', { signed: false })).toMatch(/^not a release file: Floti-3\.0\.0-SHA256SUMS\.minisig, Floti-3\.0\.0-arm64-mac\.zip\.minisig, /)
  })

  it('writes the SHA-256 list of the desktop files, sorted, in sha256sum format', async () => {
    const folder = await mkdtemp(join(inject(RUN_ROOT), 'desktop sums '))
    for (const name of desktopFileNames('3.0.0')) writeFileSync(join(folder, name), `bytes of ${name}`)
    const file = await writeDesktopSums(folder, '3.0.0')
    expect(file).toBe(join(folder, desktopSumsName('3.0.0')))
    expect(desktopSumsName('3.0.0')).toBe('Floti-3.0.0-SHA256SUMS')
    const listed = parseSums(readFileSync(file, 'utf8'))
    expect(listed.map((entry) => entry.name)).toEqual([...desktopFileNames('3.0.0')].sort())
    for (const { name, sha256 } of listed) expect(sha256).toBe(sha256Of(Buffer.from(`bytes of ${name}`)))
  })

  it('refuses to write the desktop list without every desktop file', async () => {
    const folder = await mkdtemp(join(inject(RUN_ROOT), 'desktop sums missing '))
    writeFileSync(join(folder, 'Floti-3.0.0-setup.exe'), 'setup')
    await expect(writeDesktopSums(folder, '3.0.0')).rejects.toThrow()
  })

  it('lists each file with its size and SHA-256 for the step summary', () => {
    expect(summaryTable([{ name: 'floti-3.0.0-linux-x64', size: 12, sha256: 'f'.repeat(64) }])).toBe(
      `| File | Size | SHA-256 |\n|---|---:|---|\n| \`floti-3.0.0-linux-x64\` | 12 | \`${'f'.repeat(64)}\` |\n`
    )
  })
})
