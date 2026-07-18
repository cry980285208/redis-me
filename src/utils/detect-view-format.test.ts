import { encode } from '@msgpack/msgpack'
import { describe, expect, it } from 'vitest'

import { detectViewFormat } from '@/utils/detect-view-format'

function utf8ToBase64(text: string): string {
  const bytes = new TextEncoder().encode(text)
  let binary = ''
  for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i]!)
  return btoa(binary)
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = ''
  for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i]!)
  return btoa(binary)
}

describe('detectViewFormat', () => {
  it('空值 → utf8', () => {
    expect(detectViewFormat('')).toBe('utf8')
  })

  it('ACED JavaSerial', () => {
    // java.util.TreeSet 样例（与 javaserial 单测同源）
    const b64 = 'rO0ABXNyABFqYXZhLnV0aWwuVHJlZVNldN2YUJOV7YdbAwAAeHBwdwQAAAACdAABYXQAAWJ4'
    expect(detectViewFormat(b64)).toBe('javaserial')
  })

  it('MsgPack map', () => {
    expect(detectViewFormat(bytesToBase64(encode({ a: 1, b: 'x' })))).toBe('msgpack')
  })

  it('MsgPack array', () => {
    expect(detectViewFormat(bytesToBase64(encode([1, 2, 3])))).toBe('msgpack')
  })

  it('普通 JSON 文本 → utf8（不是 StrJson）', () => {
    expect(detectViewFormat(utf8ToBase64('{"a":1}'))).toBe('utf8')
  })

  it('双层 StrJson', () => {
    const wire = JSON.stringify(JSON.stringify({ a: 1 }))
    expect(detectViewFormat(utf8ToBase64(wire))).toBe('strjson')
  })

  it('普通 UTF-8 文本', () => {
    expect(detectViewFormat(utf8ToBase64('hello 你好'))).toBe('utf8')
  })

  it('非法 UTF-8 → hex', () => {
    expect(detectViewFormat(bytesToBase64(new Uint8Array([0xff, 0xfe, 0xfd])))).toBe('hex')
  })

  it('非法 base64 → hex', () => {
    expect(detectViewFormat('!!!not-base64!!!')).toBe('hex')
  })

  it('短 UTF-8 不被 MsgPack 误判', () => {
    expect(detectViewFormat(utf8ToBase64('hello'))).toBe('utf8')
    expect(detectViewFormat(utf8ToBase64('a'))).toBe('utf8')
  })
})
