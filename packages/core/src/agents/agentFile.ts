/**
 * An agent file as the Agents page edits it: one draft text, which the form
 * reads its fields from and writes each change into, line by line.
 *
 * Pure by construction and re-exported through `types.ts`. The form and the
 * Source view are two views of the same text, so a form edit rewrites only
 * the lines of the field it changed: comments, unknown lines and the order
 * the file was written in all stay as they were. The host is the judge of
 * whether a text is an agent (`agent_definitions/parse.rs`); this module
 * reads fields the way that parser splits them (`written_values`) and never
 * refuses a text, so a bad file still shows each value as written.
 */

/** The fields an agent file has, in the order a new line is placed in. */
export const AGENT_FILE_FIELDS = [
  'description',
  'profile',
  'backend',
  'model',
  'effort',
  'permission',
  'worktree',
  'can_start',
  'allowed_tools',
  'disallowed_tools',
  'command',
  'submit',
  'stop'
] as const

export type AgentFileField = (typeof AGENT_FILE_FIELDS)[number]

/** Fields the form has no control for: an agent that has them edits them in Source. */
export const SOURCE_ONLY_FIELDS: readonly AgentFileField[] = ['command', 'submit', 'stop']

/** Lines Floti once read and no longer does (`RETIRED_FIELDS` in the host's parser). */
export const RETIRED_AGENT_FIELDS: readonly string[] = ['class']

/** The permission words an agent file takes, with the launcher's label for each. */
export const AGENT_PERMISSIONS: ReadonlyArray<{ word: string; label: string }> = [
  { word: 'manual', label: 'Ask first' },
  { word: 'plan', label: 'Plan' },
  { word: 'accept-edits', label: 'Accept edits' },
  { word: 'auto', label: 'Auto' },
  { word: 'dont-ask', label: "Don't ask" },
  { word: 'bypass-permissions', label: 'Bypass permissions' }
]

/** The efforts an agent file takes, lowest first. */
export const AGENT_EFFORTS: readonly string[] = ['low', 'medium', 'high', 'xhigh', 'max']

/** One top-level field of the front matter as the file writes it. */
export interface AgentFileValue {
  field: string
  /** The file's line it is on, from 1. */
  line: number
  /** What follows its colon and the lines under it, dedented and trimmed. */
  value: string
}

/** What the form shows of a draft. */
export interface AgentDraftFields {
  /** Whether the text has front matter the form can read. */
  frontMatter: boolean
  description: string
  profile: string | null
  /** The backends it names, in its order; empty for any installed one. */
  backends: string[]
  /** The model for each backend, by backend id, as written. */
  models: Record<string, string>
  /** The effort for each backend, by backend id, as written: a word outside the choices stays. */
  efforts: Record<string, string>
  /** The permission word as written, or null for none. */
  permission: string | null
  worktree: boolean
  canStart: boolean
  allowedTools: string[]
  disallowedTools: string[]
  /** The fields it has that only Source edits (`command`, `submit`, `stop`). */
  sourceOnly: string[]
  /** The retired lines it has, by field, with their lines. */
  retired: AgentFileValue[]
  /** The instructions: the text after the front matter. */
  body: string
}

/** An agent name: lowercase letters, digits and dashes (`is_agent_name` in the host). */
export function isAgentName(name: string): boolean {
  return /^[a-z0-9-]+$/.test(name)
}

/** What the name field says of what is typed, or null when it is a name the folder accepts. */
export type AgentNameIssue =
  | { kind: 'empty' }
  | { kind: 'invalid'; suggestion: string | null }
  | { kind: 'taken' }

export function agentNameIssue(typed: string, taken: ReadonlySet<string>): AgentNameIssue | null {
  if (typed === '') return { kind: 'empty' }
  if (!isAgentName(typed)) {
    const suggestion = typed
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '')
    return { kind: 'invalid', suggestion: suggestion === '' ? null : suggestion }
  }
  return taken.has(typed) ? { kind: 'taken' } : null
}

/** The name a copy of `name` is given: `<name>-2`, or the next free number. */
export function copyName(name: string, taken: ReadonlySet<string>): string {
  for (let n = 2; ; n += 1) {
    const candidate = `${name}-${n}`
    if (!taken.has(candidate)) return candidate
  }
}

/** Every name a host's agents folder holds: its agents' and the files that are not agents'. */
export function listedAgentNames(listing: {
  agents: ReadonlyArray<{ name: string }>
  bad: ReadonlyArray<{ name: string }>
}): Set<string> {
  return new Set([...listing.agents.map((agent) => agent.name), ...listing.bad.map((file) => file.name)])
}

/** The text a blank agent starts from. */
export function blankAgentText(): string {
  return '---\ndescription:\n---\n'
}

// ---------------------------------------------------------------------------
// The file, split into lines

interface Lines {
  bom: string
  eol: string
  lines: string[]
  /** The fences' line indices, or null when there is no front matter. */
  open: number | null
  close: number | null
}

interface Block {
  field: string
  rest: string
  /** First line index of the field. */
  start: number
  /** One past the last line of its value, trailing blank lines left out. */
  end: number
  under: string[]
}

function split(text: string): Lines {
  const bom = text.startsWith('﻿') ? '﻿' : ''
  const body = text.slice(bom.length)
  const eol = body.includes('\r\n') ? '\r\n' : '\n'
  const lines = body.split('\n').map((line) => (line.endsWith('\r') ? line.slice(0, -1) : line))
  if (lines[0]?.trim() !== '---') return { bom, eol, lines, open: null, close: null }
  for (let i = 1; i < lines.length; i += 1) {
    const fence = (lines[i] ?? '').trim()
    if (fence === '---' || fence === '...') return { bom, eol, lines, open: 0, close: i }
  }
  return { bom, eol, lines, open: null, close: null }
}

function join(parts: Lines): string {
  return parts.bom + parts.lines.join(parts.eol)
}

/** The key and what follows its colon, when `line` starts a top-level field. */
function fieldLine(line: string): { field: string; rest: string } | null {
  const first = line.charAt(0)
  if (first === '' || /\s/.test(first) || '#-[{?'.includes(first)) return null
  if (first === '"' || first === "'") {
    const close = line.indexOf(first, 1)
    if (close < 0) return null
    const after = line.slice(close + 1).trimStart()
    if (!after.startsWith(':')) return null
    const field = line.slice(1, close)
    return field === '' ? null : { field, rest: after.slice(1) }
  }
  const colon = line.indexOf(':')
  if (colon < 0) return null
  const field = line.slice(0, colon).trimEnd()
  return field === '' ? null : { field, rest: line.slice(colon + 1) }
}

function blocks(parts: Lines): Block[] {
  if (parts.open === null || parts.close === null) return []
  const out: Block[] = []
  for (let i = parts.open + 1; i < parts.close; i += 1) {
    const line = parts.lines[i] ?? ''
    const field = fieldLine(line)
    if (field) {
      out.push({ ...field, start: i, end: i + 1, under: [] })
      continue
    }
    const last = out[out.length - 1]
    if (!last || line.startsWith('#')) continue
    last.under.push(line)
    if (line.trim() !== '') last.end = i + 1
  }
  return out
}

function dedented(lines: string[]): string {
  const kept = lines.filter((line) => line.trim() !== '')
  if (kept.length === 0) return ''
  const first = lines.findIndex((line) => line.trim() !== '')
  let last = lines.length - 1
  while ((lines[last] ?? '').trim() === '') last -= 1
  const indent = Math.min(...kept.map((line) => line.length - line.trimStart().length))
  return lines
    .slice(first, last + 1)
    .map((line) => line.slice(Math.min(indent, line.length - line.trimStart().length)).trimEnd())
    .join('\n')
}

function valueOf(block: Block): string {
  const head = block.rest.trim()
  const under = dedented(block.under)
  if (under === '') return head
  return head === '' ? under : `${head}\n${under}`
}

/** Each top-level field of the text's front matter as written, in the file's order. */
export function agentFileValues(text: string): AgentFileValue[] {
  return blocks(split(text)).map((block) => ({
    field: block.field,
    line: block.start + 1,
    value: valueOf(block)
  }))
}

// ---------------------------------------------------------------------------
// Reading values as the form shows them

function unquote(raw: string): string {
  const value = raw.trim()
  if (value.length >= 2 && value.startsWith('"') && value.endsWith('"')) {
    try {
      return JSON.parse(value) as string
    } catch {
      return value.slice(1, -1)
    }
  }
  if (value.length >= 2 && value.startsWith("'") && value.endsWith("'")) {
    return value.slice(1, -1).replace(/''/g, "'")
  }
  const comment = value.search(/\s#/)
  return comment < 0 ? value : value.slice(0, comment).trimEnd()
}

/** `text` split at the commas outside parentheses and quotes. */
function splitCommas(text: string): string[] {
  const items: string[] = []
  let depth = 0
  let quote: string | null = null
  let current = ''
  for (const c of text) {
    if (quote) {
      if (c === quote) quote = null
    } else if (c === '"' || c === "'") quote = c
    else if (c === '(' || c === '[' || c === '{') depth += 1
    else if (c === ')' || c === ']' || c === '}') depth = Math.max(0, depth - 1)
    else if (c === ',' && depth === 0) {
      items.push(current)
      current = ''
      continue
    }
    current += c
  }
  items.push(current)
  return items
}

function readList(value: string): string[] {
  const text = value.trim()
  if (text === '') return []
  let items: string[]
  if (text.startsWith('[') && text.endsWith(']')) {
    items = splitCommas(text.slice(1, -1)).map(unquote)
  } else if (text.split('\n').every((line) => line.trim().startsWith('-'))) {
    items = text.split('\n').map((line) => unquote(line.trim().slice(1)))
  } else {
    items = splitCommas(unquote(text)).map((item) => item.trim())
  }
  return items.filter((item) => item !== '')
}

function readMap(value: string): Record<string, string> {
  const text = value.trim()
  const entries =
    text.startsWith('{') && text.endsWith('}') ? splitCommas(text.slice(1, -1)) : text.split('\n')
  const out: Record<string, string> = {}
  for (const entry of entries) {
    const colon = entry.indexOf(':')
    if (colon < 0) continue
    const key = unquote(entry.slice(0, colon)).toLowerCase()
    if (key !== '') out[key] = unquote(entry.slice(colon + 1))
  }
  return out
}

function readFlag(value: string): boolean {
  return ['yes', 'true'].includes(unquote(value).toLowerCase())
}

/** What the form shows of `text`. */
export function readAgentDraft(text: string): AgentDraftFields {
  const parts = split(text)
  const found = blocks(parts)
  const value = (field: string): string | null => {
    const block = found.find((b) => b.field === field)
    return block ? valueOf(block) : null
  }
  const text_ = (field: string): string | null => {
    const raw = value(field)
    if (raw === null) return null
    const unquoted = unquote(raw)
    return unquoted === '' || unquoted === '~' || unquoted === 'null' ? null : unquoted
  }
  const body =
    parts.close === null ? join({ ...parts, bom: '' }) : parts.lines.slice(parts.close + 1).join('\n')
  return {
    frontMatter: parts.close !== null,
    description: text_('description') ?? '',
    profile: text_('profile'),
    backends: [...new Set(readList(value('backend') ?? '').map((id) => id.toLowerCase()))],
    models: readMap(value('model') ?? ''),
    efforts: readMap(value('effort') ?? ''),
    permission: text_('permission'),
    worktree: readFlag(value('worktree') ?? ''),
    canStart: readFlag(value('can_start') ?? ''),
    allowedTools: readList(value('allowed_tools') ?? ''),
    disallowedTools: readList(value('disallowed_tools') ?? ''),
    sourceOnly: found.map((b) => b.field).filter((f) => (SOURCE_ONLY_FIELDS as string[]).includes(f)),
    retired: found
      .filter((b) => RETIRED_AGENT_FIELDS.includes(b.field))
      .map((b) => ({ field: b.field, line: b.start + 1, value: valueOf(b) })),
    body
  }
}

// ---------------------------------------------------------------------------
// Writing

const RESERVED = /^(?:yes|no|true|false|null|~|on|off|y|n)$/i

/** `value` as one YAML scalar: plain where the parser reads it back as the same text, else quoted. */
export function yamlScalar(value: string): string {
  const flat = value.replace(/\r?\n/g, ' ')
  const plain =
    flat !== '' &&
    flat === flat.trim() &&
    !RESERVED.test(flat) &&
    !/^[-+.\d]/.test(flat) &&
    !/^[!&*?|>'"%@`#,[\]{}:]/.test(flat) &&
    !flat.includes(': ') &&
    !flat.includes(' #') &&
    !flat.endsWith(':')
  return plain ? flat : JSON.stringify(flat)
}

/** The text after `field:` for each kind of value, or null to leave the line out. */
function rendered(lines: string[] | string | null): string | null {
  if (lines === null) return null
  if (typeof lines === 'string') return lines === '' ? null : ` ${lines}`
  return lines.length === 0 ? null : lines.map((line) => `\n  ${line}`).join('')
}

/**
 * `text` with `field`'s lines replaced by `after` (what follows its colon),
 * added in the field order when the file lacks it, or taken out when `after`
 * is null. Every other line stays as written.
 */
function withField(text: string, field: string, after: string | null): string {
  let parts = split(text)
  if (parts.close === null) {
    if (after === null) return text
    const rest = join({ ...parts, bom: '' })
    parts = split(`---${parts.eol}---${parts.eol}${rest}`)
    parts.bom = text.startsWith('﻿') ? '﻿' : ''
  }
  const found = blocks(parts)
  const own = found.filter((b) => b.field === field)
  const replacement = after === null ? [] : `${field}:${after}`.split('\n')
  if (own.length > 0) {
    // A repeated key is replaced at its first line and the others dropped.
    const lines = [...parts.lines]
    for (const block of [...own].reverse().slice(0, -1)) lines.splice(block.start, block.end - block.start)
    const first = own[0]!
    lines.splice(first.start, first.end - first.start, ...replacement)
    return join({ ...parts, lines })
  }
  if (after === null) return text
  const order = AGENT_FILE_FIELDS as readonly string[]
  const rank = order.indexOf(field)
  const before = found.filter((b) => {
    const at = order.indexOf(b.field)
    return at >= 0 && at < rank
  })
  const at =
    before.length > 0
      ? before[before.length - 1]!.end
      : (found.find((b) => order.indexOf(b.field) > rank)?.start ?? (parts.close as number))
  const lines = [...parts.lines]
  lines.splice(at, 0, ...replacement)
  return join({ ...parts, lines })
}

export type AgentDraftChange =
  | { field: 'description'; value: string }
  | { field: 'profile'; value: string | null }
  | { field: 'backends'; value: string[] }
  | { field: 'models'; value: Record<string, string> }
  | { field: 'efforts'; value: Record<string, string> }
  | { field: 'permission'; value: string | null }
  | { field: 'worktree' | 'canStart'; value: boolean }
  | { field: 'allowedTools' | 'disallowedTools'; value: string[] }
  | { field: 'body'; value: string }

function mapLines(map: Record<string, string>): string[] {
  return Object.entries(map)
    .filter(([, v]) => v !== '')
    .map(([k, v]) => `${k}: ${yamlScalar(v)}`)
}

function list(items: string[]): string | null {
  const kept = items.map((item) => item.trim()).filter((item) => item !== '')
  return kept.length === 0 ? null : yamlScalar(kept.join(', '))
}

/** `text` with one form change written into it. */
export function applyAgentDraftChange(text: string, change: AgentDraftChange): string {
  switch (change.field) {
    case 'description':
      return withField(text, 'description', rendered(change.value.trim() === '' ? null : yamlScalar(change.value.trim())))
    case 'profile':
      return withField(text, 'profile', rendered(change.value === null ? null : yamlScalar(change.value)))
    case 'backends':
      return withField(text, 'backend', rendered(list(change.value)))
    case 'models':
      return withField(text, 'model', rendered(mapLines(change.value)))
    case 'efforts':
      return withField(text, 'effort', rendered(mapLines(change.value)))
    case 'permission':
      return withField(text, 'permission', rendered(change.value))
    case 'worktree':
    case 'canStart': {
      const field = change.field === 'worktree' ? 'worktree' : 'can_start'
      const present = agentFileValues(text).some((v) => v.field === field)
      if (!change.value && !present) return text
      return withField(text, field, change.value ? ' yes' : ' no')
    }
    case 'allowedTools':
      return withField(text, 'allowed_tools', rendered(list(change.value)))
    case 'disallowedTools':
      return withField(text, 'disallowed_tools', rendered(list(change.value)))
    case 'body':
      return withBody(text, change.value)
  }
}

/** `text` with its instructions replaced by `body`. */
export function withBody(text: string, body: string): string {
  const parts = split(text)
  if (parts.close === null) {
    return parts.bom + body.split('\n').join(parts.eol)
  }
  const lines = [...parts.lines.slice(0, parts.close + 1), ...body.split('\n')]
  return join({ ...parts, lines })
}

/** `text` without its retired lines: what Save writes. */
export function withoutRetiredLines(text: string): string {
  let out = text
  for (const field of RETIRED_AGENT_FIELDS) out = withField(out, field, null)
  return out
}

/**
 * The lines of `left` and of `right` (1-based) that are not in their longest
 * common run of lines: what the comparison marks on each side.
 */
export function differingLines(left: string, right: string): { left: Set<number>; right: Set<number> } {
  const a = left.split(/\r?\n/)
  const b = right.split(/\r?\n/)
  // The common run's length from each pair of positions to the end.
  const width = b.length + 1
  const table = new Array<number>((a.length + 1) * width).fill(0)
  const at = (i: number, j: number): number => table[i * width + j] ?? 0
  for (let i = a.length - 1; i >= 0; i -= 1) {
    for (let j = b.length - 1; j >= 0; j -= 1) {
      table[i * width + j] = a[i] === b[j] ? at(i + 1, j + 1) + 1 : Math.max(at(i + 1, j), at(i, j + 1))
    }
  }
  const out = { left: new Set<number>(), right: new Set<number>() }
  let i = 0
  let j = 0
  while (i < a.length || j < b.length) {
    if (i < a.length && j < b.length && a[i] === b[j]) {
      i += 1
      j += 1
    } else if (j >= b.length || (i < a.length && at(i + 1, j) >= at(i, j + 1))) {
      out.left.add(i + 1)
      i += 1
    } else {
      out.right.add(j + 1)
      j += 1
    }
  }
  return out
}
