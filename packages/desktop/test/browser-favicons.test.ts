import { mkdirSync, mkdtempSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { afterEach, describe, expect, it } from 'vitest'
import { PATIENCE } from '../../../test/patience'
import { bundleFixture, endFixtures, reportOf, startFixture } from './electron-fixture'
import type { FaviconsReport } from './favicons/report'

/**
 * Icons fetched over Electron's own network stack, in real Electron.
 *
 * What a fake cannot show is how Electron's `net.request` behaves at a
 * redirect with `redirect: 'manual'`, and that behaviour is the whole of the
 * reach check on a redirect hop. So the fixture (`favicons/main.fixture.ts`)
 * runs the shipped `main/favicons.ts` on the Electron binary from
 * `node_modules` (`electron-fixture.ts`), hidden, with a user-data folder
 * under the run's root, against loopback servers, and reports what each
 * server was asked for.
 */

const HERE = dirname(fileURLToPath(import.meta.url))

afterEach(endFixtures)

/** Runs the fixture to its end, owned by this process, and returns its report. */
async function run(root: string, userData: string): Promise<FaviconsReport> {
  const app = await bundleFixture(root, join(HERE, 'favicons', 'main.fixture.ts'), 'favicons-fixture', 'Favicons Fixture')
  const result = join(root, 'favicons.json')
  const env = { FAVICONS_RESULT: result, FAVICONS_OWNER: String(process.pid) }
  return reportOf<FaviconsReport>(startFixture(app, userData, env, 'fixture'), result)
}

describe('favicons over Electronâ€™s network, in real Electron', () => {
  it(
    'follows a redirect to an address in reach, and never asks for one out of it',
    async () => {
      const root = mkdtempSync(join(tmpdir(), 'favicons '))
      const userData = join(root, 'user data')
      mkdirSync(userData)
      const report = await run(root, userData)

      expect(report.direct).toBe(report.expected)
      // The hop was checked and followed: the second server was asked.
      expect(report.redirected).toBe(report.expected)
      expect(report.alsoAsked).toEqual(['/icon.png'])
      // The hop out of reach was never asked for.
      expect(report.refused).toBeNull()
      expect(report.farAsked).toEqual([])
      expect(report.notImage).toBeNull()
      expect(report.nearAsked).toEqual(['/icon.png', '/favicon.ico', '/away.ico', '/page.html'])
    },
    // One Electron start, bounded by the patience, and a bundle.
    2 * PATIENCE
  )
})
