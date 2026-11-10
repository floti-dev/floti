import { Buffer } from 'node:buffer'
import { describe, expect, it } from 'vitest'
import { clipboardWriteOf, PROGRAM_CLIPBOARD_MAX_BYTES } from './programClipboard'

/**
 * What an OSC 52 body asks to put on the clipboard: a write's decoded text,
 * and nothing for a read or for anything that is not a clean write.
 */

const base64 = (text: string): string => Buffer.from(text, 'utf8').toString('base64')
const bytes = (...values: number[]): string => Buffer.from(values).toString('base64')

describe('an OSC 52 write', () => {
  it('is the payload decoded as UTF-8, for the clipboard, primary and selection, and none named', () => {
    const text = 'naïve → 日本 ✓\nsecond line'
    for (const selection of ['c', 'p', 's', '', 'pc', 'cs']) {
      expect(clipboardWriteOf(`${selection};${base64(text)}`)).toBe(text)
    }
  })

  it('takes base64 with its padding left off, as some programs write it', () => {
    expect(base64('ab')).toBe('YWI=')
    expect(clipboardWriteOf('c;YWI')).toBe('ab')
    expect(clipboardWriteOf('c;YQ')).toBe('a')
  })

  it('is taken up to the size limit and dropped past it', () => {
    const atLimit = 'a'.repeat(PROGRAM_CLIPBOARD_MAX_BYTES)
    expect(clipboardWriteOf(`c;${base64(atLimit)}`)).toBe(atLimit)
    expect(clipboardWriteOf(`c;${base64(`${atLimit}a`)}`)).toBeNull()
  })
})

describe('an OSC 52 that writes nothing', () => {
  it('is a read request, which gets no answer', () => {
    expect(clipboardWriteOf('c;?')).toBeNull()
    expect(clipboardWriteOf(';?')).toBeNull()
  })

  it('is base64 that does not decode', () => {
    for (const payload of ['not base64!', 'YWJj ZA==', 'YWJjZ', 'YW=I', 'YWI==', '====', 'YQ=']) {
      expect(clipboardWriteOf(`c;${payload}`), payload).toBeNull()
    }
  })

  it('is bytes that are not UTF-8', () => {
    expect(clipboardWriteOf(`c;${bytes(0xff, 0xfe, 0x41)}`)).toBeNull()
    expect(clipboardWriteOf(`c;${bytes(0xe6, 0x97)}`)).toBeNull()
  })

  it('is an empty payload, which leaves the clipboard as it is', () => {
    expect(clipboardWriteOf('c;')).toBeNull()
  })

  it('is a body with no selection part, or a selection that is not the clipboard', () => {
    expect(clipboardWriteOf(base64('hello'))).toBeNull()
    for (const selection of ['0', '7', 'q', 'c0', 'x']) {
      expect(clipboardWriteOf(`${selection};${base64('hello')}`), selection).toBeNull()
    }
  })
})
