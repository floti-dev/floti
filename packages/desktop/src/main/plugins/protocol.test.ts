import { describe, expect, it, vi } from 'vitest'
import type { PluginPageSource, PluginRuntime } from './protocol'

vi.mock('electron', async () => (await import('../../../test/electron')).electronFake())

const { injectRuntime, PLUGIN_CSP, registerHostPluginProtocol, servePluginRequestFrom } = await import('./protocol')
const { protocol } = await import('electron')

/**
 * `floti-plugin://<id>/`: a plugin's page files served under its own origin
 * from the host that keeps it, every response under the plugin CSP, and Floti's
 * runtime injected at the top of every page and served under `/__floti/`.
 */

const runtime: PluginRuntime = {
  bridge: () => 'window.__bridge = 1',
  css: () => ':root { --floti: 1 }',
  boot: (plugin) => JSON.stringify({ theme: { kind: 'dark' }, plugin, quote: "it's <b>&" })
}

describe('injectRuntime', () => {
  const boot = '{"a":"it\'s <b> & co"}'

  it('puts both tags straight after <head>, with any attributes it has', () => {
    const html = injectRuntime('<html><HEAD lang="en"><title>t</title></HEAD></html>', boot)
    expect(html).toMatch(/^<html><HEAD lang="en"><link rel="stylesheet" href="\/__floti\/floti\.css"><script src="\/__floti\/bridge\.js"/)
  })

  it('makes a head for a page with none', () => {
    expect(injectRuntime('<html><body>x</body></html>', '{}')).toBe(
      `<html><head><link rel="stylesheet" href="/__floti/floti.css"><script src="/__floti/bridge.js" data-floti-boot='{}'></script></head><body>x</body></html>`
    )
  })

  it('keeps a bare page out of quirks mode by going after its doctype', () => {
    expect(injectRuntime('<!DOCTYPE html><p>x</p>', '{}')).toMatch(/^<!DOCTYPE html><link rel="stylesheet"/)
    expect(injectRuntime('<p>x</p>', '{}')).toMatch(/^<link rel="stylesheet".*<p>x<\/p>$/)
  })

  it('escapes the boot JSON so it cannot close its attribute or open a tag', () => {
    const html = injectRuntime('<head></head>', boot)
    const attribute = /data-floti-boot='([^']*)'/.exec(html)?.[1]
    expect(attribute).toBe('{"a":"it&#39;s &lt;b> &amp; co"}')
    // What the browser hands the bridge, once it has decoded the attribute.
    const decoded = attribute!.replace(/&#39;/g, "'").replace(/&lt;/g, '<').replace(/&amp;/g, '&')
    expect(JSON.parse(decoded)).toEqual({ a: "it's <b> & co" })
  })
})

describe('servePluginRequestFrom: pages from the local host’s files', () => {
  const text = (body: string): Uint8Array => new TextEncoder().encode(body)
  /** The host, as the scheme meets it: one plugin on, its files by path, and every path it was asked for. */
  const host = (onHostComputer = true): PluginPageSource & { asked: string[] } => {
    const asked: string[] = []
    return {
      asked,
      onHostComputer,
      serves: (id) => Promise.resolve(id === 'sample'),
      file: (id, path) => {
        asked.push(`${id}:${path}`)
        if (id !== 'sample') return Promise.resolve({ status: 404, contentType: 'text/plain', body: text('Not found') })
        if (path === 'dist/main page.html') {
          return Promise.resolve({ status: 200, contentType: 'text/html; charset=utf-8', body: text('<!doctype html><html><head><title>t</title></head><body>hi</body></html>') })
        }
        if (path === 'dist/assets/app.js') return Promise.resolve({ status: 200, contentType: 'text/javascript; charset=utf-8', body: text('console.log(1)') })
        if (path === 'escape/x.js') return Promise.resolve({ status: 403, contentType: 'text/plain', body: text('Forbidden') })
        return Promise.resolve({ status: 404, contentType: 'text/plain', body: text('Not found') })
      }
    }
  }

  it('renders a page the host served on the plugin’s origin, with the runtime injected and the same policy as today', async () => {
    const source = host()
    const response = await servePluginRequestFrom('floti-plugin://sample/dist/main%20page.html', source, runtime)
    expect(source.asked).toEqual(['sample:dist/main page.html'])
    expect(response.status).toBe(200)
    expect(response.headers.get('content-security-policy')).toBe(PLUGIN_CSP)
    expect(response.headers.get('x-content-type-options')).toBe('nosniff')
    expect(response.headers.get('cache-control')).toBe('no-store')
    expect(response.headers.get('content-type')).toBe('text/html; charset=utf-8')
    const html = await response.text()
    expect(html.indexOf('<head>')).toBeLessThan(html.indexOf('/__floti/floti.css'))
    expect(html).toContain('<script src="/__floti/bridge.js" data-floti-boot=')

    const js = await servePluginRequestFrom('floti-plugin://sample/dist/assets/app.js?v=2', source, runtime)
    expect(js.headers.get('content-security-policy')).toBe(PLUGIN_CSP)
    expect(await js.text()).toBe('console.log(1)')
  })

  const bootOf = (html: string): unknown => {
    const attribute = /data-floti-boot='([^']*)'/.exec(html)?.[1] ?? ''
    return JSON.parse(attribute.replace(/&#39;/g, "'").replace(/&lt;/g, '<').replace(/&amp;/g, '&'))
  }

  it('tells a page drawn from the host’s files that its channels are relayed through the host', async () => {
    const hosted = await servePluginRequestFrom('floti-plugin://sample/dist/main%20page.html', host(), runtime)
    expect(bootOf(await hosted.text())).toEqual({
      theme: { kind: 'dark' },
      plugin: 'sample',
      quote: "it's <b>&",
      channels: 'host',
      onHostComputer: true
    })
  })

  it('tells a page drawn from a host on another computer that it is not on the host’s computer', async () => {
    const remote = await servePluginRequestFrom('floti-plugin://sample/dist/main%20page.html', host(false), runtime)
    expect(bootOf(await remote.text())).toMatchObject({ channels: 'host', onHostComputer: false })
  })

  it('serves the runtime only on the origin of a plugin the host has on', async () => {
    const source = host()
    const bridge = await servePluginRequestFrom('floti-plugin://sample/__floti/bridge.js', source, runtime)
    expect(await bridge.text()).toBe('window.__bridge = 1')
    expect(bridge.headers.get('content-security-policy')).toBe(PLUGIN_CSP)
    expect((await servePluginRequestFrom('floti-plugin://other/__floti/bridge.js', source, runtime)).status).toBe(404)
    expect(source.asked).toEqual([])
  })

  it('passes on the host’s refusals as they are', async () => {
    const source = host()
    expect((await servePluginRequestFrom('floti-plugin://sample/escape/x.js', source, runtime)).status).toBe(403)
    expect((await servePluginRequestFrom('floti-plugin://sample/dist/gone.js', source, runtime)).status).toBe(404)
    expect((await servePluginRequestFrom('floti-plugin://other/dist/main%20page.html', source, runtime)).status).toBe(404)
    expect((await servePluginRequestFrom('floti-plugin://sample/', source, runtime)).status).toBe(404)
  })

  it('says why when the host cannot be reached, in the page’s own place', async () => {
    const sentence = 'Floti could not reach the local host; turn it off.'
    const source: PluginPageSource = {
      onHostComputer: true,
      serves: () => Promise.reject(new Error(sentence)),
      file: () => Promise.reject(new Error(sentence))
    }
    const page = await servePluginRequestFrom('floti-plugin://sample/dist/main%20page.html', source, runtime)
    expect(page.status).toBe(503)
    expect(await page.text()).toBe(sentence)
    expect((await servePluginRequestFrom('floti-plugin://sample/__floti/bridge.js', source, runtime)).status).toBe(503)
  })

  it('serves no other host’s origin from the local host alone', async () => {
    const source = host()
    expect((await servePluginRequestFrom('floti-plugin://sample.build-box/dist/main%20page.html', source, runtime)).status).toBe(404)
    expect(source.asked).toEqual([])
  })

  it('s22: serves build-box’s plugin pages from build-box’s files only, This PC’s from This PC’s, and nothing for a host not added', async () => {
    const ours = host()
    const theirs = host(false)
    const sources = (label: string | null): PluginPageSource | null =>
      label === null ? ours : label === 'build-box' ? theirs : null

    const remote = await servePluginRequestFrom('floti-plugin://sample.build-box/dist/main%20page.html', sources, runtime)
    expect(remote.status).toBe(200)
    expect(remote.headers.get('content-security-policy')).toBe(PLUGIN_CSP)
    // Drawn from build-box's files: not the computer the plugin runs on.
    expect(bootOf(await remote.text())).toMatchObject({ channels: 'host', onHostComputer: false })
    expect(theirs.asked).toEqual(['sample:dist/main page.html'])
    expect(ours.asked).toEqual([])

    await servePluginRequestFrom('floti-plugin://sample/dist/assets/app.js', sources, runtime)
    expect(ours.asked).toEqual(['sample:dist/assets/app.js'])
    expect(theirs.asked).toEqual(['sample:dist/main page.html'])

    expect((await servePluginRequestFrom('floti-plugin://sample.lab/dist/main%20page.html', sources, runtime)).status).toBe(404)
    expect((await servePluginRequestFrom('floti-plugin://sample.lab/__floti/bridge.js', sources, runtime)).status).toBe(404)
  })
})

describe('registerHostPluginProtocol', () => {
  it('s1: answers floti-plugin, and only it, with the handler', async () => {
    const handle = vi.spyOn(protocol, 'handle').mockImplementation(() => undefined)
    const source: PluginPageSource = {
      onHostComputer: true,
      serves: () => Promise.resolve(true),
      file: () => Promise.resolve({ status: 200, contentType: 'text/html', body: new TextEncoder().encode('<head></head>') })
    }
    registerHostPluginProtocol(() => source, runtime)
    expect(handle.mock.calls.map(([scheme]) => scheme)).toEqual(['floti-plugin'])
    const handler = handle.mock.calls[0]![1] as (request: Request) => Promise<Response>
    const response = await handler(new Request('floti-plugin://sample/p.html'))
    expect(response.status).toBe(200)
    expect(await response.text()).toContain('<script src="/__floti/bridge.js" data-floti-boot=')
    handle.mockRestore()
  })
})
