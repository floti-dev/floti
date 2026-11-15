import { createServer } from 'node:http'
import type { AddressInfo } from 'node:net'
import { writeFileSync } from 'node:fs'
import { app, net, session } from 'electron'
import { PATIENCE } from '../../../../test/patience'
import { createFavicons } from '../../src/main/favicons'
import type { FaviconsReport } from './report'

/**
 * Not a test: the Electron main script `browser-favicons.test.ts` bundles and
 * runs. It fetches icons with the shipped `main/favicons.ts` over Electron's
 * own `net.request` on a partition, from three loopback servers: `near` and
 * `also` are in reach, `far` is not. It reports what each fetch answered and
 * which paths each server was asked for, then exits.
 *
 * It ends with the test that started it, once the test's process is gone,
 * and in any case after `PATIENCE`. No window is opened, and any failure is
 * written to the report rather than raised as Electron's error dialog.
 */

const RESULT = process.env['FAVICONS_RESULT'] ?? ''
const PNG = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])

const report = (body: FaviconsReport | { error: string } | { ended: 'owner' }): void => {
  writeFileSync(RESULT, JSON.stringify(body))
}
const fail = (error: unknown): void => {
  report({ error: error instanceof Error ? (error.stack ?? error.message) : String(error) })
  app.exit(1)
}
process.on('uncaughtException', fail)
process.on('unhandledRejection', fail)

// Ends with its test, as the sign-ins fixture does: signal 0 sends nothing,
// and EPERM means the owner runs as somebody else.
const owner = Number(process.env['FAVICONS_OWNER'])
if (!Number.isSafeInteger(owner) || owner <= 0) fail(new Error('started without FAVICONS_OWNER'))
setInterval(() => {
  try {
    process.kill(owner, 0)
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'EPERM') return
    report({ ended: 'owner' })
    app.exit(1)
  }
}, 250)
setTimeout(() => fail(new Error(`still running after ${String(PATIENCE)} ms`)), PATIENCE)

const userData = app.commandLine.getSwitchValue('user-data-dir')
if (userData === '') fail(new Error('started without --user-data-dir'))
else app.setPath('userData', userData)
app.disableHardwareAcceleration()

/** A loopback server answering `routes`, recording every path it was asked for. */
async function serve(
  routes: Record<string, (send: (status: number, headers: Record<string, string>, body?: Buffer) => void) => void>
): Promise<{ origin: string; asked: string[] }> {
  const asked: string[] = []
  const server = createServer((request, response) => {
    const path = request.url ?? '/'
    asked.push(path)
    const route = routes[path]
    if (route === undefined) {
      response.writeHead(404).end()
      return
    }
    route((status, headers, body) => response.writeHead(status, headers).end(body))
  })
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  return { origin: `http://127.0.0.1:${String((server.address() as AddressInfo).port)}`, asked }
}

app.on('window-all-closed', () => undefined)

void app.whenReady().then(async () => {
  const far = await serve({
    '/icon.png': (send) => send(200, { 'Content-Type': 'image/png' }, PNG)
  })
  const also = await serve({
    '/icon.png': (send) => send(200, { 'Content-Type': 'image/png' }, PNG)
  })
  const near = await serve({
    '/icon.png': (send) => send(200, { 'Content-Type': 'image/png' }, PNG),
    '/favicon.ico': (send) => send(302, { Location: `${also.origin}/icon.png` }),
    '/away.ico': (send) => send(302, { Location: `${far.origin}/icon.png` }),
    '/page.html': (send) => send(200, { 'Content-Type': 'text/html' }, Buffer.from('<!doctype html>'))
  })

  const partition = session.fromPartition('persist:favicons-fixture')
  const favicons = createFavicons({
    request: (url) => net.request({ url, session: partition, redirect: 'manual', useSessionCookies: true })
  })
  const reach = new Set([near.origin, also.origin])
  const allows = (url: string): boolean => reach.has(new URL(url).origin)

  const direct = await favicons.iconFor([`${near.origin}/icon.png`], allows)
  const redirected = await favicons.iconFor([`${near.origin}/favicon.ico`], allows)
  const refused = await favicons.iconFor([`${near.origin}/away.ico`], allows)
  const notImage = await favicons.iconFor([`${near.origin}/page.html`], allows)

  report({
    expected: `data:image/png;base64,${PNG.toString('base64')}`,
    direct,
    redirected,
    refused,
    notImage,
    nearAsked: near.asked,
    alsoAsked: also.asked,
    farAsked: far.asked
  })
  app.exit(0)
}, fail)
