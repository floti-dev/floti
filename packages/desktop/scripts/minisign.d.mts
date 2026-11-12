// Types for minisign.mjs, so signing and checking signatures can be tested from TypeScript.

export interface MinisignResult {
  status: number | null
  stdout: string
  stderr: string
}

export type MinisignRunner = (args: string[]) => MinisignResult

export const SECRET_KEY_VARIABLE: 'MINISIGN_SECRET_KEY'
export const PREHASHED: 'ED'

export function signatureFile(file: string): string
export function trustedComment(template: string, file: string): string
export function signatureAlgorithm(text: string): string | null
export function runMinisign(args: string[]): MinisignResult
export function withSecretKeyFile<T>(secretKey: string | undefined, tempRoot: string, use: (file: string) => T): T
export function signFiles(options: {
  files: string[]
  comment: string
  secretKey: string | undefined
  tempRoot: string
  run?: MinisignRunner
}): { file: string; signature: string; comment: string }[]
export function verifyProblems(options: { files: string[]; comment: string; publicKey: string; run?: MinisignRunner }): string[]
