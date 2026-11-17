import { mkdirSync, mkdtempSync, readFileSync, realpathSync, symlinkSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, expect, inject, it } from 'vitest'
import { RUN_ROOT } from '../../../../test/run-root'
import afterExtract, { ELECTRON_LICENSES, bundleResources } from '../../scripts/after-extract.mjs'
import {
  DESKTOP_TITLE,
  FLOTI_TITLE,
  V8_LIBRARIES,
  copyrightHolders,
  crateClosure,
  desktopNotices,
  flotiNotices,
  isLicenseFileName,
  licenseChoices,
  licenseFilesIn,
  lockedVersions,
  mitText,
  normalizeRepository,
  noticesHold,
  npmClosure,
  parseGitmodules,
  renderNotices,
  repositoryHost,
  resolveNotices,
  v8VendoredName,
  vendorV8,
  type CargoMetadata,
  type NoticePackage
} from '../../scripts/third-party-notices.mjs'

/**
 * The third-party notices every download carries (scripts/third-party-notices.mjs):
 * which packages they cover, where each package's texts come from, the text
 * they make, the vendored V8 file, and the Mac hook that keeps Electron's
 * licenses (scripts/after-extract.mjs). Every folder is a fixture under the
 * run's root; no test runs cargo or reaches the network.
 */

const APACHE = 'Apache License\nVersion 2.0, January 2004\n\n(the text)'

function tempDir(name: string): string {
  return mkdtempSync(join(inject(RUN_ROOT), `notices ${name} `))
}

function write(path: string, text: string): string {
  mkdirSync(dirname(path), { recursive: true })
  writeFileSync(path, text)
  return path
}

function pkg(dir: string, name: string, options: Partial<NoticePackage> = {}): NoticePackage {
  mkdirSync(dir, { recursive: true })
  return { name, version: '1.0.0', license: 'MIT', repository: null, authors: [], dir, named: [], ...options }
}

/** A `cargo metadata` answer for crates in `root`, each `[name, deps]`, deps `[name, kinds]`. */
function metadata(root: string, crates: { name: string; source?: string | null; license?: string; licenseFile?: string; deps?: [string, (string | null)[]][] }[]): CargoMetadata {
  const id = (name: string) => `registry#${name}@1.0.0`
  return {
    packages: crates.map((crate) => ({
      id: id(crate.name),
      name: crate.name,
      version: '1.0.0',
      source: crate.source === undefined ? 'registry+https://github.com/rust-lang/crates.io-index' : crate.source,
      license: crate.license ?? 'MIT',
      license_file: crate.licenseFile ?? null,
      repository: null,
      authors: [],
      manifest_path: join(root, crate.name, 'Cargo.toml')
    })),
    resolve: {
      nodes: crates.map((crate) => ({
        id: id(crate.name),
        deps: (crate.deps ?? []).map(([name, kinds]) => ({ name, pkg: id(name), dep_kinds: kinds.map((kind) => ({ kind })) }))
      }))
    }
  }
}

describe('the crates floti links', () => {
  it("follows floti-host's normal dependencies only, past workspace members, and lists no workspace crate", () => {
    const root = tempDir('closure')
    const crates = crateClosure(
      metadata(root, [
        { name: 'floti-host', source: null, deps: [['zeta', [null]], ['dev-only', ['dev']], ['build-only', ['build']], ['floti-protocol', [null]], ['both', ['dev', null]]] },
        { name: 'floti-protocol', source: null, deps: [['alpha', [null]]] },
        { name: 'zeta', deps: [['alpha', [null]], ['zeta-build', ['build']]] },
        { name: 'alpha', licenseFile: 'LICENSE.txt' },
        { name: 'both' },
        { name: 'dev-only', deps: [['dev-dep', [null]]] },
        { name: 'dev-dep' },
        { name: 'build-only' },
        { name: 'zeta-build' }
      ])
    )
    expect(crates.map((crate) => crate.name)).toEqual(['alpha', 'both', 'zeta'])
    expect(crates[0]).toMatchObject({ version: '1.0.0', license: 'MIT', dir: join(root, 'alpha'), named: ['LICENSE.txt'] })
    expect(crates[1]!.named).toEqual([])
  })

  it('refuses metadata with no floti-host workspace package', () => {
    expect(() => crateClosure(metadata(tempDir('no root'), [{ name: 'floti-host' }]))).toThrow('no workspace package floti-host')
  })
})

describe("a package's license files", () => {
  it('reads the files at its root and in a LICENSES folder, as LF text, and skips HTML pages, source code and empty files', () => {
    const dir = tempDir('files')
    write(join(dir, 'LICENSE-MIT'), 'MIT text  \r\nline two\r\n\r\n')
    write(join(dir, 'LICENSE-APACHE'), `${String.fromCodePoint(0xfeff)}Apache text\n`)
    write(join(dir, 'license.html'), '<p>moved to LICENSE</p>')
    write(join(dir, 'copying-phase.cc'), 'int main() {}')
    write(join(dir, 'NOTICE'), '\n  \n')
    write(join(dir, 'LICENSES', 'BSD-3-Clause.txt'), 'BSD text')
    write(join(dir, 'legal', 'TERMS'), 'named text')
    write(join(dir, 'README.md'), 'not a license')

    expect(licenseFilesIn(dir, ['legal/TERMS', 'missing/FILE'])).toEqual([
      { name: 'LICENSE-APACHE', text: 'Apache text' },
      { name: 'LICENSE-MIT', text: 'MIT text\nline two' },
      { name: 'LICENSES/BSD-3-Clause.txt', text: 'BSD text' },
      { name: 'legal/TERMS', text: 'named text' }
    ])
  })

  it("finds a vendored project's license below the root only when asked, never in tests, examples or node_modules", () => {
    const dir = tempDir('nested')
    write(join(dir, 'LICENSE'), 'own')
    write(join(dir, 'third_party', 'fiat', 'LICENSE'), 'fiat')
    write(join(dir, 'src', 'spin', 'LICENSE-MIT'), 'spin')
    write(join(dir, 'src', 'phase', 'copying-phase.h'), 'code')
    write(join(dir, 'tests', 'data', 'LICENSE'), 'fixture')
    write(join(dir, 'examples', 'LICENSE'), 'example')
    write(join(dir, 'node_modules', 'x', 'LICENSE'), 'another package')
    write(join(dir, '.github', 'LICENSE'), 'hidden')

    expect(licenseFilesIn(dir).map((file) => file.name)).toEqual(['LICENSE'])
    expect(licenseFilesIn(dir, [], { nested: true })).toEqual([
      { name: 'LICENSE', text: 'own' },
      { name: 'src/spin/LICENSE-MIT', text: 'spin' },
      { name: 'third_party/fiat/LICENSE', text: 'fiat' }
    ])
  })

  it('names a license file by its start, and not source code or a page', () => {
    for (const name of ['LICENSE', 'license', 'LICENCE.md', 'LICENSE-APACHE', 'COPYING', 'NOTICE', 'COPYRIGHT', 'UNLICENSE', 'LICENSE.fdlibm']) {
      expect(isLicenseFileName(name), name).toBe(true)
    }
    for (const name of ['license.html', 'copying-phase.cc', 'license.rs', 'notice.js', 'README', 'MIT.txt']) expect(isLicenseFileName(name), name).toBe(false)
  })
})

describe('licenses and repositories', () => {
  it('spells one repository one way, however a manifest gives it', () => {
    const forms = [
      'https://github.com/xtermjs/xterm.js',
      'git+https://github.com/xtermjs/xterm.js.git',
      'git://github.com/xtermjs/xterm.js.git',
      'git@github.com:xtermjs/xterm.js.git',
      'ssh://git@github.com/xtermjs/xterm.js',
      'http://github.com/XtermJS/xterm.js/',
      'https://github.com/xtermjs/xterm.js/tree/master/addons/addon-serialize',
      'github:xtermjs/xterm.js',
      'xtermjs/xterm.js'
    ]
    for (const form of forms) expect(normalizeRepository(form), form).toBe('https://github.com/xtermjs/xterm.js')
    expect(normalizeRepository('https://chromium.googlesource.com/chromium/src.git')).toBe('https://chromium.googlesource.com/chromium/src')
    expect(normalizeRepository(null)).toBeNull()
    expect(normalizeRepository('  ')).toBeNull()
  })

  it('reads a choice of licenses, and gives none for an expression that combines them', () => {
    expect(licenseChoices('MIT OR Apache-2.0')).toEqual(['MIT', 'Apache-2.0'])
    expect(licenseChoices('MIT/Apache-2.0')).toEqual(['MIT', 'Apache-2.0'])
    expect(licenseChoices('Apache-2.0 / MIT')).toEqual(['Apache-2.0', 'MIT'])
    expect(licenseChoices('Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT')).toEqual(['Apache-2.0 WITH LLVM-exception', 'Apache-2.0', 'MIT'])
    expect(licenseChoices('(MIT OR Apache-2.0) AND Unicode-3.0')).toBeNull()
    expect(licenseChoices('BSD-3-Clause')).toEqual(['BSD-3-Clause'])
    expect(licenseChoices(null)).toBeNull()
  })

  it('names the copyright holders without their e-mail addresses, or as the authors of the package', () => {
    expect(copyrightHolders({ name: 'deno_core', authors: ['the Deno authors'] })).toBe('the Deno authors')
    expect(copyrightHolders({ name: 'vsimd', authors: ['Nugine <nugine@example.com>', 'Somebody (https://example.com)'] })).toBe('Nugine, Somebody')
    expect(copyrightHolders({ name: 'ts-rs', authors: [] })).toBe('the ts-rs authors')
    expect(mitText('the Deno authors')).toMatch(/^MIT License\n\nCopyright \(c\) the Deno authors\n\nPermission is hereby granted/)
  })
})

describe("where a package's notice comes from", () => {
  it('takes its own files, a vendored file, a sibling from the same repository, MIT with its authors, or Apache-2.0, in that order', () => {
    const root = tempDir('fallbacks')
    const vendoredDir = join(root, 'third-party')
    write(join(vendoredDir, v8VendoredName('150.4.0')), 'librusty_v8 150.4.0\n\nV8 text')
    const own = pkg(join(root, 'xterm'), '@xterm/xterm', { repository: 'git+https://github.com/xtermjs/xterm.js.git' })
    write(join(own.dir, 'LICENSE'), 'xterm MIT')
    const sibling = pkg(join(root, 'serialize'), '@xterm/addon-serialize', { repository: 'https://github.com/xtermjs/xterm.js' })
    const otherLicense = pkg(join(root, 'other'), 'xterm-other', { repository: 'https://github.com/xtermjs/xterm.js', license: 'Apache-2.0' })
    const mit = pkg(join(root, 'deno'), 'deno_core', { license: 'MIT', authors: ['the Deno authors'] })
    const dual = pkg(join(root, 'dual'), 'objc2-core-foundation', { license: 'Zlib OR Apache-2.0 OR MIT' })
    const apache = pkg(join(root, 'drizzle'), 'drizzle-orm', { license: 'Apache-2.0' })
    const v8 = pkg(join(root, 'v8'), 'v8', { version: '150.4.0' })

    const { resolved, missing } = resolveNotices([own, sibling, otherLicense, mit, dual, apache, v8], { vendoredDir, apacheText: APACHE })
    expect(missing).toEqual([])
    const texts = new Map(resolved.map(({ pkg: each, texts: found }) => [each.name, found]))
    expect(texts.get('@xterm/xterm')).toEqual([{ text: 'xterm MIT', how: null }])
    expect(texts.get('@xterm/addon-serialize')).toEqual([{ text: 'xterm MIT', how: 'license file of @xterm/xterm 1.0.0, from the same repository' }])
    expect(texts.get('xterm-other')).toEqual([{ text: APACHE, how: 'ships no license file: the Apache License 2.0' }])
    expect(texts.get('deno_core')).toEqual([{ text: mitText('the Deno authors'), how: 'ships no license file: the MIT License, with the copyright its manifest names' }])
    expect(texts.get('objc2-core-foundation')![0]!.text).toBe(mitText('the objc2-core-foundation authors'))
    expect(texts.get('drizzle-orm')).toEqual([{ text: APACHE, how: 'ships no license file: the Apache License 2.0' }])
    expect(texts.get('v8')).toEqual([{ text: 'librusty_v8 150.4.0\n\nV8 text', how: 'vendored as v8-150.4.0.txt' }])
  })

  it('names every package no fallback covers, and the v8 version with no vendored file', () => {
    const root = tempDir('missing')
    const bsd = pkg(join(root, 'bsd'), 'some-bsd', { license: 'BSD-3-Clause' })
    const combined = pkg(join(root, 'combined'), 'unicode-ident', { license: '(MIT OR Apache-2.0) AND Unicode-3.0' })
    const v8 = pkg(join(root, 'v8'), 'v8', { version: '151.0.0' })
    const { resolved, missing } = resolveNotices([bsd, combined, v8], { vendoredDir: join(root, 'third-party'), apacheText: APACHE })
    expect(resolved).toEqual([])
    expect(missing.map(({ pkg: each }) => each.name)).toEqual(['some-bsd', 'unicode-ident', 'v8'])
    expect(missing[2]!.why).toMatch(/v8-151\.0\.0\.txt is missing: run node packages\/desktop\/scripts\/third-party-notices\.mjs vendor-v8$/)

    expect(() =>
      flotiNotices({
        metadata: metadata(root, [{ name: 'floti-host', source: null, deps: [['bsd', [null]]] }, { name: 'bsd', license: 'BSD-3-Clause' }]),
        vendoredDir: join(root, 'third-party'),
        apacheText: APACHE
      })
    ).toThrow(/^no notice for one package:\n {2}bsd 1\.0\.0 \(BSD-3-Clause\): it ships no license file/)
  })
})

describe('the notices', () => {
  it('lists every package, then each text once under the packages it covers, the same bytes every time', () => {
    const root = tempDir('render')
    const a = pkg(join(root, 'a'), 'a', { license: 'MIT OR Apache-2.0' })
    const b = pkg(join(root, 'b'), 'b')
    const resolved = [
      { pkg: a, texts: [{ text: 'MIT text\nline', how: null }, { text: APACHE, how: null }] },
      { pkg: b, texts: [{ text: 'MIT   text\n\nline', how: 'license file of a 1.0.0, from the same repository' }] }
    ]
    const text = renderNotices({ title: 'Title', intro: 'What this is.', resolved })
    expect(text).toBe(renderNotices({ title: 'Title', intro: 'What this is.', resolved }))
    expect(text).toBe(
      [
        'Title',
        '=====',
        '',
        'What this is.',
        '',
        'Packages',
        '--------',
        '',
        '  a 1.0.0 (MIT OR Apache-2.0)',
        '  b 1.0.0 (MIT)',
        '',
        '',
        'License texts',
        '-------------',
        '',
        '='.repeat(80),
        'The license text below covers:',
        '  a 1.0.0',
        '  b 1.0.0 (license file of a 1.0.0, from the same repository)',
        '-'.repeat(80),
        '',
        'MIT text\nline',
        '',
        '='.repeat(80),
        'The license text below covers:',
        '  a 1.0.0',
        '-'.repeat(80),
        '',
        APACHE,
        ''
      ].join('\n')
    )
  })

  it("writes floti's from cargo metadata, with every crate it links", () => {
    const root = tempDir('floti')
    write(join(root, 'alpha', 'LICENSE'), 'alpha license')
    write(join(root, 'beta', 'LICENSE-MIT'), 'beta license')
    const text = flotiNotices({
      metadata: metadata(root, [
        { name: 'floti-host', source: null, deps: [['beta', [null]]] },
        { name: 'beta', deps: [['alpha', [null]]] },
        { name: 'alpha' }
      ]),
      vendoredDir: join(root, 'third-party'),
      apacheText: APACHE
    })
    expect(text.startsWith(`${FLOTI_TITLE}\n`)).toBe(true)
    expect(text).toContain('  alpha 1.0.0 (MIT)\n  beta 1.0.0 (MIT)\n')
    expect(text).toContain('alpha license')
    expect(text).toContain('beta license')
    expect(text).not.toContain('\r')
  })
})

describe('the npm packages the desktop app carries', () => {
  /**
   * A hoisted install: the desktop and a workspace package, linked into
   * node_modules as pnpm links them, and third-party packages with a nested
   * copy that wins over the hoisted one.
   */
  function install(): { root: string; desktop: string } {
    const root = tempDir('npm')
    const manifest = (dir: string, body: object) => write(join(dir, 'package.json'), JSON.stringify(body))
    const desktop = join(root, 'packages', 'desktop')
    const core = join(root, 'packages', 'core')
    manifest(desktop, { name: '@floti/desktop', version: '0.0.0', dependencies: { '@floti/core': 'workspace:*', react: '^19' }, devDependencies: { vite: '^7' } })
    manifest(core, { name: '@floti/core', version: '0.0.0', dependencies: { 'drizzle-orm': '^0.45' } })
    mkdirSync(join(root, 'node_modules', '@floti'), { recursive: true })
    symlinkSync(core, join(root, 'node_modules', '@floti', 'core'), 'junction')
    const modules = join(root, 'node_modules')
    manifest(join(modules, 'react'), { name: 'react', version: '19.2.8', license: 'MIT', dependencies: { scheduler: '^0.27' }, optionalDependencies: { 'not-installed': '1' } })
    manifest(join(modules, 'scheduler'), { name: 'scheduler', version: '0.20.0', license: 'MIT' })
    manifest(join(modules, 'react', 'node_modules', 'scheduler'), { name: 'scheduler', version: '0.27.0', license: 'MIT', author: { name: 'Meta' } })
    manifest(join(modules, 'drizzle-orm'), { name: 'drizzle-orm', version: '0.45.2', license: { type: 'Apache-2.0' }, repository: { type: 'git', url: 'git+https://github.com/drizzle-team/drizzle-orm.git' } })
    manifest(join(modules, 'vite'), { name: 'vite', version: '7.0.0', license: 'MIT' })
    write(join(modules, 'react', 'LICENSE'), 'react license')
    write(join(modules, 'react', 'node_modules', 'scheduler', 'LICENSE'), 'scheduler license')
    return { root, desktop }
  }

  it("follows the desktop's dependencies through its workspace packages, lists no workspace package or devDependency, and resolves as Node does", () => {
    const { root, desktop } = install()
    const packages = npmClosure(desktop)
    expect(packages.map((each) => `${each.name}@${each.version}`)).toEqual(['drizzle-orm@0.45.2', 'react@19.2.8', 'scheduler@0.27.0'])
    expect(packages[0]).toMatchObject({ license: 'Apache-2.0', repository: 'git+https://github.com/drizzle-team/drizzle-orm.git', authors: [] })
    expect(packages[2]).toMatchObject({ authors: ['Meta'], dir: join(realpathSync(root), 'node_modules', 'react', 'node_modules', 'scheduler') })
  })

  it('refuses an install that lacks a dependency', () => {
    const { root, desktop } = install()
    write(join(root, 'node_modules', 'react', 'package.json'), JSON.stringify({ name: 'react', version: '19.2.8', dependencies: { gone: '1' } }))
    expect(() => npmClosure(desktop)).toThrow('react depends on gone, which is not installed: run pnpm install')
  })

  it("writes the app's notices, ending with floti's whole", () => {
    const { root, desktop } = install()
    const floti = `${FLOTI_TITLE}\n\nfloti's crates and texts\n`
    const text = desktopNotices({ desktopDir: desktop, flotiText: floti, vendoredDir: join(root, 'third-party'), apacheText: APACHE })
    expect(text.startsWith(`${DESKTOP_TITLE}\n`)).toBe(true)
    expect(text.endsWith(`\n\n${floti}`)).toBe(true)
    expect(text).toContain('  drizzle-orm 0.45.2 (ships no license file: the Apache License 2.0)')
    expect(text).toContain('LICENSE.electron.txt')
    expect(noticesHold(text, floti)).toBe(true)
    expect(noticesHold(text.replace(/\n/g, '\r\n'), floti)).toBe(true)
    expect(noticesHold(text, `${floti}one line more\n`)).toBe(false)
    expect(() => desktopNotices({ desktopDir: desktop, flotiText: 'something else\n', apacheText: APACHE })).toThrow(`do not start with "${FLOTI_TITLE}"`)
  })
})

describe("the vendored notices for the v8 crate's librusty_v8", () => {
  const TAG = 'v150.4.0'
  const GITHUB_LIBRARY = 'v8'
  const gitilesBase = (path: string) => `https://chromium.googlesource.com/x/${path.replaceAll('/', '_')}`
  const sha = (path: string) => `sha-${path.replaceAll('/', '-')}`
  const base64 = (text: string) => Buffer.from(text).toString('base64')

  /** rusty_v8 at TAG and its submodules, as GitHub and gitiles answer; any other URL fails the test. */
  async function get(url: string): Promise<string> {
    const modules = V8_LIBRARIES.map((library) =>
      [`[submodule "${library.path}"]`, `\tpath = ${library.path}`, `\turl = ${library.path === GITHUB_LIBRARY ? 'https://github.com/denoland/v8.git' : `${gitilesBase(library.path)}.git`}`].join('\n')
    )
    const answers = new Map<string, string>([
      [`https://raw.githubusercontent.com/denoland/rusty_v8/${TAG}/.gitmodules`, `${modules.join('\n')}\n`],
      [`https://api.github.com/repos/denoland/rusty_v8/contents/?ref=${TAG}`, JSON.stringify([{ name: 'LICENSE', type: 'file' }, { name: 'README.md', type: 'file' }, { name: 'src', type: 'dir' }])],
      [`https://raw.githubusercontent.com/denoland/rusty_v8/${TAG}/LICENSE`, 'rusty_v8 MIT\r\n'],
      [`https://api.github.com/repos/denoland/v8/contents/?ref=${sha('v8')}`, JSON.stringify([{ name: 'LICENSE', type: 'file' }, { name: 'LICENSE.fdlibm', type: 'file' }, { name: 'BUILD.gn', type: 'file' }])],
      [`https://raw.githubusercontent.com/denoland/v8/${sha('v8')}/LICENSE`, 'V8 BSD'],
      [`https://raw.githubusercontent.com/denoland/v8/${sha('v8')}/LICENSE.fdlibm`, 'fdlibm'],
      ['https://chromium.googlesource.com/chromium/src/+/refs/heads/main?format=JSON', `)]}'\n${JSON.stringify({ commit: 'c0ffee' })}`],
      ['https://chromium.googlesource.com/chromium/src/+/c0ffee/LICENSE?format=TEXT', base64('Chromium BSD')]
    ])
    for (const library of V8_LIBRARIES) {
      answers.set(`https://api.github.com/repos/denoland/rusty_v8/contents/${library.path}?ref=${TAG}`, JSON.stringify({ type: 'submodule', sha: sha(library.path) }))
      if (library.path === GITHUB_LIBRARY) continue
      const base = gitilesBase(library.path)
      const entries = library.fallback === undefined ? [{ name: 'LICENSE', type: 'blob' }, { name: 'license.html', type: 'blob' }, { name: 'src', type: 'tree' }] : [{ name: 'BUILD.gn', type: 'blob' }]
      answers.set(`${base}/+/${sha(library.path)}/?format=JSON`, `)]}'\n${JSON.stringify({ entries })}`)
      answers.set(`${base}/+/${sha(library.path)}/LICENSE?format=TEXT`, base64(`${library.name} license`))
    }
    const answer = answers.get(url)
    if (answer === undefined) throw new Error(`unexpected GET ${url}`)
    return answer
  }

  it("holds rusty_v8's license and each library's at the commit rusty_v8's tag pins, PartitionAlloc's from Chromium", async () => {
    const text = await vendorV8('150.4.0', { get })
    expect(text.startsWith('librusty_v8 150.4.0\n\nThe v8 crate 150.4.0 links librusty_v8')).toBe(true)
    expect(text).toContain(`rusty_v8: LICENSE\nhttps://github.com/denoland/rusty_v8 at ${TAG}\n${'-'.repeat(80)}\n\nrusty_v8 MIT\n`)
    expect(text).toContain(`V8: LICENSE\nhttps://github.com/denoland/v8 at ${sha('v8')}\n`)
    expect(text).toContain('V8: LICENSE.fdlibm\n')
    for (const library of V8_LIBRARIES.filter((each) => each.path !== GITHUB_LIBRARY && each.fallback === undefined)) {
      expect(text).toContain(`${library.name}: LICENSE\n${gitilesBase(library.path)} at ${sha(library.path)}\n${'-'.repeat(80)}\n\n${library.name} license\n`)
    }
    expect(text).toContain('PartitionAlloc: LICENSE of https://chromium.googlesource.com/chromium/src, which its sources name\nhttps://chromium.googlesource.com/chromium/src at c0ffee\n')
    expect(text).not.toContain('license.html')
    expect(text).not.toContain('\r')
    expect(text).not.toContain('='.repeat(80))
    expect(text.endsWith('Chromium BSD\n')).toBe(true)
  })

  it('reads the locked v8 version, the submodules, and only GitHub and googlesource repositories', () => {
    const lock = '[[package]]\nname = "v8"\nversion = "150.4.0"\n\n[[package]]\nname = "v8x"\nversion = "149.4.0-rc.4"\n'
    expect(lockedVersions(lock, 'v8')).toEqual(['150.4.0'])
    expect(lockedVersions(lock, 'deno_core')).toEqual([])
    expect(parseGitmodules('[submodule "v8"]\n\tpath = v8\n\turl = https://github.com/denoland/v8.git\n[submodule "icu"]\r\n\turl = https://x.googlesource.com/icu\r\n\tpath = third_party/icu\r\n')).toEqual({
      v8: 'https://github.com/denoland/v8.git',
      'third_party/icu': 'https://x.googlesource.com/icu'
    })
    expect(repositoryHost('https://github.com/denoland/v8.git')).toEqual({ kind: 'github', owner: 'denoland', repo: 'v8', url: 'https://github.com/denoland/v8' })
    expect(repositoryHost('https://chromium.googlesource.com/chromium/deps/icu.git')).toEqual({
      kind: 'gitiles',
      base: 'https://chromium.googlesource.com/chromium/deps/icu',
      url: 'https://chromium.googlesource.com/chromium/deps/icu'
    })
    expect(() => repositoryHost('https://gitlab.com/a/b')).toThrow('only GitHub and googlesource')
  })
})

describe("Electron's licenses in a Mac build", () => {
  function extracted(): string {
    const appOutDir = tempDir('mac')
    write(join(appOutDir, 'LICENSE'), 'Electron MIT')
    write(join(appOutDir, 'LICENSES.chromium.html'), '<html>credits</html>')
    mkdirSync(join(appOutDir, 'Electron.app', 'Contents', 'Resources'), { recursive: true })
    return appOutDir
  }

  it("copies them into the bundle's Resources under the names a Windows or Linux build gives them", () => {
    const appOutDir = extracted()
    afterExtract({ appOutDir, electronPlatformName: 'darwin' })
    const resources = bundleResources(appOutDir)
    expect(resources).toBe(join(appOutDir, 'Electron.app', 'Contents', 'Resources'))
    expect(ELECTRON_LICENSES.map((each) => each.to)).toEqual(['LICENSE.electron.txt', 'LICENSES.chromium.html'])
    expect(readFileSync(join(resources, 'LICENSE.electron.txt'), 'utf8')).toBe('Electron MIT')
    expect(readFileSync(join(resources, 'LICENSES.chromium.html'), 'utf8')).toBe('<html>credits</html>')
  })

  it('leaves every other platform alone, and fails a Mac build whose download lacks one', () => {
    afterExtract({ appOutDir: join(inject(RUN_ROOT), 'no such folder'), electronPlatformName: 'win32' })
    const appOutDir = extracted()
    const lacking = tempDir('mac lacking')
    mkdirSync(join(lacking, 'Electron.app', 'Contents', 'Resources'), { recursive: true })
    expect(() => afterExtract({ appOutDir: lacking, electronPlatformName: 'darwin' })).toThrow("Electron's download has no LICENSE")
    mkdirSync(join(appOutDir, 'Other.app'))
    expect(() => bundleResources(appOutDir)).toThrow('holds 2 app bundles, not one')
  })
})
