import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest'
import { createWorld, disposeWorld, type World } from '../../test/world'
import type { Services } from './services'

vi.mock('electron', async () => (await import('../../test/electron')).electronFake())

/**
 * What `createServices` - app start - does to the data directory before any
 * window exists: the start that creates the database is a first start, and
 * puts this device in the guided first run.
 */
describe('app start', () => {
  let world: World
  let createServices: () => Services

  beforeAll(async () => {
    world = createWorld()
    Object.assign(process.env, { PORTABLE_EXECUTABLE_DIR: world.portableDir, USERPROFILE: world.home, HOME: world.home })
    ;({ createServices } = await import('./services'))
  })

  afterAll(() => disposeWorld(world))

  it('opens on the walkthrough in a data folder this start made', () => {
    const services = createServices()
    try {
      expect(services.settings.walkthrough).toEqual({ step: 1, workspace: null })
    } finally {
      services.store.close()
    }
  })
})
