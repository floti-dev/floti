import { createServer, type IncomingMessage, type ServerResponse } from 'node:http'
import type { AddressInfo } from 'node:net'
import { writeFileSync } from 'node:fs'
import { app, BrowserWindow, session, type Cookie, type Session } from 'electron'
import { PATIENCE } from '../../../../test/patience'
import {
  BROWSER_PARTITION_NAME,
  type ChangeSeen,
  type CookieRole,
  type Ended,
  type FirstRun,
  type SecondRun
} from './report'
import { flushCookies, forgottenCookie, keepSignIns } from '../../src/main/browserCookies'
import { COOKIES } from './cookies'

/**
 * Not a test: the Electron main script `browser-sign-ins.test.ts` bundles and
 * runs on one user-data folder under the run's root.
 *
 * - `first`: the browser partition gets the real `keepSignIns`; a hidden
 *   window loads a loopback page that sets five session cookies, waits until
 *   all are kept, removes the deep twin exactly as the keeper undoes a
 *   rewrite (`forgottenCookie`), signs out of one, flushes the store and exits
 *   at once (`app.exit`, which skips Chromium's own orderly shutdown, so only
 *   the flush can have written them).
 * - `second`: reads the partition back.
 * - `linger`: waits, for the test that shows the process ends with its test.
 *
 * It ends with the test that started it: once the test's process is gone,
 * however that ends, and in any case after `PATIENCE`. What it reports names each cookie by its role and
 * says only whether its value is the one the page set: no cookie's name or
 * value (`cookies.ts`) is in its report or its output. No window is shown, and any failure is written to
 * the report and ends the process rather than raising Electron's error dialog.
 */

const MODE = process.env['SIGN_INS_MODE']
const RESULT = process.env['SIGN_INS_RESULT'] ?? ''

const ROLES = Object.keys(COOKIES) as CookieRole[]

const report = (body: FirstRun | SecondRun | Ended): void => {
  writeFileSync(RESULT, JSON.stringify(body))
}

const fail = (error: unknown): void => {
  report({ error: error instanceof Error ? (error.stack ?? error.message) : String(error) })
  app.exit(1)
}
process.on('uncaughtException', fail)
process.on('unhandledRejection', fail)

/*
 * Ends with its test: once the process `SIGN_INS_OWNER` names (the test's
 * worker) is gone, however it went, as a fake agent does with its owner
 * (`watchOwner` in test/fake-agents/common.mjs, polled as often). Signal 0
 * sends nothing; EPERM means the owner runs as somebody else.
 */
const OWNER_POLL_MS = 250
const owner = Number(process.env['SIGN_INS_OWNER'])
if (!Number.isSafeInteger(owner) || owner <= 0) fail(new Error('started without SIGN_INS_OWNER'))
const ownerGone = (): boolean => {
  try {
    process.kill(owner, 0)
    return false
  } catch (error) {
    return (error as NodeJS.ErrnoException).code !== 'EPERM'
  }
}
setInterval(() => {
  if (!ownerGone()) return
  report({ ended: 'owner' })
  app.exit(1)
}, OWNER_POLL_MS)
setTimeout(() => fail(new Error(`still running after ${String(PATIENCE)} ms`)), PATIENCE)

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

const isRole = (cookie: Cookie, role: CookieRole): boolean =>
  cookie.name === COOKIES[role].name && cookie.path === COOKIES[role].path

const byRole = (cookies: Cookie[], role: CookieRole): Cookie | undefined =>
  cookies.find((cookie) => isRole(cookie, role))

const setCookieHeader = (role: CookieRole): string => {
  const { name, value, path, attributes } = COOKIES[role]
  return `${name}=${value}; Path=${path}${attributes === '' ? '' : `; ${attributes}`}`
}

async function first(partition: Session): Promise<void> {
  const keptChanges: ChangeSeen[] = []
  partition.cookies.on('changed', (_event, cookie, cause, removed) => {
    if (isRole(cookie, 'kept')) keptChanges.push({ cause, removed, session: cookie.session === true })
  })
  keepSignIns(partition, app.getName())

  const seen: { userAgent: string; clientHints: string } = { userAgent: '', clientHints: '' }
  const server = createServer((request: IncomingMessage, response: ServerResponse) => {
    if (request.url === '/sign-in') {
      seen.userAgent = request.headers['user-agent'] ?? ''
      seen.clientHints = String(request.headers['sec-ch-ua'] ?? '')
      response.setHeader('Set-Cookie', ROLES.map(setCookieHeader))
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
      const cookie = byRole(cookies, role)
      return cookie !== undefined && cookie.session === false
    })
  )
  const deep = byRole(await partition.cookies.get({}), 'twinDeep')
  if (deep === undefined) throw new Error('the deep twin is gone before it was removed')
  await partition.cookies.set(forgottenCookie(deep))
  await page.webContents.loadURL(`${origin}/sign-out`)
  await until(partition, (cookies) => byRole(cookies, 'signedOut') === undefined && byRole(cookies, 'twinDeep') === undefined)
  await flushCookies(partition.cookies)

  const { userAgent, brands } = JSON.parse(inPage) as { userAgent: string; brands: FirstRun['brands'] }
  report({
    appName: app.getName(),
    defaultAgentHadElectron: /\bElectron\//.test(session.defaultSession.getUserAgent()),
    pageAgent: userAgent,
    headerAgent: seen.userAgent,
    clientHints: seen.clientHints,
    brands,
    chromeVersion: process.versions.chrome,
    keptChanges
  })
  server.close()
  app.exit(0)
}

async function second(partition: Session): Promise<void> {
  const cookies = await partition.cookies.get({})
  const nowSeconds = Date.now() / 1000
  const found = Object.fromEntries(
    ROLES.map((role) => {
      const cookie = byRole(cookies, role)
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
    if (MODE === 'linger') return undefined
    throw new Error(`unknown mode ${String(MODE)}`)
  })
  .catch(fail)
