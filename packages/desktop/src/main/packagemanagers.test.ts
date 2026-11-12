import { describe, expect, it } from 'vitest'
import { RELEASE_TARGETS, formatSums, releaseFileName, releaseFileNames } from '../../scripts/floti-release.mjs'
import {
  DESCRIPTION,
  HOMEBREW_TARGETS,
  WINGET_IDENTIFIER,
  WINGET_MANIFEST_VERSION,
  WINGET_TARGETS,
  downloadUrl,
  homebrewFormula,
  releaseFiles,
  wingetFolder,
  wingetManifests
} from '../../scripts/package-managers.mjs'

/**
 * What a stable release hands to package managers
 * (scripts/package-managers.mjs): the Homebrew formula pushed to
 * floti-dev/homebrew-tap and the winget manifests sent to microsoft/winget-pkgs,
 * both built from the release's own URLs and SHA-256 list. The release
 * workflow's dry run checks them with `brew style`, `brew audit`, a
 * `brew install` and `brew test` of the run's own file, and `winget validate`.
 */

const VERSION = '3.0.0'
const REPO = 'floti-dev/floti'

/** A distinct SHA-256 per target, so a URL paired with another target's sum shows. */
const SHA = Object.fromEntries(RELEASE_TARGETS.map((target, i) => [target.name, String(i + 1).repeat(64)]))

function sums(version = VERSION, { without = [] as string[] } = {}): string {
  return formatSums(
    RELEASE_TARGETS.filter((target) => !without.includes(target.name)).map((target) => ({
      name: releaseFileName(version, target),
      sha256: SHA[target.name]!
    }))
  )
}

const url = (target: string, version = VERSION): string =>
  downloadUrl(REPO, version, releaseFileName(version, RELEASE_TARGETS.find((each) => each.name === target)!))

/** The body of the first `<name> do ... end` block at `indent` spaces, or null. */
function block(text: string, name: string, indent: number): string | null {
  const pad = ' '.repeat(indent)
  const start = text.indexOf(`\n${pad}${name} do\n`)
  if (start < 0) return null
  const body = text.slice(start + pad.length + name.length + 5)
  return body.slice(0, body.indexOf(`\n${pad}end\n`) + 1)
}

/** A flat YAML document's `key: value` lines as a map; list items are skipped. */
function topLevel(yaml: string): Map<string, string> {
  const entries = yaml
    .split('\n')
    .filter((line) => /^[A-Za-z]+: /.test(line))
    .map((line) => [line.slice(0, line.indexOf(': ')), line.slice(line.indexOf(': ') + 2)] as const)
  return new Map(entries)
}

describe('the files a package manager gets', () => {
  it('are the release files of its targets, at the release download URL, with the listed SHA-256', () => {
    const files = releaseFiles(sums(), VERSION, REPO, [...HOMEBREW_TARGETS, ...WINGET_TARGETS])
    expect([...files.keys()]).toEqual(['macos-arm64', 'linux-x64', 'linux-arm64', 'windows-x64', 'windows-arm64'])
    expect(files.get('windows-arm64')).toEqual({
      name: 'floti-3.0.0-windows-arm64.exe',
      url: 'https://github.com/floti-dev/floti/releases/download/v3.0.0/floti-3.0.0-windows-arm64.exe',
      sha256: SHA['windows-arm64']
    })
    // Every name a package manager is given is one the release carries.
    for (const file of files.values()) expect(releaseFileNames(VERSION)).toContain(file.name)
  })

  it('reads a SHA-256 list written on Windows', () => {
    const files = releaseFiles(sums().replaceAll('\n', '\r\n'), VERSION, REPO, HOMEBREW_TARGETS)
    expect(files.get('linux-x64')?.sha256).toBe(SHA['linux-x64'])
  })

  it('refuses a list that does not name a target, naming the file', () => {
    expect(() => releaseFiles(sums(VERSION, { without: ['linux-arm64'] }), VERSION, REPO, HOMEBREW_TARGETS)).toThrow(
      'the SHA-256 list does not name floti-3.0.0-linux-arm64'
    )
    // Another version's list names none of this version's files.
    expect(() => releaseFiles(sums('2.9.0'), VERSION, REPO, WINGET_TARGETS)).toThrow(
      'does not name floti-3.0.0-windows-x64.exe, floti-3.0.0-windows-arm64.exe'
    )
  })

  it('refuses a version that is not a release version and a repository that is not owner/name', () => {
    expect(() => releaseFiles(sums(), 'v3.0.0', REPO, HOMEBREW_TARGETS)).toThrow("'v3.0.0' is not a release version")
    expect(() => releaseFiles(sums(), VERSION, 'floti', HOMEBREW_TARGETS)).toThrow("'floti' is not <owner>/<repository>")
    expect(() => releaseFiles(sums(), VERSION, 'floti-dev/floti"; system "x', HOMEBREW_TARGETS)).toThrow('is not <owner>/<repository>')
  })
})

describe('the Homebrew formula', () => {
  const formula = homebrewFormula({ sums: sums(), version: VERSION, repo: REPO })

  it('is the class Floti, for this version, with its description and licence', () => {
    expect(formula).toMatch(/^# Written by the release workflow of floti-dev\/floti/)
    expect(formula).toContain('\nclass Floti < Formula\n')
    expect(formula).toContain(`\n  desc "${DESCRIPTION}"\n`)
    expect(formula).toContain('\n  homepage "https://github.com/floti-dev/floti"\n')
    expect(formula).toContain('\n  version "3.0.0"\n')
    expect(formula).toContain('\n  license "MIT"\n')
    expect(formula.endsWith('\nend\n')).toBe(true)
    // Homebrew's own rules for a description: no article first, no full stop, at most 80 characters.
    expect(DESCRIPTION).not.toMatch(/^(A|An|The|Floti)\b/i)
    expect(DESCRIPTION).not.toMatch(/\.$/)
    expect(DESCRIPTION.length).toBeLessThanOrEqual(80)
  })

  it('downloads the macOS arm64 build on a Mac, and refuses an Intel Mac', () => {
    const mac = block(formula, 'on_macos', 2)
    expect(mac).toBe(`    url "${url('macos-arm64')}"\n    sha256 "${SHA['macos-arm64']!}"\n    depends_on arch: :arm64\n`)
  })

  it('downloads the static Linux build for the CPU on Linux', () => {
    const linux = block(formula, 'on_linux', 2)
    expect(linux).not.toBeNull()
    expect(block(`\n${linux!}`, 'on_intel', 4)).toBe(`      url "${url('linux-x64')}"\n      sha256 "${SHA['linux-x64']!}"\n`)
    expect(block(`\n${linux!}`, 'on_arm', 4)).toBe(`      url "${url('linux-arm64')}"\n      sha256 "${SHA['linux-arm64']!}"\n`)
  })

  it('carries nothing for Windows and nothing but the binary', () => {
    expect(formula).not.toContain('windows')
    expect(formula).not.toContain('Helm-')
    expect(formula).not.toContain('cask')
    expect(formula.match(/^\s*url "/gm)).toHaveLength(3)
  })

  it('installs the downloaded file as floti, executable', () => {
    expect(formula).toContain(
      '  def install\n    binary = File.basename(stable.url)\n    chmod 0755, binary\n    bin.install binary => "floti"\n  end\n'
    )
  })

  it('tests the installed floti by running floti version', () => {
    expect(block(formula, 'test', 2)).toBe(
      '    assert_match(/^floti #{Regexp.escape(version.to_s)} protocol /, shell_output("#{bin}/floti version"))\n'
    )
  })

  it('is the same text for the same release', () => {
    expect(homebrewFormula({ sums: sums(), version: VERSION, repo: REPO })).toBe(formula)
  })
})

describe('the winget manifests', () => {
  const manifests = wingetManifests({ sums: sums(), version: VERSION, repo: REPO })
  const byName = new Map(manifests.map(({ path, text }) => [path.split('/').at(-1)!, text]))

  it('are the version, installer and en-US locale manifests, where winget-pkgs keeps them', () => {
    expect(wingetFolder(VERSION)).toBe('manifests/f/FlotiDev/Floti/3.0.0')
    expect(manifests.map(({ path }) => path)).toEqual([
      'manifests/f/FlotiDev/Floti/3.0.0/FlotiDev.Floti.yaml',
      'manifests/f/FlotiDev/Floti/3.0.0/FlotiDev.Floti.installer.yaml',
      'manifests/f/FlotiDev/Floti/3.0.0/FlotiDev.Floti.locale.en-US.yaml'
    ])
  })

  it('each name the package, the version, its type and the schema it is written to', () => {
    const types = { 'FlotiDev.Floti.yaml': 'version', 'FlotiDev.Floti.installer.yaml': 'installer', 'FlotiDev.Floti.locale.en-US.yaml': 'defaultLocale' }
    for (const [name, type] of Object.entries(types)) {
      const text = byName.get(name)!
      const fields = topLevel(text)
      expect(text.split('\n')[0], name).toBe(`# yaml-language-server: $schema=https://aka.ms/winget-manifest.${type}.${WINGET_MANIFEST_VERSION}.schema.json`)
      expect(fields.get('PackageIdentifier'), name).toBe(WINGET_IDENTIFIER)
      expect(fields.get('PackageVersion'), name).toBe(VERSION)
      expect(fields.get('ManifestType'), name).toBe(type)
      expect(fields.get('ManifestVersion'), name).toBe(WINGET_MANIFEST_VERSION)
      expect(text.endsWith('\n'), name).toBe(true)
      expect(text, name).not.toContain('\r')
    }
    expect(topLevel(byName.get('FlotiDev.Floti.yaml')!).get('DefaultLocale')).toBe('en-US')
  })

  it('install the Windows x64 and arm64 exes as a portable command named floti', () => {
    const installer = byName.get('FlotiDev.Floti.installer.yaml')!
    expect(topLevel(installer).get('InstallerType')).toBe('portable')
    expect(installer).toContain('\nCommands:\n- floti\n')
    expect(installer).toContain(
      [
        'Installers:',
        '- Architecture: x64',
        `  InstallerUrl: ${url('windows-x64')}`,
        `  InstallerSha256: ${SHA['windows-x64']!.toUpperCase()}`,
        '- Architecture: arm64',
        `  InstallerUrl: ${url('windows-arm64')}`,
        `  InstallerSha256: ${SHA['windows-arm64']!.toUpperCase()}`,
        'ManifestType: installer'
      ].join('\n')
    )
    expect(installer).not.toMatch(/macos|linux/)
  })

  it('describe the package from the release', () => {
    const locale = topLevel(byName.get('FlotiDev.Floti.locale.en-US.yaml')!)
    expect(locale.get('PackageLocale')).toBe('en-US')
    expect(locale.get('PackageName')).toBe('Floti')
    expect(locale.get('Publisher')).toBe('Floti')
    expect(locale.get('Moniker')).toBe('floti')
    expect(locale.get('License')).toBe('MIT')
    expect(locale.get('ShortDescription')).toBe(DESCRIPTION)
    expect(locale.get('PackageUrl')).toBe('https://github.com/floti-dev/floti')
    expect(locale.get('LicenseUrl')).toBe('https://github.com/floti-dev/floti/blob/v3.0.0/LICENSE')
    expect(locale.get('ReleaseNotesUrl')).toBe('https://github.com/floti-dev/floti/releases/tag/v3.0.0')
    // A plain YAML scalar ends at ": " or starts a comment at " #".
    expect(DESCRIPTION).not.toMatch(/: | #/)
  })

  it('put a pre-release version in its own folder, as a dry run of a preview builds them', () => {
    const preview = '2.4.1-preview.11'
    const paths = wingetManifests({ sums: sums(preview), version: preview, repo: REPO }).map(({ path }) => path)
    expect(paths.every((path) => path.startsWith('manifests/f/FlotiDev/Floti/2.4.1-preview.11/'))).toBe(true)
  })
})
