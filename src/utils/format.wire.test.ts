import { describe, expect, it } from 'vitest'

import {
  IPC_WIRE_FORMAT,
  base64WireToUtf8Display,
  defaultFieldViewFmt,
  fieldViewOptions,
  meFormatViewValue,
  meViewToWire,
  toWireFormat,
  utf8TextToBase64,
} from '@/utils/format'

describe('format wire=base64 display-only', () => {
  it('toWireFormat 恒 base64', () => {
    expect(toWireFormat('utf8')).toBe(IPC_WIRE_FORMAT)
    expect(toWireFormat('strjson')).toBe('base64')
    expect(toWireFormat('hex')).toBe('base64')
  })

  it('utf8 展示/写回 roundtrip', () => {
    const text = 'hello 你好'
    const wire = utf8TextToBase64(text)
    expect(meFormatViewValue(wire, 'utf8')).toBe(text)
    expect(meViewToWire(text, 'utf8')).toBe(wire)
    expect(base64WireToUtf8Display(wire)).toBe(text)
  })

  it('空值', () => {
    expect(meFormatViewValue('', 'utf8')).toBe('')
    expect(meViewToWire('', 'utf8')).toBe('')
    expect(meFormatViewValue('', 'hex')).toBe('')
  })

  it('hex roundtrip 经 base64 wire', () => {
    const wire = utf8TextToBase64('AB')
    const hex = meFormatViewValue(wire, 'hex')
    expect(hex).toMatch(/^[0-9a-f]+$/)
    expect(meViewToWire(hex, 'hex')).toBe(wire)
  })

  it('fieldViewOptions 含 JavaSerial/Pickle/UTF8', () => {
    const labels = fieldViewOptions().map(o => o.label)
    expect(labels).toContain('UTF8')
    expect(labels).toContain('JavaSerial')
    expect(labels).toContain('Pickle')
    expect(labels).toContain('MsgPack')
  })

  it('defaultFieldViewFmt：无魔数时跟随键级', () => {
    const wire = utf8TextToBase64('plain')
    expect(defaultFieldViewFmt(wire, 'hex')).toBe('hex')
    expect(defaultFieldViewFmt(wire, 'utf8')).toBe('utf8')
  })

  it('defaultFieldViewFmt：ACED 识别为 JavaSerial', () => {
    // java.util.TreeSet 样例（与 javaserial / detect 单测同源）
    const b64 = 'rO0ABXNyABFqYXZhLnV0aWwuVHJlZVNldLq2J1h0c5eMAwAAeHB3BAAAAAJ0AAFhdAABYng='
    expect(defaultFieldViewFmt(b64, 'utf8')).toBe('javaserial')
  })
})
