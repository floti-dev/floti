// Signs files with minisign and checks their signatures, for
// .github/workflows/release.yml and any other file the project signs. The
// trusted comment is an argument, in which `{name}` stands for each file's
// name: `floti <version> {name}` for a release's files, `floti update.json`
// for the update manifest.
//
//   MINISIGN_SECRET_KEY=<the secret key file's text> \
//   node minisign.mjs sign --comment <comment> <file>...
//     writes <file>.minisig beside each file: prehashed (minisign's default
//     since 0.10), with exactly <comment> as its trusted comment. The key must
//     have no password (made with `minisign -G -W`). It is written to an
//     owner-only file in a folder of its own under $RUNNER_TEMP, and that
//     folder is removed before this returns, whether signing worked or not.
//     minisign never sees MINISIGN_SECRET_KEY in its environment.
//
//   node minisign.mjs verify --public-key <minisign.pub> --comment <comment> <file>...
//     refuses unless each file's <file>.minisig is prehashed, verifies with
//     that public key - minisign itself checks the signature and the trusted
//     comment's own signature - and carries exactly <comment>.
//
// minisign is the one on PATH, or $MINISIGN.

import { spawnSync } from 'node:child_process'
import { chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { basename, join, resolve } from 'node:path'
import { parseArgs } from 'node:util'
import { fileURLToPath } from 'node:url'

/** The variable `sign` reads the secret key from. It is never passed on to minisign. */
export const SECRET_KEY_VARIABLE = 'MINISIGN_SECRET_KEY'

/** The algorithm a prehashed signature names: Ed25519 over the file's BLAKE2b-512. */
export const PREHASHED = 'ED'

/** Where a file's signature goes: beside it, as `<file>.minisig`. */
export function signatureFile(file) {
  return `${file}.minisig`
}

/** `template` with `{name}` replaced by `file`'s name. Refuses one that is empty or not one line. */
export function trustedComment(template, file) {
  const comment = template.replaceAll('{name}', basename(file))
  if (comment.trim() === '' || /[\r\n]/.test(comment)) {
    throw new Error(`the trusted comment for ${basename(file)} must be one line of text, not ${JSON.stringify(comment)}`)
  }
  return comment
}

/**
 * The algorithm a signature file's text names: `ED` for a prehashed one,
 * `Ed` for a legacy one, or null when it is not a minisign signature.
 */
export function signatureAlgorithm(text) {
  const lines = text.split(/\r?\n/)
  if (!(lines[0] ?? '').startsWith('untrusted comment:')) return null
  const line = (lines[1] ?? '').trim()
  if (!/^[A-Za-z0-9+/]+=*$/.test(line)) return null
  // Two bytes of algorithm, eight of key id, sixty-four of signature.
  const bytes = Buffer.from(line, 'base64')
  if (bytes.length !== 74) return null
  return bytes.subarray(0, 2).toString('latin1')
}

/** Runs minisign with `args`, with no input and without the secret key variable. */
export function runMinisign(args) {
  const env = { ...process.env }
  delete env[SECRET_KEY_VARIABLE]
  const result = spawnSync(process.env.MINISIGN ?? 'minisign', args, {
    encoding: 'utf8',
    env,
    stdio: ['ignore', 'pipe', 'pipe'],
    // A key with a password would wait for one; with no input it fails instead.
    timeout: 60_000
  })
  if (result.error) throw new Error(`could not run minisign: ${result.error.message}`)
  return { status: result.status, stdout: result.stdout, stderr: result.stderr }
}

/**
 * Calls `use` with the path of an owner-only file holding `secretKey`, in a
 * folder of its own under `tempRoot`, and removes that folder before
 * returning or throwing.
 */
export function withSecretKeyFile(secretKey, tempRoot, use) {
  if (typeof secretKey !== 'string' || secretKey.trim() === '') {
    throw new Error(`${SECRET_KEY_VARIABLE} is empty or not set, so nothing can be signed`)
  }
  const folder = mkdtempSync(join(tempRoot, 'minisign-'))
  try {
    const file = join(folder, 'minisign.key')
    const text = secretKey.replace(/\r\n/g, '\n')
    writeFileSync(file, text.endsWith('\n') ? text : `${text}\n`, { mode: 0o600, flag: 'wx' })
    // `mode` is filtered through the umask; this is not.
    chmodSync(file, 0o600)
    return use(file)
  } finally {
    rmSync(folder, { recursive: true, force: true })
  }
}

function missing(files) {
  const absent = files.filter((file) => !existsSync(file) || !statSync(file).isFile())
  if (absent.length > 0) throw new Error(`no such file: ${absent.join(', ')}`)
}

function said(result) {
  return (result.stderr || result.stdout || '').trim() || `exit ${String(result.status)}`
}

/**
 * Signs each file, writing `<file>.minisig` beside it with the trusted
 * comment `comment` gives it, and returns what it wrote.
 */
export function signFiles({ files, comment, secretKey, tempRoot, run = runMinisign }) {
  if (files.length === 0) throw new Error('no files to sign')
  missing(files)
  const comments = files.map((file) => trustedComment(comment, file))
  return withSecretKeyFile(secretKey, tempRoot, (keyFile) =>
    files.map((file, i) => {
      const signature = signatureFile(file)
      const result = run(['-S', '-s', keyFile, '-m', file, '-x', signature, '-t', comments[i]])
      if (result.status !== 0) throw new Error(`minisign could not sign ${basename(file)}: ${said(result)}`)
      return { file, signature, comment: comments[i] }
    })
  )
}

/**
 * Why each file's signature is not a prehashed one that verifies with the
 * public key in `publicKey` and carries exactly the comment `comment` gives
 * it: one line per file that fails, none when every one passes.
 */
export function verifyProblems({ files, comment, publicKey, run = runMinisign }) {
  if (files.length === 0) throw new Error('no files to verify')
  if (!existsSync(publicKey) || readFileSync(publicKey, 'utf8').trim() === '') {
    throw new Error(`${publicKey} is missing or empty, so no signature can be checked against it`)
  }
  missing(files)
  const problems = []
  for (const file of files) {
    const name = basename(file)
    const expected = trustedComment(comment, file)
    const signature = signatureFile(file)
    if (!existsSync(signature)) {
      problems.push(`${name} has no signature beside it (${basename(signature)})`)
      continue
    }
    const algorithm = signatureAlgorithm(readFileSync(signature, 'utf8'))
    if (algorithm === null) {
      problems.push(`${basename(signature)} is not a minisign signature`)
      continue
    }
    if (algorithm !== PREHASHED) {
      problems.push(`${basename(signature)} is not prehashed (it names ${JSON.stringify(algorithm)}, not ${PREHASHED})`)
      continue
    }
    // -Q prints the trusted comment alone, once both signatures verify.
    const result = run(['-V', '-Q', '-p', publicKey, '-m', file, '-x', signature])
    if (result.status !== 0) {
      problems.push(`${name} does not verify with ${publicKey}: ${said(result)}`)
      continue
    }
    const found = result.stdout.replace(/\r?\n$/, '')
    if (found !== expected) {
      problems.push(`${basename(signature)} carries the trusted comment ${JSON.stringify(found)}, not ${JSON.stringify(expected)}`)
    }
  }
  return problems
}

/** An error as the workflow shows it: an annotation in Actions, a plain line elsewhere. */
function report(message) {
  for (const line of message.split('\n')) console.error(process.env.GITHUB_ACTIONS === 'true' ? `::error::${line}` : line)
}

function main(argv) {
  const [command, ...rest] = argv
  const usage = 'usage: node minisign.mjs sign --comment <comment> <file>... | verify --public-key <file> --comment <comment> <file>...'
  const { values, positionals } = parseArgs({
    args: rest,
    options: { comment: { type: 'string' }, 'public-key': { type: 'string' } },
    allowPositionals: true
  })
  if (command === 'sign' && values.comment !== undefined && values['public-key'] === undefined) {
    const signed = signFiles({
      files: positionals,
      comment: values.comment,
      secretKey: process.env[SECRET_KEY_VARIABLE],
      tempRoot: process.env.RUNNER_TEMP ?? tmpdir()
    })
    for (const { signature, comment } of signed) console.log(`${basename(signature)}: "${comment}"`)
    return 0
  }
  if (command === 'verify' && values.comment !== undefined && values['public-key'] !== undefined) {
    const problems = verifyProblems({ files: positionals, comment: values.comment, publicKey: values['public-key'] })
    if (problems.length > 0) {
      report(problems.join('\n'))
      return 1
    }
    for (const file of positionals) console.log(`${basename(file)}: verified, "${trustedComment(values.comment, file)}"`)
    return 0
  }
  console.error(usage)
  return 2
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    process.exitCode = main(process.argv.slice(2))
  } catch (error) {
    report(error instanceof Error ? error.message : String(error))
    process.exitCode = 1
  }
}
