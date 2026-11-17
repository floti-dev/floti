import { lstatSync, mkdirSync, readdirSync, rmdirSync, rmSync, unlinkSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import type { World } from './world'

/**
 * Profiles and overlay shims in a test world: a workspace with a repository worth
 * composing, and the removal of the shims a launch composed for it.
 *
 * A shim holds junctions into the world's own repositories, so anything that
 * makes one removes it with `removeShims` before the world is disposed.
 */

export interface PlantedWorkspace {
  /** The workspace root, inside the world's projects folder so its scan finds it. */
  root: string
  /** `repos/tools`, carrying a skill, an agent and a CLAUDE.md to compose. */
  overlay: string
  /** The overlay's skill directory name. */
  skill: string
  /** The overlay's agent name. */
  agent: string
}

function write(file: string, text: string): void {
  mkdirSync(dirname(file), { recursive: true })
  writeFileSync(file, text)
}

/** Where `plantWorkspace` puts its workspace, without writing anything. */
export function workspaceIn(world: World, name = 'hub'): PlantedWorkspace {
  const root = join(world.projectsDir, name)
  return { root, overlay: join(root, 'repos', 'tools'), skill: 'think', agent: 'reviewer' }
}

export function plantWorkspace(world: World, name = 'hub'): PlantedWorkspace {
  const planted = workspaceIn(world, name)
  const { root, overlay } = planted
  write(join(root, 'workspace.yaml'), `name: "${name}"\nversion: 1\n`)
  mkdirSync(join(root, '.claude'), { recursive: true })
  write(join(overlay, '.claude', 'skills', 'think', 'SKILL.md'), '---\nname: think\ndescription: Think it through.\n---\n# think\n')
  write(join(overlay, '.claude', 'agents', 'reviewer.md'), '---\nname: reviewer\ndescription: Reviews a change.\n---\nReview it.\n')
  write(join(overlay, 'CLAUDE.md'), '# tools\n\nThe tools repository.\n')
  return planted
}

/** Whether `path` is a link (a junction, on Windows) rather than a real directory. */
export function isLink(path: string): boolean {
  try {
    return lstatSync(path).isSymbolicLink()
  } catch {
    return false
  }
}

/**
 * Removes every shim under `shimRoot`, unlinking its junctions first so that
 * nothing recursive is ever handed a directory with a link in it.
 */
export function removeShims(shimRoot: string): void {
  let names: string[]
  try {
    names = readdirSync(shimRoot)
  } catch {
    return
  }
  for (const name of names) {
    const dir = join(shimRoot, name)
    if (!lstatSync(dir).isDirectory()) continue
    for (const entry of readdirSync(dir)) {
      const path = join(dir, entry)
      if (!isLink(path)) continue
      try {
        unlinkSync(path)
      } catch {
        rmdirSync(path)
      }
    }
  }
  rmSync(shimRoot, { recursive: true, force: true })
}
