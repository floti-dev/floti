import { describe, expect, it } from 'vitest'
import {
  agentFileValues,
  agentNameIssue,
  applyAgentDraftChange,
  copyName,
  differingLines,
  readAgentDraft,
  withBody,
  withoutRetiredLines,
  yamlScalar
} from './agentFile'

const LEAD = [
  '---',
  'description: Researches a new project with you and writes the plan.',
  'backend: claude-code',
  'model:',
  '  claude-code: opus',
  'effort:',
  '  claude-code: high',
  '# a note the user wrote',
  'permission: plan',
  'class: light',
  'worktree: no',
  'can_start: yes',
  'allowed_tools: Read, Grep, Bash(git log:*)',
  '---',
  'You research a new project.',
  ''
].join('\n')

describe('readAgentDraft', () => {
  it('reads each field as the form shows it', () => {
    const draft = readAgentDraft(LEAD)
    expect(draft).toMatchObject({
      frontMatter: true,
      description: 'Researches a new project with you and writes the plan.',
      profile: null,
      backends: ['claude-code'],
      models: { 'claude-code': 'opus' },
      efforts: { 'claude-code': 'high' },
      permission: 'plan',
      worktree: false,
      canStart: true,
      allowedTools: ['Read', 'Grep', 'Bash(git log:*)'],
      disallowedTools: [],
      sourceOnly: [],
      retired: [{ field: 'class', line: 10, value: 'light' }],
      body: 'You research a new project.\n'
    })
  })

  it('reads YAML lists, flow maps, quoted text and keeps a value outside the choices', () => {
    const text = [
      '---',
      'description: "Says: hello"',
      'backend: [codex, Claude-Code]',
      'effort: {codex: extreme}',
      'disallowed_tools:',
      '  - Edit',
      "  - 'Write'",
      'permission: yolo',
      'command: ./run.sh',
      '---',
      'body'
    ].join('\n')
    expect(readAgentDraft(text)).toMatchObject({
      description: 'Says: hello',
      backends: ['codex', 'claude-code'],
      efforts: { codex: 'extreme' },
      disallowedTools: ['Edit', 'Write'],
      permission: 'yolo',
      sourceOnly: ['command']
    })
  })

  it('treats a text with no front matter as all instructions', () => {
    expect(readAgentDraft('just words\n')).toMatchObject({ frontMatter: false, body: 'just words\n' })
  })

  it('gives values with their lines, as the host splits them', () => {
    expect(agentFileValues(LEAD).slice(2, 4)).toEqual([
      { field: 'model', line: 4, value: 'claude-code: opus' },
      { field: 'effort', line: 6, value: 'claude-code: high' }
    ])
  })
})

describe('applyAgentDraftChange', () => {
  it('rewrites only the lines of the field it changes', () => {
    const next = applyAgentDraftChange(LEAD, { field: 'efforts', value: { 'claude-code': 'xhigh' } })
    expect(next).toBe(LEAD.replace('claude-code: high', 'claude-code: xhigh'))
  })

  it('adds a missing field in the field order, and removes one set to nothing', () => {
    const added = applyAgentDraftChange(LEAD, { field: 'profile', value: 'studio docs' })
    const lines = added.split('\n')
    expect(lines[2]).toBe('profile: studio docs')
    expect(lines[1]).toMatch(/^description:/)
    const removed = applyAgentDraftChange(LEAD, { field: 'models', value: {} })
    expect(removed).not.toContain('model:')
    expect(removed).not.toContain('claude-code: opus')
    expect(removed).toContain('effort:\n  claude-code: high')
  })

  it('writes flags, lists and quoted text the host reads back', () => {
    let text = applyAgentDraftChange(LEAD, { field: 'worktree', value: true })
    text = applyAgentDraftChange(text, { field: 'disallowedTools', value: ['Edit', 'Write'] })
    text = applyAgentDraftChange(text, { field: 'description', value: 'Plans: then stops' })
    expect(text).toContain('worktree: yes')
    expect(text).toContain('disallowed_tools: Edit, Write\n---')
    expect(text).toContain('description: "Plans: then stops"')
    expect(readAgentDraft(text).description).toBe('Plans: then stops')
  })

  it('leaves an absent flag out when it is turned off', () => {
    const text = '---\ndescription: d\n---\n'
    expect(applyAgentDraftChange(text, { field: 'canStart', value: false })).toBe(text)
  })

  it('makes front matter for a text that has none', () => {
    const text = applyAgentDraftChange('words\n', { field: 'description', value: 'd' })
    expect(text).toBe('---\ndescription: d\n---\nwords\n')
  })

  it('keeps CRLF line ends and a byte order mark', () => {
    const crlf = '﻿---\r\ndescription: a\r\n---\r\nbody\r\n'
    const next = applyAgentDraftChange(crlf, { field: 'description', value: 'b' })
    expect(next).toBe('﻿---\r\ndescription: b\r\n---\r\nbody\r\n')
  })

  it('replaces the instructions and keeps the front matter', () => {
    expect(withBody(LEAD, 'New body\n')).toBe(LEAD.replace('You research a new project.\n', 'New body\n'))
  })
})

describe('withoutRetiredLines', () => {
  it('drops the class line and nothing else', () => {
    expect(withoutRetiredLines(LEAD)).toBe(LEAD.replace('class: light\n', ''))
  })
})

describe('yamlScalar', () => {
  it('quotes what YAML would read as something else', () => {
    expect(yamlScalar('plain words')).toBe('plain words')
    expect(yamlScalar('yes')).toBe('"yes"')
    expect(yamlScalar('12')).toBe('"12"')
    expect(yamlScalar('a: b')).toBe('"a: b"')
    expect(yamlScalar('- item')).toBe('"- item"')
  })
})

describe('names', () => {
  it('says what a typed name would become', () => {
    const taken = new Set(['worker'])
    expect(agentNameIssue('Android Worker', taken)).toEqual({ kind: 'invalid', suggestion: 'android-worker' })
    expect(agentNameIssue('worker', taken)).toEqual({ kind: 'taken' })
    expect(agentNameIssue('', taken)).toEqual({ kind: 'empty' })
    expect(agentNameIssue('android-worker', taken)).toBeNull()
  })

  it('names a copy after the first free number', () => {
    expect(copyName('worker', new Set(['worker', 'worker-2']))).toBe('worker-3')
  })
})

describe('differingLines', () => {
  it('marks the lines each side has that the other lacks', () => {
    const { left, right } = differingLines(['a', 'b', 'c', 'd'].join('\n'), ['a', 'B', 'c', 'd', 'e'].join('\n'))
    expect([...left]).toEqual([2])
    expect([...right]).toEqual([2, 5])
  })
})
