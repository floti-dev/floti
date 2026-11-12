// Types for verify-linux-artifact.mjs, so its checks of an extracted AppImage can be tested from TypeScript.

export const APPIMAGE_NAME: RegExp
export const UNPACKED: readonly { path: string; elf: boolean }[]
export const RESOURCES: readonly string[]
export const GLIBC_FLOOR: string
export function glibcNeeded(bytes: Uint8Array): string | null
export function glibcByFile(root: string): { path: string; needed: string }[]
export function appProblems(
  root: string,
  options: { floti: Uint8Array; sums: Uint8Array; origin: string; modes?: boolean }
): string[]
