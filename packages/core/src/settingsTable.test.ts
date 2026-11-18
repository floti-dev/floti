import { describe, expect, it } from 'vitest'
import { SETTING_VALIDATORS } from './store/settings'
import {
  DEFAULT_SETTINGS,
  describeSettingValues,
  findSettingEntry,
  settingEntry,
  settingHint,
  settingsTable,
  settingValueProblem,
  settingValueWords,
  STATE_KEYS,
  type SettingEntry,
  type SettingKey,
  type SettingValues
} from './types'

const ABSOLUTE = process.platform === 'win32' ? 'C:\\work\\a project' : '/work/a project'
const OTHER = process.platform === 'win32' ? 'D:\\other' : '/other'

/** Values a setting of this kind takes, and values it does not, for the validator to agree with. */
function samples(values: SettingValues): { good: unknown[]; bad: unknown[] } {
  const common = [undefined, {}, 'nonsense!']
  switch (values.kind) {
    case 'boolean':
      return { good: [true, false], bad: [...common, 'true', 0, 1, null] }
    case 'choice':
      return { good: values.options.map((option) => option.value), bad: [...common, 'Nope', null, 3, true] }
    case 'integer':
      return {
        good: [values.min, values.max, Math.floor((values.min + values.max) / 2)],
        bad: [values.min - 1, values.max + 1, values.min + 0.5, String(values.min), null, Number.NaN]
      }
    case 'theme':
      return { good: ['nocturne', 'my-theme-2'], bad: [...common, 'Nocturne', 'two  words', '-leading', null, 4] }
    case 'accent':
      return { good: [null, '#1a2b3c'], bad: [...common, '#ABCDEF', '#abc', 'red', 4] }
    case 'font':
      return { good: [null, 'JetBrains Mono'], bad: [...common.slice(0, 2), '', 'a; color: red', 'a,b', 'x'.repeat(101), 4] }
    case 'program':
      return { good: [null, ABSOLUTE], bad: [...common, '', 'relative\\claude.exe', 4] }
    case 'folders': {
      const twice = [ABSOLUTE, ABSOLUTE.toUpperCase()]
      return {
        good: [[], [ABSOLUTE], [ABSOLUTE, OTHER], ...(values.unique ? [] : [twice])],
        bad: [
          ...common,
          null,
          ['relative'],
          [''],
          [4],
          ...(values.unique ? [twice] : []),
          ...(values.most === null ? [] : [Array.from({ length: values.most + 1 }, (_, i) => `${ABSOLUTE}${String(i)}`)])
        ]
      }
    }
  }
}

describe('the settings table', () => {
  it('describes every validated setting, or names it as state the page does not show', () => {
    const listed = settingsTable().map((entry) => entry.key)
    const state = Object.keys(STATE_KEYS)
    for (const key of Object.keys(SETTING_VALIDATORS) as SettingKey[]) {
      const where = Number(listed.includes(key)) + Number(state.includes(key))
      expect(where, `${key} is a setting in the table or state, exactly once`).toBe(1)
    }
    expect(new Set(listed).size).toBe(listed.length)
  })

  it('takes exactly the values each validator takes, and says which when it does not', () => {
    for (const entry of settingsTable()) {
      const validator = SETTING_VALIDATORS[entry.key]
      const { good, bad } = samples(entry.values)
      for (const value of good.map((sample) => (entry.key === 'quitSessions' ? { local: sample } : sample))) {
        expect(validator(value), `${entry.key} validator on ${JSON.stringify(value)}`).toBeNull()
      }
      for (const value of good) expect(settingValueProblem(entry.values, value), `${entry.key}: ${String(value)}`).toBeNull()
      for (const value of bad) {
        const stored = entry.key === 'quitSessions' ? { local: value } : value
        expect(validator(stored), `${entry.key} validator refuses ${JSON.stringify(value)}`).not.toBeNull()
        expect(settingValueProblem(entry.values, value), `${entry.key} refuses ${JSON.stringify(value)}`).toBe(
          `it takes ${describeSettingValues(entry.values)}`
        )
      }
    }
  })

  it('gives each setting its default from DEFAULT_SETTINGS, one the setting takes', () => {
    for (const entry of settingsTable()) {
      const value = entry.defaultOn('local')
      expect(settingValueProblem(entry.values, value), entry.key).toBeNull()
      if (entry.key !== 'quitSessions') expect(value).toEqual(DEFAULT_SETTINGS[entry.key])
    }
    const quit = settingEntry('quitSessions')
    expect(quit.defaultOn('local')).toBe('end')
    expect(quit.defaultOn('build-box')).toBe('keep')
  })

  it('reads and patches a per-host setting by host, keeping every other host’s value', () => {
    const quit = settingEntry('quitSessions')
    const settings = { ...DEFAULT_SETTINGS, quitSessions: { 'build-box': 'end' as const } }
    expect(quit.read(settings, 'build-box')).toBe('end')
    expect(quit.read(settings, 'local')).toBe('end')
    expect(quit.read(settings, 'laptop')).toBe('keep')
    expect(quit.patch(settings, 'local', 'keep')).toEqual({ quitSessions: { 'build-box': 'end', local: 'keep' } })
    expect(settingEntry('agentDepth').patch(settings, 'local', 1)).toEqual({ agentDepth: 1 })
  })

  it('lets a session only narrow what agents, or Floti on its own, may reach or do', () => {
    const narrowOnly = settingsTable()
      .filter((entry) => entry.session === 'narrow')
      .map((entry) => entry.key)
    expect(narrowOnly.sort()).toEqual(
      [
        'agentChildrenPerParent',
        'agentCommandsPerParent',
        'agentDepth',
        'agentSessionsPerHost',
        'agentStartsPerParent',
        'browserMcp',
        'browserMcpLocalOnly',
        'browserReach',
        'browserSearch',
        'desktopMcp',
        'quitSessions',
        'restoreWithoutAsking',
        'scanRoots',
        'sessionAgents',
        'sessionMcp',
        'updateCheck'
      ].sort()
    )
    const narrows = (key: SettingKey, from: unknown, to: unknown): boolean => {
      const entry: SettingEntry = settingEntry(key)
      return entry.narrows?.(from, to) ?? false
    }
    expect(narrows('sessionAgents', true, false)).toBe(true)
    expect(narrows('sessionAgents', false, true)).toBe(false)
    expect(narrows('sessionAgents', false, false)).toBe(true)
    expect(narrows('browserMcpLocalOnly', false, true)).toBe(true)
    expect(narrows('browserMcpLocalOnly', true, false)).toBe(false)
    expect(narrows('browserReach', 'web', 'local')).toBe(true)
    expect(narrows('browserReach', 'local', 'web')).toBe(false)
    expect(narrows('agentDepth', 2, 1)).toBe(true)
    expect(narrows('agentDepth', 2, 3)).toBe(false)
    expect(narrows('scanRoots', [ABSOLUTE, OTHER], [ABSOLUTE])).toBe(true)
    expect(narrows('scanRoots', [ABSOLUTE, OTHER], [])).toBe(true)
    expect(narrows('scanRoots', [ABSOLUTE], [ABSOLUTE, OTHER])).toBe(false)
    // A folder differing only in case is another folder on macOS and Linux, so it is refused on every OS.
    expect(narrows('scanRoots', ['/home/u/work'], ['/home/u/WORK'])).toBe(false)
    expect(narrows('scanRoots', [ABSOLUTE], [ABSOLUTE.toUpperCase()])).toBe(false)
    // What Floti or its sessions do on their own goes only toward doing less.
    expect(narrows('updateCheck', true, false)).toBe(true)
    expect(narrows('updateCheck', false, true)).toBe(false)
    expect(narrows('browserSearch', 'google', 'off')).toBe(true)
    expect(narrows('browserSearch', 'off', 'google')).toBe(false)
    expect(narrows('browserSearch', 'google', 'bing')).toBe(false)
    expect(narrows('quitSessions', 'keep', 'end')).toBe(true)
    expect(narrows('quitSessions', 'end', 'keep')).toBe(false)
    expect(narrows('restoreWithoutAsking', true, false)).toBe(true)
    expect(narrows('restoreWithoutAsking', false, true)).toBe(false)
    // What names a program to run, and what would drop archived conversations, only the user changes.
    for (const key of ['claudePath', 'terminalShell', 'transcriptArchiveMaxBytes'] as const) {
      expect(settingEntry(key).session).toBe('nothing')
      expect(settingEntry(key).why).toBeTruthy()
    }
  })

  it('marks what the desktop tells its host', () => {
    const told = settingsTable()
      .filter((entry) => entry.toldToHost)
      .map((entry) => entry.key)
    expect(told).toContain('sessionAgents')
    expect(told).not.toContain('theme')
  })

  it('gives the page a hint per value where the page has one, and none where it shows none', () => {
    expect(settingHint('browserMcp', false)).toMatch(/^Off\./)
    expect(settingHint('browserSearch', 'duckduckgo')).toContain('DuckDuckGo')
    expect(settingHint('terminalCursorStyle')).toBeUndefined()
    expect(settingHint('sessionAgents')).toBe(settingEntry('sessionAgents').description)
    expect(() => settingEntry('paneLayout')).toThrow()
    expect(findSettingEntry('paneLayout')).toBeNull()
    expect(findSettingEntry('agentDepth')?.label).toBe('Depth')
  })

  it('says a value as the page would', () => {
    expect(settingValueWords(settingEntry('sessionAgents').values, false)).toBe('off')
    expect(settingValueWords(settingEntry('browserReach').values, 'local')).toBe('This machine only')
    expect(settingValueWords(settingEntry('accentColor').values, null)).toBe('the theme’s own')
    expect(settingValueWords(settingEntry('scanRoots').values, [])).toBe('none')
    expect(settingValueWords(settingEntry('agentDepth').values, 3)).toBe('3')
  })
})
