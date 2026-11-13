import { createHash } from 'node:crypto'
import { mkdtemp, readFile, writeFile } from 'node:fs/promises'
import { join, resolve } from 'node:path'
import { describe, expect, inject, it } from 'vitest'
import { RUN_ROOT } from '../../../../test/run-root'
import { RELEASE_TARGETS, formatSums, releaseFileName, sumsFileName } from '../../scripts/floti-release.mjs'
import {
  DESKTOP_FILES,
  INSTALL_SCRIPTS,
  STABLE_CHANNEL,
  installFunction,
  installScripts,
  manifestProblems,
  manifestText,
  releaseFolder,
  releaseManifest,
  releaseUrlProblem,
  scriptsProblems,
  stableVersion,
  updateManifest
} from '../../scripts/floti-dev.mjs'

/**
 * What a stable release copies to floti.dev (scripts/floti-dev.mjs): the
 * install scripts made from scripts/floti.dev's templates around the install
 * function of scripts/install, and update.json, which `floti update` reads.
 * The release workflow's dry run runs the scripts for real in CI runners,
 * against a local server holding the run's files; nothing here runs them.
 */

const REPO_ROOT = resolve(import.meta.dirname, '../../../..')
const VERSION = '3.0.0'
const REPO = 'floti-dev/floti'
const FOLDER = `https://github.com/${REPO}/releases/download/v${VERSION}`

/** A distinct SHA-256 per target, so a script carrying another target's sum shows. */
const SHA = Object.fromEntries(RELEASE_TARGETS.map((target, i) => [target.name, String(i + 1).repeat(64)]))

function sums(version = VERSION, { without = [] as string[] } = {}): string {
  return formatSums(
    RELEASE_TARGETS.filter((target) => !without.includes(target.name)).map((target) => ({
      name: releaseFileName(version, target),
      sha256: SHA[target.name]!
    }))
  )
}

const script = (name: string, input: Partial<Parameters<typeof installScripts>[0]> = {}): string => {
  const found = installScripts({ sums: sums(), version: VERSION, repo: REPO, ...input }).find((each) => each.name === name)
  if (found === undefined) throw new Error(`no ${name}`)
  return found.text
}

const lines = (text: string): string[] => text.trimEnd().split('\n')

describe('the install function an install script carries', () => {
  it('is scripts/install’s function, without the comments above it and the call on its last line', async () => {
    for (const { install } of INSTALL_SCRIPTS) {
      const text = await readFile(install, 'utf8')
      const body = installFunction(text)
      const first = lines(body)[0]!
      expect(first).toMatch(/^(function Install-Floti \{|floti_install\(\) \{)$/)
      expect(lines(body).at(-1)).toBe('}')
      expect(text.replace(/\r\n/g, '\n')).toContain(body)
      expect(body).not.toMatch(/^#/m)
      expect(body).not.toContain('@args')
      expect(body).not.toContain('"$@"')
    }
  })

  it('refuses a script that is not one function called on its last line', () => {
    expect(() => installFunction('# a\nf() {\n  echo hi\n}\necho run\nf\n')).toThrow(/one function/)
    expect(() => installFunction('f() {\n  echo hi\n}\n')).toThrow(/one function/)
    expect(() => installFunction('echo hi\n')).toThrow(/one function/)
  })
})

describe('floti.dev’s install scripts', () => {
  it('write in the version, the release’s folder and each of their targets’ SHA-256s, and no other target’s', () => {
    const sh = script('install.sh')
    const ps1 = script('install.ps1')
    for (const text of [sh, ps1]) {
      expect(text).toContain(`'${VERSION}'`)
      expect(text).toContain(`'${FOLDER}'`)
      expect(text).not.toContain('{{')
    }
    for (const target of RELEASE_TARGETS) {
      const [carries, lacks] = target.os === 'windows' ? [ps1, sh] : [sh, ps1]
      expect(carries).toContain(`'${SHA[target.name]!}'`)
      expect(lacks).not.toContain(SHA[target.name]!)
    }
    expect(sh).toContain('floti_install "$version" "floti-$version-$os-$cpu" "$sha256" "$release"')
    expect(ps1).toContain('Install-Floti $version "floti-$version-$target.exe" $sums[$target] $release')
  })

  it('carry the install function of scripts/install as it is, indented into their own', async () => {
    for (const { name, install } of INSTALL_SCRIPTS) {
      const body = installFunction(await readFile(install, 'utf8'))
      const indented = body
        .split('\n')
        .map((line) => (line === '' ? '' : `  ${line}`))
        .join('\n')
      expect(script(name)).toContain(indented)
    }
  })

  it('are each one function called on its last line, so a script cut off halfway runs nothing', () => {
    for (const [name, call] of [
      ['install.sh', 'floti_get'],
      ['install.ps1', 'Get-Floti']
    ] as const) {
      const all = lines(script(name))
      expect(all.at(-1)).toBe(call)
      const code = all.slice(0, -1).filter((line) => !line.startsWith('#'))
      const start = code.findIndex((line) => line !== '')
      expect(code[start]).toBe(name === 'install.sh' ? `${call}() {` : `function ${call} {`)
      let end = code.length - 1
      while (end > start && code[end] === '') end -= 1
      expect(code[end]).toBe('}')
      // Everything between is inside the function: blank, or indented.
      for (const line of code.slice(start + 1, end)) expect(line === '' || line.startsWith(' '), `${name}: ${line}`).toBe(true)
    }
  })

  it('never exit from install.ps1, which iex runs in the user’s own PowerShell session', () => {
    const code = lines(script('install.ps1')).filter((line) => !line.trimStart().startsWith('#'))
    expect(code.filter((line) => /\bexit\b/i.test(line))).toEqual([])
  })

  it('are refused by the release’s check unless each is the script its signed SHA-256 list makes', () => {
    const made = new Map(installScripts({ sums: sums(), version: VERSION, repo: REPO }).map(({ name, text }) => [name, text]))
    expect(scriptsProblems({ scripts: made, sums: sums(), version: VERSION, repo: REPO })).toEqual([])
    const tampered = new Map(made)
    tampered.set('install.sh', made.get('install.sh')!.replace(SHA['linux-x64']!, 'f'.repeat(64)))
    expect(scriptsProblems({ scripts: tampered, sums: sums(), version: VERSION, repo: REPO })).toEqual([
      "install.sh is not the script 3.0.0's signed SHA-256 list makes"
    ])
    const one = new Map([['install.ps1', made.get('install.ps1')!]])
    expect(scriptsProblems({ scripts: one, sums: sums(), version: VERSION, repo: REPO })).toEqual(['install.sh is missing'])
  })

  it('are printable ASCII, as PowerShell reads a download without a charset as Latin-1', () => {
    for (const { text } of installScripts({ sums: sums(), version: VERSION, repo: REPO })) {
      expect(text).toMatch(/^[\t\n -~]*$/)
    }
  })

  it('download from the release’s own folder, or from the server a dry run gives, http on this computer', () => {
    expect(releaseFolder(REPO, VERSION)).toBe(FOLDER)
    const local = script('install.sh', { releaseUrl: 'http://127.0.0.1:8765/' })
    expect(local).toContain(`release='http://127.0.0.1:8765'`)
    expect(local).not.toContain('github.com')
    expect(script('install.ps1', { releaseUrl: 'http://localhost:8765' })).toContain(`$release = 'http://localhost:8765'`)
  })

  it('refuse a folder that is not https or http to this computer, or that a quoted string would end inside', () => {
    expect(releaseUrlProblem(FOLDER)).toBeNull()
    for (const bad of [
      'http://example.com/v3.0.0',
      'ftp://127.0.0.1/v',
      "https://example.com/it's",
      'https://example.com/a b',
      'https://example.com/v?x=1',
      'https://example.com/v#x',
      'not an address'
    ]) {
      expect(releaseUrlProblem(bad), bad).not.toBeNull()
      expect(() => installScripts({ sums: sums(), version: VERSION, repo: REPO, releaseUrl: bad }), bad).toThrow()
    }
  })

  it('refuse a SHA-256 list that does not name a target, naming its file', () => {
    expect(() => installScripts({ sums: sums(VERSION, { without: ['linux-arm64'] }), version: VERSION, repo: REPO })).toThrow(
      /floti-3\.0\.0-linux-arm64/
    )
  })

  it('refuse a template with a placeholder no release fills, or one that installs no floti for one of its targets', async () => {
    const real = (path: string): Promise<string> => readFile(path, 'utf8')
    const shTemplate = await real(INSTALL_SCRIPTS[0]!.template)
    const files = new Map<string, string>()
    for (const each of INSTALL_SCRIPTS) {
      files.set(each.template, await real(each.template))
      files.set(each.install, await real(each.install))
    }
    const reading = (override: string) => (path: string) => (path === INSTALL_SCRIPTS[0]!.template ? override : files.get(path)!)
    expect(() =>
      installScripts({ sums: sums(), version: VERSION, repo: REPO, read: reading(`${shTemplate}\n# {{channel}}\n`) })
    ).toThrow(/\{\{channel\}\}/)
    expect(() =>
      installScripts({
        sums: sums(),
        version: VERSION,
        repo: REPO,
        read: reading(shTemplate.replace(/^ *linux-arm64\).*$/m, ''))
      })
    ).toThrow(/install\.sh's template installs no floti for linux-arm64/)
    expect(() =>
      installScripts({ sums: sums(), version: VERSION, repo: REPO, read: reading(`${shTemplate}\n# café\n`) })
    ).toThrow(/ASCII/)
  })
})

describe('update.json', () => {
  const floti = new Map(
    RELEASE_TARGETS.map((target) => [target.name, { file: releaseFileName(VERSION, target), sha256: SHA[target.name]! }])
  )
  const desktop = new Map(
    Object.entries(DESKTOP_FILES).map(([target, name], i) => [target, { file: name(VERSION), sha256: 'abc'[i]!.repeat(64) }])
  )
  const manifest = () => updateManifest({ version: VERSION, url: `${FOLDER}/`, floti, desktop })

  it('is the file floti update’s own tests read (crates/host/src/update/fixtures/update.json), byte for byte', async () => {
    const fixture = await readFile(join(REPO_ROOT, 'crates', 'host', 'src', 'update', 'fixtures', 'update.json'), 'utf8')
    expect(manifestText(manifest())).toBe(fixture)
  })

  it('names the stable channel alone, with every floti target and the desktop’s setup exe, Mac zip and AppImage', () => {
    const written = manifest()
    expect(written.schema).toBe(1)
    expect(Object.keys(written.channels)).toEqual([STABLE_CHANNEL])
    const stable = written.channels[STABLE_CHANNEL]!
    expect(stable.version).toBe(VERSION)
    expect(stable.url).toBe(`${FOLDER}/`)
    expect(Object.keys(stable.floti)).toEqual(RELEASE_TARGETS.map((target) => target.name))
    expect(stable.floti['windows-arm64']).toEqual({ file: 'floti-3.0.0-windows-arm64.exe', sha256: SHA['windows-arm64'] })
    expect(stable.desktop).toEqual({
      'windows-x64': { file: 'Floti-3.0.0-setup.exe', sha256: 'a'.repeat(64) },
      'macos-arm64': { file: 'Floti-3.0.0-arm64-mac.zip', sha256: 'b'.repeat(64) },
      'linux-x64': { file: 'Floti-3.0.0-linux-x64.AppImage', sha256: 'c'.repeat(64) }
    })
  })

  it('is refused by the release’s check unless it names the version, its folder and exactly the signed list’s floti files', async () => {
    const fixture = await readFile(join(REPO_ROOT, 'crates', 'host', 'src', 'update', 'fixtures', 'update.json'), 'utf8')
    const check = (text: string) => manifestProblems({ text, sums: sums(), version: VERSION, repo: REPO })
    expect(check(fixture)).toEqual([])
    const changed = (edit: (written: ReturnType<typeof manifest>) => void): string => {
      const written = manifest()
      edit(written)
      return JSON.stringify(written)
    }
    expect(check(changed((m) => (m.channels[STABLE_CHANNEL]!.floti['linux-arm64']!.sha256 = 'f'.repeat(64))))).toEqual([
      "update.json's floti for linux-arm64 is not floti-3.0.0-linux-arm64 with the SHA-256 the list gives"
    ])
    expect(check(changed((m) => (m.channels[STABLE_CHANNEL]!.version = '3.0.1')))).toEqual(['update.json names version 3.0.1, not 3.0.0'])
    expect(check(changed((m) => (m.channels['beta'] = m.channels[STABLE_CHANNEL]!)))).toEqual([
      'update.json names the channels [stable, beta], not stable alone'
    ])
    expect(check('{')[0]).toMatch(/^update\.json is not JSON/)
  })

  it('is never written for a preview, and refuses what floti update would refuse', () => {
    expect(() => updateManifest({ version: '3.0.0-preview.1', url: `${FOLDER}/`, floti, desktop })).toThrow(/preview/)
    expect(() => updateManifest({ version: VERSION, url: FOLDER, floti, desktop })).toThrow(/ending in \//)
    expect(() => updateManifest({ version: VERSION, url: 'http://example.com/v/', floti, desktop })).toThrow(/https/)
    const one = (entry: { file: string; sha256: string }) => new Map([['linux-x64', entry]])
    expect(() => updateManifest({ version: VERSION, url: `${FOLDER}/`, floti: one({ file: '../floti', sha256: SHA['linux-x64']! }), desktop })).toThrow(
      /plain file name/
    )
    expect(() => updateManifest({ version: VERSION, url: `${FOLDER}/`, floti: one({ file: 'floti', sha256: 'AB'.repeat(32) }), desktop })).toThrow(
      /SHA-256/
    )
  })

  it('is made from a release’s folder: floti from its SHA-256 list, the desktop files hashed; a dry run of a preview names the stable form', async () => {
    const preview = '3.0.0-preview.2'
    const folder = await mkdtemp(join(inject(RUN_ROOT), 'floti dev release '))
    await writeFile(join(folder, sumsFileName(preview)), sums(preview))
    const hashes: Record<string, string> = {}
    for (const [target, name] of Object.entries(DESKTOP_FILES)) {
      const bytes = Buffer.from(`the ${target} app`)
      await writeFile(join(folder, name(preview)), bytes)
      hashes[target] = createHash('sha256').update(bytes).digest('hex')
    }
    await expect(releaseManifest({ folder, version: preview, repo: REPO })).rejects.toThrow(/preview/)
    const stable = (await releaseManifest({ folder, version: preview, repo: REPO, asStable: true })).channels[STABLE_CHANNEL]!
    expect(stableVersion(preview)).toBe(VERSION)
    expect(stable.version).toBe(VERSION)
    expect(stable.url).toBe(`${FOLDER}/`)
    expect(stable.floti['macos-arm64']).toEqual({ file: 'floti-3.0.0-macos-arm64', sha256: SHA['macos-arm64'] })
    expect(stable.desktop['linux-x64']).toEqual({ file: 'Floti-3.0.0-linux-x64.AppImage', sha256: hashes['linux-x64'] })
    expect(stable.desktop['windows-x64']).toEqual({ file: 'Floti-3.0.0-setup.exe', sha256: hashes['windows-x64'] })
  })
})
