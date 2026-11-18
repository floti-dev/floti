import { describe, expect, it } from 'vitest'
import type { OwnLogTail } from '../../../shared/ipc'
import { localHost, logForAgent, remoteHost, statusForAgent, type DesktopDiagnosisWindow } from './desktopDiagnosis'
import type { DesktopCall } from './host/desktopTools'

const call = (tool: string, args: unknown = {}): DesktopCall => ({ host: 'local', session: 4, tool, arguments: args })

const parts = (result: { content: { text: string }[] }): { text: string; json: Record<string, unknown> } => ({
  text: result.content[0]?.text ?? '',
  json: JSON.parse(result.content[1]?.text ?? 'null') as Record<string, unknown>
})

function window(overrides: Partial<DesktopDiagnosisWindow> = {}): DesktopDiagnosisWindow & { asked: [number, string | undefined][] } {
  const asked: [number, string | undefined][] = []
  return {
    asked,
    version: () => '2.4.0',
    hosts: () => [
      localHost('local', 'This PC', { step: 'ready', since: 1 }, null, '2.4.0'),
      remoteHost('box', 'build box', { state: 'failed', lastLine: 'ssh: connect to host box port 22: Connection refused' })
    ],
    mainLog: (lines, word): Promise<OwnLogTail> => {
      asked.push([lines, word])
      return Promise.resolve({ path: 'C:\\logs dir\\main.log', lines: lines === 0 ? [] : ['main started', 'plugin page shown'] })
    },
    ...overrides
  }
}

describe('desktop_status in the window', () => {
  it('answers the desktop version, each host with its state and last error, and where main.log is', async () => {
    const asking = window()
    const { text, json } = parts(await statusForAgent(call('desktop_status'), asking))
    expect(text).toContain("Floti desktop 2.4.0; main's log is C:\\logs dir\\main.log.")
    expect(text).toContain('This PC (local): connected, floti 2.4.0')
    expect(text).toContain('build box (box): failed: ssh: connect to host box port 22: Connection refused')
    expect(json).toEqual({
      desktop: { version: '2.4.0' },
      mainLog: 'C:\\logs dir\\main.log',
      hosts: [
        { id: 'local', name: 'This PC', state: 'connected', version: '2.4.0' },
        { id: 'box', name: 'build box', state: 'failed', lastError: 'ssh: connect to host box port 22: Connection refused' }
      ]
    })
    // The path is asked for without reading a line.
    expect(asking.asked).toEqual([[0, undefined]])
  })

  it('says when main could not say where its log is, and still answers the rest', async () => {
    const { text, json } = parts(
      await statusForAgent(call('desktop_status'), window({ mainLog: () => Promise.reject(new Error('main is busy')) }))
    )
    expect(text).toContain("main's log is not known: main is busy.")
    expect(json['mainLogProblem']).toBe('main is busy')
    expect(json['hosts']).toHaveLength(2)
  })
})

describe('the hosts as the window knows them', () => {
  it('says how this PC host stands from where main’s link is', () => {
    expect(localHost('local', 'This PC', null, null, null)).toEqual({ id: 'local', name: 'This PC', state: 'unknown' })
    expect(localHost('local', 'This PC', { step: 'connecting', since: 1 }, null, null).state).toBe('connecting')
    expect(localHost('local', 'This PC', { step: 'unreachable', since: 1, reason: 'no floti' }, null, null)).toMatchObject({
      state: 'unreachable',
      lastError: 'no floti'
    })
    expect(localHost('local', 'This PC', { step: 'ready', since: 1 }, { restarts: true }, '2.4.0')).toMatchObject({
      state: 'lost',
      lastError: 'The host went away; Floti is starting a new one.'
    })
  })

  it('says how each host on another computer stands, with its last error', () => {
    expect(remoteHost('a', 'A', null).state).toBe('unknown')
    expect(remoteHost('a', 'A', { state: 'connected', version: '2.3.0' })).toEqual({ id: 'a', name: 'A', state: 'connected', version: '2.3.0' })
    expect(remoteHost('a', 'A', { state: 'missing', lastLine: 'floti: not found' }).lastError).toBe('floti: not found')
    expect(remoteHost('a', 'A', { state: 'keyChanged' }).lastError).toContain('another key')
    expect(remoteHost('a', 'A', { state: 'tooOld', host: '4.1', client: '4.3' }).lastError).toBe(
      "Its floti speaks protocol 4.1, older than this desktop's 4.3."
    )
    expect(remoteHost('a', 'A', { state: 'retrying' })).toEqual({ id: 'a', name: 'A', state: 'retrying' })
  })
})

describe('desktop_log in the window', () => {
  it('answers main’s latest lines as the host asked for them', async () => {
    const asking = window()
    const { text, json } = parts(await logForAgent(call('desktop_log', { lines: 5, word: ' plugin ' }), asking))
    expect(asking.asked).toEqual([[5, 'plugin']])
    expect(text).toBe('main.log (C:\\logs dir\\main.log), its last 2 lines holding "plugin":\nmain started\nplugin page shown')
    expect(json).toEqual({ path: 'C:\\logs dir\\main.log', lines: ['main started', 'plugin page shown'] })
  })

  it('says when there is no log yet, or no line holding the word', async () => {
    const none = window({ mainLog: () => Promise.resolve({ path: 'm', lines: null }) })
    expect(parts(await logForAgent(call('desktop_log'), none)).text).toBe('main.log (m) is not there yet.')
    const empty = window({ mainLog: () => Promise.resolve({ path: 'm', lines: [] }) })
    expect(parts(await logForAgent(call('desktop_log', { word: 'x' }), empty)).text).toBe('main.log (m) has no lines holding "x".')
    // Arguments of the wrong shape fall back to 100 lines and no word.
    const asking = window()
    await logForAgent(call('desktop_log', { lines: '9', word: 3 }), asking)
    expect(asking.asked).toEqual([[100, undefined]])
  })
})
