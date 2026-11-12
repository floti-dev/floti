// What a stable release hands to package managers, for
// .github/workflows/release.yml: the Homebrew formula floti-dev/homebrew-tap
// carries as Formula/floti.rb, and the winget manifests of FlotiDev.Floti that
// go to microsoft/winget-pkgs. Both carry the floti binary only (decision I9),
// at the release's own download URLs, with the SHA-256s of the release's own
// floti-<version>-SHA256SUMS. Nothing here reads a file but that list.
//
//   node package-managers.mjs formula <sums> <version> <owner/repo>
//     prints the formula.
//
//   node package-managers.mjs winget <sums> <version> <owner/repo> <folder>
//     writes the three manifests under <folder>, at the path winget-pkgs keeps
//     them (manifests/f/FlotiDev/Floti/<version>/), and prints each path.

import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseSums, releaseFileName, targetNamed } from './floti-release.mjs'
import { parseVersion } from './release.mjs'

/** The release targets Homebrew installs floti on. */
export const HOMEBREW_TARGETS = Object.freeze(['macos-arm64', 'linux-x64', 'linux-arm64'])

/** The release targets winget installs floti on. */
export const WINGET_TARGETS = Object.freeze(['windows-x64', 'windows-arm64'])

export const WINGET_IDENTIFIER = 'FlotiDev.Floti'

/** The winget manifest schema the manifests are written to. */
export const WINGET_MANIFEST_VERSION = '1.10.0'

/** One line about floti, for Homebrew's `desc` and winget's ShortDescription. */
export const DESCRIPTION = 'Host for Claude Code sessions, reached from any computer over SSH'

const LICENSE = 'MIT'
const REPO = /^[A-Za-z0-9-]+\/[A-Za-z0-9._-]+$/
const SHA256 = /^[0-9a-f]{64}$/

function checkRelease(version, repo) {
  if (parseVersion(version) === null) throw new Error(`'${version}' is not a release version`)
  if (!REPO.test(repo)) throw new Error(`'${repo}' is not <owner>/<repository>`)
}

/** Where a release's file downloads from. */
export function downloadUrl(repo, version, name) {
  return `https://github.com/${repo}/releases/download/v${version}/${name}`
}

/**
 * `{ name, url, sha256 }` for each of `targets`, from the release's SHA-256
 * list. Refuses a list that does not name one of them.
 */
export function releaseFiles(sumsText, version, repo, targets) {
  checkRelease(version, repo)
  const listed = new Map(parseSums(sumsText).map(({ name, sha256 }) => [name, sha256]))
  const missing = []
  const files = new Map()
  for (const targetName of targets) {
    const name = releaseFileName(version, targetNamed(targetName))
    const sha256 = listed.get(name)
    if (sha256 === undefined || !SHA256.test(sha256)) missing.push(name)
    else files.set(targetName, { name, url: downloadUrl(repo, version, name), sha256 })
  }
  if (missing.length > 0) throw new Error(`the SHA-256 list does not name ${missing.join(', ')}`)
  return files
}

/**
 * The Homebrew formula for floti `version`, one binary per OS and CPU it
 * installs on, each URL in an `on_arm` or `on_intel` block inside `on_macos`
 * or `on_linux`: Homebrew's style allows no `url` directly in an OS block.
 * The version is written out because Homebrew cannot read it from these
 * URLs: its parser takes the digits at the end of a name, the 64 of
 * `-arm64` or `-x64`. The dry run audits the stable form as well as its own. The release's file is the whole download, so `install` takes it
 * by its own name, the last part of the URL Homebrew fetched it from.
 */
export function homebrewFormula({ sums, version, repo }) {
  const files = releaseFiles(sums, version, repo, HOMEBREW_TARGETS)
  const source = (target, indent) => {
    const { url, sha256 } = files.get(target)
    return `${indent}url "${url}"\n${indent}sha256 "${sha256}"\n`
  }
  return `# Written by the release workflow of ${repo} for each release; a change made here is replaced by the next.
class Floti < Formula
  desc "${DESCRIPTION}"
  homepage "https://github.com/${repo}"
  version "${version}"
  license "${LICENSE}"

  on_macos do
    depends_on arch: :arm64

    on_arm do
${source('macos-arm64', '      ')}    end
  end

  on_linux do
    on_intel do
${source('linux-x64', '      ')}    end
    on_arm do
${source('linux-arm64', '      ')}    end
  end

  def install
    binary = File.basename(stable.url)
    chmod 0755, binary
    bin.install binary => "floti"
  end

  test do
    assert_match(/^floti #{Regexp.escape(version.to_s)} protocol /, shell_output("#{bin}/floti version"))
  end
end
`
}

/** The folder winget-pkgs keeps a version's manifests in. */
export function wingetFolder(version) {
  const [publisher, name] = WINGET_IDENTIFIER.split('.')
  return `manifests/${publisher[0].toLowerCase()}/${publisher}/${name}/${version}`
}

function manifest(type, version, body) {
  return [
    `# yaml-language-server: $schema=https://aka.ms/winget-manifest.${type}.${WINGET_MANIFEST_VERSION}.schema.json`,
    '',
    `PackageIdentifier: ${WINGET_IDENTIFIER}`,
    `PackageVersion: ${version}`,
    ...body,
    `ManifestType: ${type}`,
    `ManifestVersion: ${WINGET_MANIFEST_VERSION}`,
    ''
  ].join('\n')
}

/**
 * The winget manifests of floti `version`, as `{ path, text }` with `path`
 * relative to the root of winget-pkgs: the version manifest, the installer
 * manifest (a portable exe per CPU, run as `floti`) and the en-US locale.
 */
export function wingetManifests({ sums, version, repo }) {
  const files = releaseFiles(sums, version, repo, WINGET_TARGETS)
  const folder = wingetFolder(version)
  const installers = WINGET_TARGETS.flatMap((targetName) => {
    const { url, sha256 } = files.get(targetName)
    return [
      // winget's architecture names are the release's CPU names: x64, arm64.
      `- Architecture: ${targetNamed(targetName).cpu}`,
      `  InstallerUrl: ${url}`,
      `  InstallerSha256: ${sha256.toUpperCase()}`
    ]
  })
  return [
    {
      path: `${folder}/${WINGET_IDENTIFIER}.yaml`,
      text: manifest('version', version, ['DefaultLocale: en-US'])
    },
    {
      path: `${folder}/${WINGET_IDENTIFIER}.installer.yaml`,
      text: manifest('installer', version, ['InstallerType: portable', 'Commands:', '- floti', 'Installers:', ...installers])
    },
    {
      path: `${folder}/${WINGET_IDENTIFIER}.locale.en-US.yaml`,
      text: manifest('defaultLocale', version, [
        'PackageLocale: en-US',
        'Publisher: Floti',
        `PublisherUrl: https://github.com/${repo.split('/')[0]}`,
        'PackageName: Floti',
        `PackageUrl: https://github.com/${repo}`,
        `License: ${LICENSE}`,
        `LicenseUrl: https://github.com/${repo}/blob/v${version}/LICENSE`,
        `ShortDescription: ${DESCRIPTION}`,
        'Moniker: floti',
        `ReleaseNotesUrl: https://github.com/${repo}/releases/tag/v${version}`
      ])
    }
  ]
}

function main(args) {
  const [command, ...rest] = args
  if (command === 'formula' && rest.length === 3) {
    const [sumsFile, version, repo] = rest
    process.stdout.write(homebrewFormula({ sums: readFileSync(sumsFile, 'utf8'), version, repo }))
  } else if (command === 'winget' && rest.length === 4) {
    const [sumsFile, version, repo, folder] = rest
    for (const { path, text } of wingetManifests({ sums: readFileSync(sumsFile, 'utf8'), version, repo })) {
      const file = join(folder, path)
      mkdirSync(dirname(file), { recursive: true })
      writeFileSync(file, text)
      console.log(path)
    }
  } else {
    console.error('usage: node package-managers.mjs formula <sums> <version> <owner/repo> | winget <sums> <version> <owner/repo> <folder>')
    return 2
  }
  return 0
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    process.exitCode = main(process.argv.slice(2))
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error))
    process.exitCode = 1
  }
}
