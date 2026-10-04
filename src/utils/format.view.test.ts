import { describe, expect, it } from 'vite-plus/test'

import i18n from '@/locales'
import {
  CODEC_STDIN_B64_THRESHOLD,
  buildCodecCommand,
  customFormatName,
  customFormatValue,
  decodeErrTitle,
  fieldViewOptions,
  findCustomCodec,
  isCustomView,
  isReadonlyView,
  isStringOnlyView,
  isViewDecodeError,
  meFormatViewValue,
  meJsonToMsgpackBase64,
  meVector32Base64ToDisplay,
  meMsgpackBase64ToJson,
  meViewToWire,
  needsJsonNormalize,
  needsStdinInput,
  parseCodecErrorDetail,
  readonlyViewTip,
  viewFmtForField,
} from '@/utils/format'

describe('view flags', () => {
  it('custom 名称、仅 STRING 的视图、只读视图', () => {
    expect(isCustomView('custom:Py')).toBe(true)
    expect(isCustomView('utf8')).toBe(false)
    expect(customFormatValue('Py')).toBe('custom:Py')
    expect(customFormatName('custom:Py')).toBe('Py')
    expect(customFormatName('utf8')).toBe(null)
    expect(customFormatName('custom:')).toBe('')

    for (const view of [
      'auto',
      'msgpack',
      'vector32',
      'strjson',
      'javaserial',
      'pickle',
      'phpserial',
    ] as const) {
      expect(isStringOnlyView(view)).toBe(true)
    }
    expect(isStringOnlyView('custom:Py')).toBe(true)
    for (const view of ['utf8', 'hex', 'binary', 'base64'] as const) {
      expect(isStringOnlyView(view)).toBe(false)
    }

    expect(isReadonlyView('javaserial')).toBe(true)
    expect(isReadonlyView('pickle')).toBe(true)
    expect(isReadonlyView('phpserial')).toBe(true)
    expect(isReadonlyView('vector32')).toBe(true)
    expect(isReadonlyView('utf8')).toBe(false)
    expect(needsJsonNormalize('msgpack')).toBe(true)
    expect(needsJsonNormalize('strjson')).toBe(false)
  })

  it('非 STRING 键把 string-only 视图降成 utf8', () => {
    expect(viewFmtForField('auto')).toBe('utf8')
    expect(viewFmtForField('msgpack')).toBe('utf8')
    expect(viewFmtForField('vector32')).toBe('utf8')
    expect(viewFmtForField('custom:Py')).toBe('utf8')
    expect(viewFmtForField('hex')).toBe('hex')
    expect(viewFmtForField('binary')).toBe('binary')
  })

  it('字段下拉在内置项后追加自定义 codec', () => {
    const labels = fieldViewOptions(['Py']).map(item => item.label)
    expect(labels.at(-1)).toBe('Py')
    expect(fieldViewOptions(['Py']).at(-1)).toEqual({ label: 'Py', value: 'custom:Py' })
  })

  it('只读提示和写回会拒绝', () => {
    i18n.global.locale.value = 'en'
    expect(readonlyViewTip('pickle')).toBe('Pickle is view-only; saving back is not supported')
    expect(readonlyViewTip('phpserial')).toBe(
      'PhpSerial is view-only; saving back is not supported',
    )
    expect(readonlyViewTip('javaserial')).toBe(
      'JdkSerial is view-only; saving back is not supported',
    )
    expect(readonlyViewTip('vector32')).toBe('Vector32 is view-only; saving back is not supported')
    expect(readonlyViewTip('utf8')).toBe('')
    expect(() => meViewToWire('x', 'javaserial')).toThrow(/JdkSerial/)
    expect(() => meViewToWire('x', 'pickle')).toThrow(/Pickle/)
    expect(() => meViewToWire('x', 'phpserial')).toThrow(/PhpSerial/)
    expect(() => meViewToWire('[1, 2]', 'vector32')).toThrow(/Vector32/)
  })
})

describe('vector32', () => {
  function f32le(nums: number[]): string {
    const bytes = new Uint8Array(nums.length * 4)
    const view = new DataView(bytes.buffer)
    nums.forEach((n, i) => view.setFloat32(i * 4, n, true))
    let binary = ''
    for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i]!)
    return btoa(binary)
  }

  it('小端 FLOAT32 展示成数组，尾随 0 去掉', () => {
    const wire = f32le([1, -2.5, 0, 0.5])
    expect(meFormatViewValue(wire, 'vector32')).toBe('[1, -2.5, 0, 0.5]')
    expect(meVector32Base64ToDisplay(f32le([0.5]))).toBe('[0.5]')
  })

  it('长度不是 4 的倍数或含 NaN 时解码错误，不退回别的编码', () => {
    const badLen = btoa('abc')
    const bad = meVector32Base64ToDisplay(badLen)
    expect(bad.startsWith(decodeErrTitle('Vector32'))).toBe(true)
    expect(isViewDecodeError(bad)).toBe(true)
    const nan = meFormatViewValue(f32le([1, Number.NaN]), 'vector32')
    expect(nan.startsWith(decodeErrTitle('Vector32'))).toBe(true)
    expect(meVector32Base64ToDisplay('')).toBe('')
  })
})

describe('msgpack / binary', () => {
  it('msgpack 经 base64 wire 往返，JSON5 可写回', () => {
    const display = JSON.stringify({ a: 1, b: [true, null] }, null, 2)
    const wire = meViewToWire(display, 'msgpack')
    expect(meFormatViewValue(wire, 'msgpack')).toBe(display)
    expect(meMsgpackBase64ToJson(meJsonToMsgpackBase64('{a:1,}'))).toBe(
      JSON.stringify({ a: 1 }, null, 2),
    )
  })

  it('非法 msgpack 返回解码错误，不抛出', () => {
    const text = meMsgpackBase64ToJson('kQ==')
    expect(text.startsWith(decodeErrTitle('MsgPack'))).toBe(true)
    expect(isViewDecodeError(text)).toBe(true)
    expect(meMsgpackBase64ToJson('')).toBe('')
  })

  it('binary 往返；长度和字符非法时写回失败', () => {
    const wire = meViewToWire('A', 'utf8')
    const binary = meFormatViewValue(wire, 'binary')
    expect(binary).toBe('01000001')
    expect(meViewToWire(binary, 'binary')).toBe(wire)
    expect(meViewToWire('', 'binary')).toBe('')
    expect(meViewToWire('', 'msgpack')).toBe('')
    expect(() => meViewToWire('0000000', 'binary')).toThrow()
    expect(() => meViewToWire('0000000a', 'binary')).toThrow()
    expect(() => meViewToWire('abc', 'hex')).toThrow()
    expect(() => meViewToWire('zz', 'hex')).toThrow()
  })
})

describe('custom codec command', () => {
  it('短参数拼在命令行上，超长改走 stdin', () => {
    i18n.global.locale.value = 'en'
    const codec = { name: 'Py', command: '  python c.py  ' }
    expect(needsStdinInput('a'.repeat(CODEC_STDIN_B64_THRESHOLD - 1))).toBe(false)
    expect(needsStdinInput('a'.repeat(CODEC_STDIN_B64_THRESHOLD))).toBe(true)
    expect(buildCodecCommand(codec, 'decode', 'YQ==')).toBe('python c.py decode YQ==')
    expect(buildCodecCommand(codec, 'encode', 'a'.repeat(CODEC_STDIN_B64_THRESHOLD))).toBe(
      'python c.py encode --stdin',
    )
    expect(() => buildCodecCommand({ name: 'Py', command: '  ' }, 'decode', 'YQ==')).toThrow(
      'Command is required',
    )
  })

  it('从失败文案取出 Reason', () => {
    const message = 'Py Decode Error\nReason: bad\nline2\nScript: python c.py decode --stdin'
    expect(parseCodecErrorDetail(message)).toBe('bad\nline2')
    expect(parseCodecErrorDetail('plain failure')).toBe('plain failure')
  })

  it('按名称查找；同步格式化 custom 视图会返回错误文案', () => {
    const holder = globalThis as unknown as { window?: { meTauri?: MeTauriGlobal } }
    if (!holder.window) holder.window = {}
    const prev = holder.window.meTauri
    const settings: MeTauriGlobal['settings'] = {
      customCodecs: [{ name: 'Py', command: 'python c.py' }],
    }
    holder.window.meTauri = {
      connList: [],
      settings,
      systemTheme: 'light',
      systemLanguage: 'en',
      isAppStore: false,
    }
    try {
      expect(findCustomCodec('Py')?.command).toBe('python c.py')
      expect(findCustomCodec('Missing')).toBeUndefined()
      settings.customCodecs = undefined
      expect(findCustomCodec('Py')).toBeUndefined()
      settings.customCodecs = [{ name: 'Py', command: 'python c.py' }]
      const text = meFormatViewValue('YQ==', 'custom:Py')
      expect(isViewDecodeError(text)).toBe(true)
      expect(text).toContain('custom view requires meFormatViewValueAsync')
      expect(() => meViewToWire('hello', 'custom:Py')).toThrow(/meViewToWireAsync/)
    } finally {
      holder.window.meTauri = prev
    }
  })
})
