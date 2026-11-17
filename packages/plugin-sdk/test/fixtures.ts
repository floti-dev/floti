import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

/** Manifests the tests share: the two that ship, and the smallest that loads. */

export const SDK_DIR = fileURLToPath(new URL('..', import.meta.url))

const readJson = (path: string): unknown => JSON.parse(readFileSync(path, 'utf8'))

/** The sample plugin's folder: a plugin with every surface, and what Floti's own tests drive. */
export const SAMPLE_DIR = join(SDK_DIR, '..', '..', 'examples', 'sample-plugin')

/** The sample plugin's manifest. */
export const SAMPLE = readJson(join(SAMPLE_DIR, 'floti-plugin.json'))

/** What `floti-plugin create` writes. */
export const TEMPLATE = readJson(join(SDK_DIR, 'template', 'floti-plugin.json'))

export const MINIMAL = { apiVersion: 2, id: 'minimal', name: 'Minimal' }
