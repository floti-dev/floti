import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { mkdtemp } from 'node:fs/promises'
import { join } from 'node:path'
import { afterEach, describe, expect, inject, it, vi } from 'vitest'
import { RUN_ROOT } from '../../../../test/run-root'
import {
  runMinisign,
  signatureAlgorithm,
  signatureFile,
  signFiles,
  trustedComment,
  verifyProblems,
  withSecretKeyFile,
  type MinisignResult
} from '../../scripts/minisign.mjs'

/**
 * Signing and checking a release's files (scripts/minisign.mjs). minisign
 * itself is faked: each test records the arguments it would get and answers
 * as it would. The release workflow runs the real one, in a dry run on every
 * push to a release-dry-run branch.
 */

const SECRET = 'untrusted comment: minisign secret key\nRWQAAEIyAAAA\n'

/** A signature file's text naming `algorithm`, in minisign's four-line form. */
function signatureText(algorithm: string, comment = 'floti 3.0.0 f'): string {
  const signature = Buffer.concat([Buffer.from(algorithm, 'latin1'), Buffer.alloc(72, 7)]).toString('base64')
  return `untrusted comment: signature from minisign secret key\n${signature}\ntrusted comment: ${comment}\n${Buffer.alloc(64, 9).toString('base64')}\n`
}

async function folderWith(names: string[]): Promise<string> {
  const folder = await mkdtemp(join(inject(RUN_ROOT), 'minisign files '))
  for (const name of names) writeFileSync(join(folder, name), name)
  return folder
}

const ok = (stdout = ''): MinisignResult => ({ status: 0, stdout, stderr: '' })

afterEach(() => {
  vi.unstubAllEnvs()
})

describe('the trusted comment', () => {
  it('puts the file name where the comment says {name}, and takes a comment without one as it is', () => {
    expect(trustedComment('floti 3.0.0 {name}', '/tmp/release/floti-3.0.0-linux-x64')).toBe('floti 3.0.0 floti-3.0.0-linux-x64')
    expect(trustedComment('floti update.json', '/tmp/site/update.json')).toBe('floti update.json')
  })

  it('refuses one that is empty or more than one line', () => {
    expect(() => trustedComment(' ', 'f')).toThrow(/must be one line of text/)
    expect(() => trustedComment('floti\n3.0.0 {name}', 'f')).toThrow(/must be one line of text/)
  })
})

describe('a signature file', () => {
  it('names its algorithm: ED when prehashed, Ed when legacy, and nothing when it is not a signature', () => {
    expect(signatureAlgorithm(signatureText('ED'))).toBe('ED')
    expect(signatureAlgorithm(signatureText('Ed'))).toBe('Ed')
    expect(signatureAlgorithm(signatureText('ED').replace(/\n/g, '\r\n'))).toBe('ED')
    expect(signatureAlgorithm('')).toBeNull()
    expect(signatureAlgorithm('untrusted comment: x\nnot base64!\n')).toBeNull()
    expect(signatureAlgorithm(`untrusted comment: x\n${Buffer.alloc(10).toString('base64')}\n`)).toBeNull()
  })

  it('goes beside the file it signs', () => {
    expect(signatureFile('/r/Floti-3.0.0-setup.exe')).toBe('/r/Floti-3.0.0-setup.exe.minisig')
  })
})

describe('the secret key file', () => {
  it('holds the key, in a folder of its own under the temp root, and is gone once the work is done', async () => {
    const root = await mkdtemp(join(inject(RUN_ROOT), 'minisign temp '))
    let seen = ''
    const returned = withSecretKeyFile(SECRET.replace(/\n/g, '\r\n').trimEnd(), root, (file) => {
      expect(file.startsWith(join(root, 'minisign-'))).toBe(true)
      seen = readFileSync(file, 'utf8')
      return 'done'
    })
    expect(returned).toBe('done')
    expect(seen).toBe(SECRET)
    expect(readdirSync(root)).toEqual([])
  })

  it('is gone when the work fails', async () => {
    const root = await mkdtemp(join(inject(RUN_ROOT), 'minisign temp '))
    let path = ''
    expect(() =>
      withSecretKeyFile(SECRET, root, (file) => {
        path = file
        throw new Error('minisign failed')
      })
    ).toThrow('minisign failed')
    expect(existsSync(path)).toBe(false)
    expect(readdirSync(root)).toEqual([])
  })

  it.skipIf(process.platform === 'win32')('is readable by its owner alone', async () => {
    const root = await mkdtemp(join(inject(RUN_ROOT), 'minisign temp '))
    withSecretKeyFile(SECRET, root, (file) => {
      expect(statSync(file).mode & 0o777).toBe(0o600)
    })
  })

  it('is never written for a secret that is empty or not set', async () => {
    const root = await mkdtemp(join(inject(RUN_ROOT), 'minisign temp '))
    const use = vi.fn()
    expect(() => withSecretKeyFile(undefined, root, use)).toThrow('MINISIGN_SECRET_KEY is empty or not set')
    expect(() => withSecretKeyFile(' \n', root, use)).toThrow('MINISIGN_SECRET_KEY is empty or not set')
    expect(use).not.toHaveBeenCalled()
    expect(readdirSync(root)).toEqual([])
  })
})

describe('signing', () => {
  it('signs each file beside it with its own trusted comment, with the key file minisign is given', async () => {
    const folder = await folderWith(['floti-3.0.0-linux-x64', 'Floti-3.0.0-setup.exe'])
    const root = await mkdtemp(join(inject(RUN_ROOT), 'minisign temp '))
    const calls: string[][] = []
    const keys: string[] = []
    const files = [join(folder, 'floti-3.0.0-linux-x64'), join(folder, 'Floti-3.0.0-setup.exe')]

    const signed = signFiles({
      files,
      comment: 'floti 3.0.0 {name}',
      secretKey: SECRET,
      tempRoot: root,
      run: (args) => {
        calls.push(args)
        keys.push(readFileSync(args[2]!, 'utf8'))
        return ok()
      }
    })

    const keyFile = calls[0]![2]!
    expect(calls).toEqual([
      ['-S', '-s', keyFile, '-m', files[0], '-x', `${files[0]!}.minisig`, '-t', 'floti 3.0.0 floti-3.0.0-linux-x64'],
      ['-S', '-s', keyFile, '-m', files[1], '-x', `${files[1]!}.minisig`, '-t', 'floti 3.0.0 Floti-3.0.0-setup.exe']
    ])
    expect(keys).toEqual([SECRET, SECRET])
    expect(signed.map((each) => each.comment)).toEqual(['floti 3.0.0 floti-3.0.0-linux-x64', 'floti 3.0.0 Floti-3.0.0-setup.exe'])
    expect(existsSync(keyFile)).toBe(false)
    expect(readdirSync(root)).toEqual([])
  })

  it('stops at the first file minisign cannot sign, naming it, and leaves no key behind', async () => {
    const folder = await folderWith(['a', 'b'])
    const root = await mkdtemp(join(inject(RUN_ROOT), 'minisign temp '))
    const run = vi.fn((): MinisignResult => ({ status: 1, stdout: '', stderr: 'Error while loading the secret key file\n' }))
    expect(() =>
      signFiles({ files: [join(folder, 'a'), join(folder, 'b')], comment: 'c {name}', secretKey: SECRET, tempRoot: root, run })
    ).toThrow('minisign could not sign a: Error while loading the secret key file')
    expect(run).toHaveBeenCalledTimes(1)
    expect(readdirSync(root)).toEqual([])
  })

  it('refuses a file that is not there, or no file at all, before the key is written', async () => {
    const folder = await folderWith(['a'])
    const root = await mkdtemp(join(inject(RUN_ROOT), 'minisign temp '))
    const run = vi.fn(() => ok())
    expect(() =>
      signFiles({ files: [join(folder, 'a'), join(folder, 'gone')], comment: 'c', secretKey: SECRET, tempRoot: root, run })
    ).toThrow(/^no such file: .*gone$/)
    expect(() => signFiles({ files: [], comment: 'c', secretKey: SECRET, tempRoot: root, run })).toThrow('no files to sign')
    expect(run).not.toHaveBeenCalled()
    expect(readdirSync(root)).toEqual([])
  })
})

describe('verifying', () => {
  async function signedFolder(signatures: Record<string, string>): Promise<{ folder: string; publicKey: string }> {
    const folder = await folderWith([...Object.keys(signatures), 'minisign.pub'])
    for (const [name, text] of Object.entries(signatures)) writeFileSync(signatureFile(join(folder, name)), text)
    return { folder, publicKey: join(folder, 'minisign.pub') }
  }

  it('passes when minisign verifies each file with the public key and says its trusted comment is the expected one', async () => {
    const { folder, publicKey } = await signedFolder({ 'floti-3.0.0-linux-x64': signatureText('ED'), 'Floti-3.0.0-setup.exe': signatureText('ED') })
    const calls: string[][] = []
    const files = [join(folder, 'floti-3.0.0-linux-x64'), join(folder, 'Floti-3.0.0-setup.exe')]
    const problems = verifyProblems({
      files,
      comment: 'floti 3.0.0 {name}',
      publicKey,
      run: (args) => {
        calls.push(args)
        return ok(`floti 3.0.0 ${args[5] === files[0] ? 'floti-3.0.0-linux-x64' : 'Floti-3.0.0-setup.exe'}\n`)
      }
    })
    expect(problems).toEqual([])
    expect(calls).toEqual(files.map((file) => ['-V', '-Q', '-p', publicKey, '-m', file, '-x', `${file}.minisig`]))
  })

  it('names each file whose signature is missing, legacy, not a signature, does not verify or carries another comment', async () => {
    const { folder, publicKey } = await signedFolder({
      legacy: signatureText('Ed'),
      garbage: 'not a signature',
      forged: signatureText('ED'),
      moved: signatureText('ED')
    })
    writeFileSync(join(folder, 'unsigned'), 'x')
    const run = vi.fn((args: string[]): MinisignResult =>
      args[5]!.endsWith('forged')
        ? { status: 1, stdout: '', stderr: 'Signature verification failed\n' }
        : ok('floti 3.0.0 Floti-3.0.0-setup.exe\n')
    )
    const problems = verifyProblems({
      files: ['unsigned', 'legacy', 'garbage', 'forged', 'moved'].map((name) => join(folder, name)),
      comment: 'floti 3.0.0 {name}',
      publicKey,
      run
    })
    expect(problems).toEqual([
      'unsigned has no signature beside it (unsigned.minisig)',
      'legacy.minisig is not prehashed (it names "Ed", not ED)',
      'garbage.minisig is not a minisign signature',
      `forged does not verify with ${publicKey}: Signature verification failed`,
      'moved.minisig carries the trusted comment "floti 3.0.0 Floti-3.0.0-setup.exe", not "floti 3.0.0 moved"'
    ])
    // Only the two prehashed signatures reach minisign.
    expect(run).toHaveBeenCalledTimes(2)
  })

  it('refuses outright when the public key is missing or empty, or there is nothing to verify', async () => {
    const { folder, publicKey } = await signedFolder({ f: signatureText('ED') })
    const run = vi.fn(() => ok())
    const files = [join(folder, 'f')]
    expect(() => verifyProblems({ files, comment: 'c', publicKey: join(folder, 'absent.pub'), run })).toThrow(
      /absent\.pub is missing or empty, so no signature can be checked against it/
    )
    writeFileSync(publicKey, '\n')
    expect(() => verifyProblems({ files, comment: 'c', publicKey, run })).toThrow(/minisign\.pub is missing or empty/)
    expect(() => verifyProblems({ files: [], comment: 'c', publicKey, run })).toThrow('no files to verify')
    expect(run).not.toHaveBeenCalled()
  })
})

describe('running minisign', () => {
  it('gives it no input and no MINISIGN_SECRET_KEY', () => {
    vi.stubEnv('MINISIGN', process.execPath)
    vi.stubEnv('MINISIGN_SECRET_KEY', SECRET)
    const result = runMinisign([
      '-e',
      'let input = ""; process.stdin.on("data", (d) => { input += d }).on("end", () => console.log(JSON.stringify({ key: process.env.MINISIGN_SECRET_KEY ?? null, input })))'
    ])
    expect(result.status).toBe(0)
    expect(JSON.parse(result.stdout)).toEqual({ key: null, input: '' })
  })
})
