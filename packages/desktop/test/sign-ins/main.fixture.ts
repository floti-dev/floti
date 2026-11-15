import { createServer, type IncomingMessage, type ServerResponse } from 'node:http'
import type { AddressInfo } from 'node:net'
import { writeFileSync } from 'node:fs'
import { app, BrowserWindow, session, type Cookie, type Session } from 'electron'
import { BROWSER_PARTITION_NAME, type CookieRole, type FirstRun, type SecondRun } from './report'
import { flushCookies, keepSignIns } from '../../src/main/browserCookies'

/**
 * Not a test: the Electron main script `browser-sign-ins.test.ts` bundles and
 * runs, twice, on one user-data folder under the run's root.
 *
 * - `first`: the browser partition gets the real `keepSignIns`; a hidden
 *   window loads a loopback page that sets three session cookies, waits until
 *   all three are kept, signs out of one, flushes the store and exits at once
 *   (`app.exit`, which skips Chromium's own orderly shutdown, so only the
 *   flush can have written them).
 * - `second`: reads the partition back.
 *
 * What it reports names each cookie by its role and says only whether its
 * value is the one the page set: no cookie's name or value leaves this file.
 * No window is shown, and any failure is written to the report and ends the
 * process rather than raising Electron's error dialog.
 */

const MODE = process.env['SIGN_INS_MODE']
const RESULT = process.env['SIGN_INS_RESULT'] ?? ''

/** The cookies the page sets, by role. */
const COOKIES: Record<CookieRole, { name: string; value: string; attributes: string }> = {
  kept: { name: 'probe-session', value: 'signed-in', attributes: 'Path=/; HttpOnly; SameSite=Lax' },
  hostPrefixed: { name: '__Host-probe', value: 'host-only', attributes: 'Path=/; Secure' },
  signedOut: { name: 'probe-signed-out', value: 'until-sign-out', attributes: 'Path=/' }
}
const ROLES = Object.keys(COOKIES) as CookieRole[]

const report = (body: FirstRun | SecondRun | { error: string }): void => {
  writeFileSync(RESULT, JSON.stringify(body))
}

const fail = (error: unknown): void => {
  report({ error: error instanceof Error ? (error.stack ?? error.message) : String(error) })
  app.exit(1)
}
process.on('uncaughtException', fail)
process.on('unhandledRejection', fail)

const userData = app.commandLine.getSwitchValue('user-data-dir')
if (userData === '') {
  fail(new Error('started without --user-data-dir'))
} else {
  app.setPath('userData', userData)
}
app.disableHardwareAcceleration()

/** Resolves once `ready` holds, checking again on every cookie change. */
function until(partition: Session, ready: (cookies: Cookie[]) => boolean): Promise<void> {
  return new Promise((resolve, reject) => {
    const check = (): void => {
      partition.cookies.get({}).then((cookies) => {
        if (!ready(cookies)) return
        partition.cookies.off('changed', check)
        resolve()
      }, reject)
    }
    partition.cookies.on('changed', check)
    check()
  })
}

const byName = (cookies: Cookie[], role: CookieRole): Cookie | undefined =>
  cookies.find((cookie) => cookie.name === COOKIES[role].name)

async function first(partition: Session): Promise<void> {
  keepSignIns(partition, app.getName())
  const seen: { userAgent: string; clientHints: string } = { userAgent: '', clientHints: '' }
  const server = createServer((request: IncomingMessage, response: ServerResponse) => {
    if (request.url === '/sign-in') {
      seen.userAgent = request.headers['user-agent'] ?? ''
      seen.clientHints = String(request.headers['sec-ch-ua'] ?? '')
      response.setHeader(
        'Set-Cookie',
        ROLES.map((role) => `${COOKIES[role].name}=${COOKIES[role].value}; ${COOKIES[role].attributes}`)
      )
    } else if (request.url === '/sign-out') {
      response.setHeader('Set-Cookie', `${COOKIES.signedOut.name}=; Path=/; Max-Age=0`)
    }
    response.setHeader('Content-Type', 'text/html')
    response.end('<!doctype html><title>probe</title>')
  })
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  const origin = `http://localhost:${String((server.address() as AddressInfo).port)}`

  const page = new BrowserWindow({
    show: false,
    webPreferences: { session: partition, sandbox: true, contextIsolation: true, nodeIntegration: false }
  })
  await page.webContents.loadURL(`${origin}/sign-in`)
  const inPage = (await page.webContents.executeJavaScript(
    'JSON.stringify({ userAgent: navigator.userAgent, brands: navigator.userAgentData ? navigator.userAgentData.brands : null })'
  )) as string
  await until(partition, (cookies) =>
    ROLES.every((role) => {
      const cookie = byName(cookies, role)
      return cookie !== undefined && cookie.session === false
    })
  )
  await page.webContents.loadURL(`${origin}/sign-out`)
  await until(partition, (cookies) => byName(cookies, 'signedOut') === undefined)
  await flushCookies(partition.cookies)

  const { userAgent, brands } = JSON.parse(inPage) as { userAgent: string; brands: FirstRun['brands'] }
  report({
    appName: app.getName(),
    defaultAgentHadElectron: /\bElectron\//.test(session.defaultSession.getUserAgent()),
    pageAgent: userAgent,
    headerAgent: seen.userAgent,
    clientHints: seen.clientHints,
    brands,
    chromeVersion: process.versions.chrome
  })
  server.close()
  app.exit(0)
}

async function second(partition: Session): Promise<void> {
  const cookies = await partition.cookies.get({})
  const nowSeconds = Date.now() / 1000
  const found = Object.fromEntries(
    ROLES.map((role) => {
      const cookie = byName(cookies, role)
      return [
        role,
        cookie === undefined
          ? { present: false }
          : {
              present: true,
              valueMatches: cookie.value === COOKIES[role].value,
              session: cookie.session === true,
              daysLeft: cookie.expirationDate === undefined ? null : (cookie.expirationDate - nowSeconds) / 86_400,
              hostOnly: cookie.hostOnly === true,
              secure: cookie.secure === true,
              httpOnly: cookie.httpOnly === true,
              sameSite: cookie.sameSite
            }
      ]
    })
  ) as SecondRun['cookies']
  report({ cookies: found })
  app.exit(0)
}

app
  .whenReady()
  .then(() => {
    const partition = session.fromPartition(BROWSER_PARTITION_NAME)
    if (MODE === 'first') return first(partition)
    if (MODE === 'second') return second(partition)
    throw new Error(`unknown mode ${String(MODE)}`)
  })
  .catch(fail)
