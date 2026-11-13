import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { plantShim, type PlantedShim } from '@helm/core/overlay-testkit'
import {
  claimShimFor,
  exitedPid,
  isLink,
  plantWorkspace,
  removeShims,
  runningProcess,
  shimOwners,
  type PlantedWorkspace
} from '../../test/overlay-world'
import { createWorld, disposeWorld, type World } from '../../test/world'
import type { Services } from './services'

vi.mock('electron', async () => (await import('../../test/electron')).electronFake())

/**
 * What `createServices` - app start - does to the data directory before any
 * window exists: sweeping overlay shims that nothing holds any more. The
 * templates folder is the local host's to seed.
 */
describe('app start', () => {
  let world: World
  let workspace: PlantedWorkspace
  let shimRoot: string
  let createServices: () => Services

  beforeAll(async () => {
    world = createWorld()
    Object.assign(process.env, { PORTABLE_EXECUTABLE_DIR: world.portableDir, USERPROFILE: world.home, HOME: world.home })
    ;({ createServices } = await import('./services'))
    workspace = plantWorkspace(world)
    // Where the app keeps these, by its own rule: under its data directory.
    shimRoot = join(world.dataDir, 'overlays')
  })

  afterEach(() => removeShims(shimRoot))

  afterAll(() => {
    removeShims(shimRoot)
    disposeWorld(world)
  })

  /** Starts the app's services once and closes the database again. */
  const start = (): Services => {
    const services = createServices()
    services.store.close()
    return services
  }

  /** The shim an earlier version of Helm built for a session composing the workspace's overlay. */
  const buildShim = (): PlantedShim => {
    const shim = plantShim(workspace.overlay, shimRoot)
    expect(isLink(join(shim.dir, 'skills'))).toBe(true)
    return shim
  }

  const skillThroughShim = (shim: PlantedShim): string =>
    readFileSync(join(shim.dir, 'skills', workspace.skill, 'SKILL.md'), 'utf8')

  it('counts the upgrade note seen in a data folder this start made, so a new device never shows it', () => {
    // Whichever test starts first makes the database; the stamp it gets then is kept.
    expect(start().settings.upgradeNoteSeenAt).not.toBeNull()
  })

  it('sweeps a shim whose owner has exited, and leaves the repository it linked', () => {
    const shim = buildShim()
    const gone = exitedPid()
    claimShimFor(shim.dir, gone)
    expect(shimOwners(shim.dir)).toEqual([gone])

    const services = start()

    expect(services.staleShims).toBe(1)
    expect(readdirSync(shimRoot).filter((name) => name.startsWith('overlay-'))).toEqual([])
    expect(readFileSync(join(workspace.overlay, '.claude', 'skills', workspace.skill, 'SKILL.md'), 'utf8')).toContain(
      '# think'
    )
    expect(existsSync(join(workspace.overlay, '.claude', 'agents', `${workspace.agent}.md`))).toBe(true)
  })

  it('leaves the live shim of another Helm on the same data directory, junctions and all', () => {
    const shim = buildShim()
    const other = runningProcess()
    try {
      claimShimFor(shim.dir, other.pid)

      const services = start()

      expect(services.staleShims).toBe(0)
      expect(shimOwners(shim.dir)).toEqual([other.pid])
      expect(isLink(join(shim.dir, 'skills'))).toBe(true)
      expect(skillThroughShim(shim)).toContain('# think')
    } finally {
      other.stop()
    }
  })
})
