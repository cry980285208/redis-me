import { encode } from '@msgpack/msgpack'
import { describe, expect, it } from 'vite-plus/test'

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

  it('ACED JdkSerial', () => {
    // java.util.TreeSet 样例（与 javaserial 单测同源）
    const b64 = 'rO0ABXNyABFqYXZhLnV0aWwuVHJlZVNldN2YUJOV7YdbAwAAeHBwdwQAAAACdAABYXQAAWJ4'
    expect(detectViewFormat(b64)).toBe('javaserial')
  })

  it('截断 ACED 试解失败 → 不认 JdkSerial', () => {
    expect(detectViewFormat(bytesToBase64(new Uint8Array([0xac, 0xed, 0x00, 0x05])))).toBe('hex')
  })

  it('Pickle PROTO4 dict', () => {
    expect(detectViewFormat('gASVFwAAAAAAAAB9lCiMAWGUSwGMAWKUXZQoSwFLAmV1Lg==')).toBe('pickle')
  })

  it('截断 PROTO 试解失败 → 不认 Pickle', () => {
    expect(detectViewFormat(bytesToBase64(new Uint8Array([0x80, 0x04])))).toBe('hex')
  })

  it('Pickle PROTO2 不被 MsgPack 误判', () => {
    expect(detectViewFormat('gAJ9cQBYAQAAAGtxAVgBAAAAdnECcy4=')).toBe('pickle')
  })

  it('PhpSerial 数组 / 对象', () => {
    expect(detectViewFormat(utf8ToBase64('a:1:{s:1:"a";i:1;}'))).toBe('phpserial')
    expect(detectViewFormat(utf8ToBase64('O:4:"User":1:{s:4:"name";s:3:"Bob";}'))).toBe('phpserial')
  })

  it('PhpSerial 标量根不参与 Auto → utf8', () => {
    expect(detectViewFormat(utf8ToBase64('s:5:"hello";'))).toBe('utf8')
    expect(detectViewFormat(utf8ToBase64('i:123;'))).toBe('utf8')
  })

  it('截断 PhpSerial 试解失败 → 不认', () => {
    expect(detectViewFormat(utf8ToBase64('a:1:{i:0;'))).toBe('utf8')
  })

  it('a 开头普通文本不被误判为 PhpSerial', () => {
    expect(detectViewFormat(utf8ToBase64('apricot and apple'))).toBe('utf8')
  })

  it('MsgPack 空 map(0x80) 不是 Pickle', () => {
    expect(detectViewFormat(bytesToBase64(new Uint8Array([0x80])))).toBe('msgpack')
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
