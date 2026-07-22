/** 值/键视图格式与 wire(utf8/base64) 编解码；auto/hex/msgpack/strjson/javaserial/pickle/custom 在前端，custom 走 shell 脚本 */

import { decode, encode } from '@msgpack/msgpack'
import { isTauri } from '@tauri-apps/api/core'
import { type } from '@tauri-apps/plugin-os'
import { Command } from '@tauri-apps/plugin-shell'
import JSON5 from 'json5'

import i18n from '@/locales'
import type { BytesFormat } from '@/types/tauri-specta'
import { formatJavaSerDisplay, javaSerBase64ToValue } from '@/utils/javaserial'
import { formatPickleDisplay, pickleBase64ToValue } from '@/utils/pickle'

const t = i18n.global.t

// #region 自定义 Codec（shell 脚本 decode/encode）

/** Base64 参数超过此长度时改走 stdin（Windows cmd 命令行约 8191 字符上限） */
export const CODEC_STDIN_B64_THRESHOLD = 8000
const STDIN_ARG = '--stdin'

/** 持久化于 settings.customCodecs；CustomCodec.vue CRUD */
export interface CustomCodec {
  name: string
  /** 可执行入口，如 `python3 /path/codec.py` */
  command: string
}

export type CodecMode = 'decode' | 'encode'

interface ShellExecResult {
  code: number
  stdout: string
  stderr: string
}

const B64_RE = /^[A-Za-z0-9+/]+=*$/

function isValidBase64(s: string): boolean {
  return B64_RE.test(s)
}

function textUtf8ToBase64(text: string): string {
  const bytes = new TextEncoder().encode(text)
  let binary = ''
  for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i]!)
  return btoa(binary)
}

/** 按名称查 settings.customCodecs；meFormatViewValueAsync / meViewToWireAsync 内部 */
export function findCustomCodec(name: string): CustomCodec | undefined {
  const list = window.meTauri.settings.customCodecs
  if (!Array.isArray(list)) return undefined
  return list.find(f => f.name === name)
}

function getExecTimeoutSec(): number {
  const n = window.meTauri.settings.codecExecTimeoutSec
  return typeof n === 'number' && n > 0 ? n : 5
}

export function needsStdinInput(b64: string): boolean {
  return b64.length >= CODEC_STDIN_B64_THRESHOLD
}

/** 拼完整命令行；CustomCodec.vue 测试弹窗展示用 */
export function buildCodecCommand(codec: CustomCodec, mode: CodecMode, b64: string): string {
  const cmd = codec.command.trim()
  if (!cmd) throw new Error(t('customCodec.emptyCommand'))
  if (needsStdinInput(b64)) return `${cmd} ${mode} ${STDIN_ARG}`
  return `${cmd} ${mode} ${b64}`
}

function formatExecError(name: string, result: ShellExecResult, fullCommand: string): string {
  const err = result.stderr?.trim() || result.stdout?.trim()
  let detail: string
  if (err) {
    detail = err
  } else if (result.code !== 0) {
    detail = t('customCodec.execFailed', { name, code: result.code })
  } else {
    detail = t('customCodec.invalidOutput', { name })
  }
  return withExecCommand(fullCommand, detail)
}

function withExecCommand(fullCommand: string, message: string): string {
  return t('customCodec.execFailResult', { command: fullCommand, detail: message })
}

/** 从 execFailResult 错误里取 detail；CustomCodec.vue 展示 */
export function parseCodecErrorDetail(message: string): string {
  const headPrefixes = ['⚠️ 错误：', '⚠️ Error: ']
  for (const prefix of headPrefixes) {
    if (!message.startsWith(prefix)) continue
    const rest = message.slice(prefix.length)
    for (const m of ['\n🔔 命令：', '\n🔔 Command: ']) {
      const i = rest.indexOf(m)
      if (i >= 0) return rest.slice(0, i).trim()
    }
    return rest.trim()
  }
  return message.trim()
}

function toExecError(fullCommand: string, e: unknown): Error {
  const detail = e instanceof Error ? e.message : String(e)
  return new Error(withExecCommand(fullCommand, detail))
}

function createExecCommand(fullCommand: string) {
  if (type() === 'windows') {
    return Command.create('exec-cmd', ['/C', fullCommand])
  }
  return Command.create('exec-sh', ['-c', fullCommand])
}

function execTimeoutPromise(
  codecName: string,
  timeoutSec: number,
): { promise: Promise<never>; clear: () => void } {
  let timer: ReturnType<typeof setTimeout> | undefined
  const promise = new Promise<never>((_, reject) => {
    timer = setTimeout(
      () => reject(new Error(t('customCodec.timeout', { name: codecName, sec: timeoutSec }))),
      timeoutSec * 1000,
    )
  })
  return {
    promise,
    clear: () => {
      if (timer) clearTimeout(timer)
    },
  }
}

async function execShell(fullCommand: string, codecName: string): Promise<ShellExecResult> {
  if (!isTauri()) {
    throw new Error(t('customCodec.shellUnavailable'))
  }
  const timeoutSec = getExecTimeoutSec()
  const cmd = createExecCommand(fullCommand)
  const timeout = execTimeoutPromise(codecName, timeoutSec)
  try {
    const result = await Promise.race([cmd.execute(), timeout.promise])
    return { code: result.code ?? 0, stdout: result.stdout ?? '', stderr: result.stderr ?? '' }
  } catch (e) {
    throw toExecError(fullCommand, e)
  } finally {
    timeout.clear()
  }
}

async function execShellWithStdin(
  fullCommand: string,
  b64: string,
  codecName: string,
): Promise<ShellExecResult> {
  if (!isTauri()) {
    throw new Error(t('customCodec.shellUnavailable'))
  }
  const timeoutSec = getExecTimeoutSec()
  const cmd = createExecCommand(fullCommand)
  const stdoutChunks: string[] = []
  const stderrChunks: string[] = []

  const closePromise = new Promise<{ code: number | null }>((resolve, reject) => {
    cmd.on('close', data => resolve({ code: data.code }))
    cmd.on('error', err => reject(new Error(String(err))))
    cmd.stdout.on('data', line => stdoutChunks.push(String(line)))
    cmd.stderr.on('data', line => stderrChunks.push(String(line)))
  })

  const timeout = execTimeoutPromise(codecName, timeoutSec)
  let child: Awaited<ReturnType<typeof cmd.spawn>> | undefined
  let finished = false
  try {
    child = await cmd.spawn()
    await child.write(`${b64}\n`)
    const closed = await Promise.race([closePromise, timeout.promise])
    finished = true
    return {
      code: closed.code ?? 0,
      stdout: stdoutChunks.join('\n'),
      stderr: stderrChunks.join('\n'),
    }
  } catch (e) {
    throw toExecError(fullCommand, e)
  } finally {
    timeout.clear()
    if (!finished) await child?.kill().catch(() => undefined)
  }
}

async function execCodec(
  codec: CustomCodec,
  mode: CodecMode,
  b64: string,
  kind: 'decode' | 'encode' | 'test',
): Promise<string> {
  if (kind === 'decode' && !b64) return ''
  const fullCommand = buildCodecCommand(codec, mode, b64)
  const result = needsStdinInput(b64)
    ? await execShellWithStdin(fullCommand, b64, codec.name)
    : await execShell(fullCommand, codec.name)
  const out = kind === 'encode' ? result.stdout.trim() : result.stdout.trimEnd()
  if (result.code !== 0) {
    throw new Error(formatExecError(codec.name, result, fullCommand))
  }
  if (kind !== 'test' && !out) {
    const msg =
      mode === 'decode'
        ? t('customCodec.decodeEmpty', { name: codec.name })
        : t('customCodec.encodeEmpty', { name: codec.name })
    throw new Error(withExecCommand(fullCommand, msg))
  }
  if (kind === 'encode' && out && !isValidBase64(out)) {
    throw new Error(
      withExecCommand(fullCommand, t('customCodec.encodeNotBase64', { name: codec.name })),
    )
  }
  return out
}

/** wire base64 → 展示文本；meFormatViewValueAsync 调用 */
export async function runDecode(wireBase64: string, codec: CustomCodec): Promise<string> {
  return execCodec(codec, 'decode', wireBase64, 'decode')
}

/** 编辑区文本 → wire base64；meViewToWireAsync 调用 */
export async function runEncode(editorText: string, codec: CustomCodec): Promise<string> {
  return execCodec(codec, 'encode', textUtf8ToBase64(editorText), 'encode')
}

/** 弹窗内测试 decode / encode；CustomCodec.vue */
export async function testCodec(
  codec: CustomCodec,
  mode: CodecMode,
  sampleBase64: string,
): Promise<string> {
  return execCodec(codec, mode, sampleBase64, 'test')
}

// #endregion

// #region 视图格式与 wire 转换

/** 键重命名等基础字节视图；KeyRename、FieldAdd */
export const BYTES_FORMAT = ['UTF8', 'Hex', 'Binary', 'Base64'] as const

/** 前端值/键展示格式；RedisValue 数据编码下拉（auto 仅 STRING，拉取 base64 后前端识别） */
export type ViewBytesFormat =
  | 'auto'
  | 'utf8'
  | 'hex'
  | 'binary'
  | 'base64'
  | 'msgpack'
  | 'strjson'
  | 'javaserial'
  | 'pickle'
  | `custom:${string}`

export const CUSTOM_FORMAT_PREFIX = 'custom:' as const

export function isCustomView(view: ViewBytesFormat): view is `custom:${string}` {
  return view.startsWith(CUSTOM_FORMAT_PREFIX)
}

/** custom 下拉项 value：`custom:${name}`；RedisValue 下拉 */
export function customFormatValue(name: string): ViewBytesFormat {
  return `${CUSTOM_FORMAT_PREFIX}${name}`
}

/** 从 custom view 解析名称；RedisValue、FieldSet */
export function customFormatName(view: ViewBytesFormat): string | null {
  return isCustomView(view) ? view.slice(CUSTOM_FORMAT_PREFIX.length) : null
}

/** 仅整键 STRING 可选（Auto、MsgPack、StrJson、JavaSerial、Pickle、custom）；RedisValue 下拉过滤 */
export function isStringOnlyView(view: ViewBytesFormat): boolean {
  return (
    view === 'auto' ||
    view === 'msgpack' ||
    view === 'strjson' ||
    view === 'javaserial' ||
    view === 'pickle' ||
    isCustomView(view)
  )
}

/** 内置只读视图（不可写回）；RedisValue canSave */
export function isReadonlyView(view: ViewBytesFormat): boolean {
  return view === 'javaserial' || view === 'pickle'
}

function resolveCustomCodec(view: ViewBytesFormat): CustomCodec {
  const name = customFormatName(view)
  if (!name) throw new Error(t('customCodec.notFound', { name: view }))
  const codec = findCustomCodec(name)
  if (!codec) throw new Error(t('customCodec.notFound', { name }))
  return codec
}

/** STRING 值详情下拉扩展项（不含 Auto，Auto 单独置顶）；RedisValue */
export const EXT_FORMAT = ['StrJson', 'MsgPack', 'JavaSerial', 'Pickle'] as const

export const MSGPACK_DECODE_ERR = '⚠️ MsgPack Decode Error'
export const STRJSON_DECODE_ERR = '⚠️ StrJson Decode Error'
export const JAVASERIAL_DECODE_ERR = '⚠️ JavaSerial Decode Error'
export const PICKLE_DECODE_ERR = '⚠️ Pickle Decode Error'

/** 展示文本是否为内置解码失败；RedisValue、FieldSet 保存校验 */
export function isViewDecodeError(text: string): boolean {
  return (
    text.startsWith(MSGPACK_DECODE_ERR) ||
    text.startsWith(STRJSON_DECODE_ERR) ||
    text.startsWith(JAVASERIAL_DECODE_ERR) ||
    text.startsWith(PICKLE_DECODE_ERR)
  )
}

/** 视图格式 → 后端 wire；RedisValue / FieldSet / FieldAdd 读写 Redis（auto → base64） */
export function toWireFormat(view: ViewBytesFormat): BytesFormat {
  return view === 'utf8' || view === 'strjson' ? 'utf8' : 'base64'
}

/** 字段弹窗：Auto / MsgPack / custom 等不适用，降级 utf8；RedisValue fieldScan */
export function viewFmtForField(view: ViewBytesFormat): ViewBytesFormat {
  return isStringOnlyView(view) ? 'utf8' : view
}

export type FieldViewOption = { label: string; value: ViewBytesFormat }

/** 字段编辑下拉选项；FieldSet */
export function fieldViewOptions(
  keyWireFmt: BytesFormat,
  customNames: string[] = [],
): FieldViewOption[] {
  if (keyWireFmt === 'utf8') {
    return [
      { label: 'UTF8', value: 'utf8' },
      { label: 'StrJson', value: 'strjson' },
    ]
  }
  const opts: FieldViewOption[] = BYTES_FORMAT.filter(label => label !== 'UTF8').map(label => ({
    label,
    value: label.toLowerCase() as ViewBytesFormat,
  }))
  opts.push({ label: 'MsgPack', value: 'msgpack' })
  for (const name of customNames) {
    opts.push({ label: name, value: customFormatValue(name) })
  }
  return opts
}

/** 字段编辑默认 view；FieldSet 打开弹窗 */
export function defaultFieldViewFmt(
  keyView: ViewBytesFormat,
  keyWireFmt: BytesFormat,
): ViewBytesFormat {
  const options = fieldViewOptions(keyWireFmt)
  if (options.some(o => o.value === keyView)) return keyView
  return options[0]!.value
}

/** 保存前需 JSON compact；FieldSet / RedisValue */
export function needsJsonNormalize(view: ViewBytesFormat): boolean {
  return view === 'msgpack' || view === 'strjson'
}

/** wire → 编辑器/表格展示（同步）；RedisValue、FieldSet */
export function meFormatViewValue(wire: string, view: ViewBytesFormat): string {
  if (!wire || view === 'utf8') return wire
  if (view === 'base64') return wire
  if (view === 'hex' || view === 'binary') return meFormatBytes(wire, view)
  if (view === 'msgpack') return meMsgpackBase64ToJson(wire)
  if (view === 'strjson') return meStrJsonWireToDisplay(wire)
  if (view === 'javaserial') return meJavaSerialBase64ToDisplay(wire)
  if (view === 'pickle') return mePickleBase64ToDisplay(wire)
  if (isCustomView(view)) {
    throw new Error('custom view requires meFormatViewValueAsync')
  }
  return wire
}

/** wire → 展示（含 custom）；RedisValue refreshKey、FieldSet */
export async function meFormatViewValueAsync(wire: string, view: ViewBytesFormat): Promise<string> {
  if (!wire || view === 'utf8') return wire
  if (isCustomView(view)) {
    return runDecode(wire, resolveCustomCodec(view))
  }
  return meFormatViewValue(wire, view)
}

/** 编辑区 → wire（同步）；RedisValue、FieldSet、FieldAdd */
export function meViewToWire(text: string, view: ViewBytesFormat): string {
  if (!text || view === 'utf8') return text
  if (view === 'base64') return text
  if (view === 'hex' || view === 'binary') return meToBase64(text, view)
  if (view === 'msgpack') return meJsonToMsgpackBase64(text)
  if (view === 'strjson') return meDisplayToStrJsonWire(text)
  if (view === 'javaserial') return meDisplayToJavaSerialBase64(text)
  if (view === 'pickle') return meDisplayToPickleBase64(text)
  if (isCustomView(view)) {
    throw new Error('custom view requires meViewToWireAsync')
  }
  return text
}

/** 编辑区 → wire（含 custom）；RedisValue 保存、FieldSet */
export async function meViewToWireAsync(text: string, view: ViewBytesFormat): Promise<string> {
  if (!text || view === 'utf8') return text
  if (isCustomView(view)) {
    return runEncode(text, resolveCustomCodec(view))
  }
  return meViewToWire(text, view)
}

/** base64 wire → hex/base64 展示；KeyRename、meFormatViewValue */
export function meFormatBytes(base64: string, bytesFormat: string): string {
  if (bytesFormat === 'base64') return base64
  if (bytesFormat === 'hex') return base64ToHex(base64)
  if (bytesFormat === 'binary') return base64ToBinary(base64)
  return 'Unknown bytesFormat: ' + bytesFormat
}

/** hex/base64 输入 → base64 wire；KeyRename、meViewToWire */
export function meToBase64(bytes: string, encoding: string): string {
  if (encoding === 'base64') return bytes
  if (encoding === 'hex') return hexToBase64(bytes)
  if (encoding === 'binary') return binaryToBase64(bytes)
  return 'Unknown encoding: ' + encoding
}

function base64ToHex(base64: string): string {
  if (!base64) return ''
  const binary = atob(base64)
  return Array.from(binary)
    .map(char => char.charCodeAt(0).toString(16).padStart(2, '0'))
    .join('')
}

function base64ToBinary(base64: string): string {
  if (!base64) return ''
  const binary = atob(base64)
  return Array.from(binary)
    .map(char => char.charCodeAt(0).toString(2).padStart(8, '0'))
    .join('')
}

function hexToBase64(hex: string): string {
  if (!hex) return ''
  if (hex.length % 2 !== 0) {
    throw new Error(t('util.invalidHexString'))
  }
  if (!/^[0-9a-fA-F]+$/.test(hex)) {
    throw new Error(t('util.invalidHexCharacter'))
  }
  const bytes: number[] = []
  for (let i = 0; i < hex.length; i += 2) {
    const byte = Number.parseInt(hex.slice(i, i + 2), 16)
    if (Number.isNaN(byte)) {
      throw new Error(t('util.invalidHexCharacter'))
    }
    bytes.push(byte)
  }
  const binary = bytes.map(b => String.fromCharCode(b)).join('')
  return btoa(binary)
}

function binaryToBase64(binary: string): string {
  if (!binary) return ''
  if (binary.length % 8 !== 0) {
    throw new Error(t('util.invalidBinaryString'))
  }
  if (!/^[01]+$/.test(binary)) {
    throw new Error(t('util.invalidBinaryCharacter'))
  }
  const bytes: number[] = []
  for (let i = 0; i < binary.length; i += 8) {
    const byte = Number.parseInt(binary.slice(i, i + 8), 2)
    if (Number.isNaN(byte)) {
      throw new Error(t('util.invalidBinaryCharacter'))
    }
    bytes.push(byte)
  }
  const binaryStr = bytes.map(b => String.fromCharCode(b)).join('')
  return btoa(binaryStr)
}

function base64ToUint8Array(base64: string): Uint8Array {
  const binary = atob(base64)
  const bytes = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i)
  return bytes
}

function uint8ArrayToBase64(bytes: Uint8Array): string {
  let binary = ''
  for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i]!)
  return btoa(binary)
}

export function meMsgpackBase64ToJson(base64: string): string {
  if (!base64) return ''
  try {
    const decoded = decode(base64ToUint8Array(base64))
    return JSON.stringify(decoded, null, 2)
  } catch {
    return `${MSGPACK_DECODE_ERR}\n${base64}`
  }
}

export function meJsonToMsgpackBase64(json: string): string {
  const v = JSON5.parse(json.trim())
  return uint8ArrayToBase64(encode(v))
}

function unwrapStrJsonValue(wire: string): unknown {
  const parsed = JSON5.parse(wire.trim())
  if (typeof parsed !== 'string') {
    throw new Error('StrJson wire is not a JSON string wrapper')
  }
  return JSON5.parse(parsed.trim())
}

export function meStrJsonWireToDisplay(wire: string): string {
  if (!wire) return ''
  try {
    const value = unwrapStrJsonValue(wire)
    return JSON.stringify(value, null, 2)
  } catch {
    return `${STRJSON_DECODE_ERR}\n${wire}`
  }
}

export function meDisplayToStrJsonWire(text: string): string {
  const value = JSON5.parse(text.trim())
  return JSON.stringify(JSON.stringify(value))
}

export function meJavaSerialBase64ToDisplay(base64: string): string {
  if (!base64) return ''
  try {
    return formatJavaSerDisplay(javaSerBase64ToValue(base64))
  } catch (e) {
    const detail = e instanceof Error ? e.message : String(e)
    return `${JAVASERIAL_DECODE_ERR}\n${detail}\n${base64}`
  }
}

/** JavaSerial 只读（与 RedisInsight / AnotherRDM 一致），不支持写回 */
export function meDisplayToJavaSerialBase64(_text: string): string {
  throw new Error(t('util.javaSerialReadonly'))
}

export function mePickleBase64ToDisplay(base64: string): string {
  if (!base64) return ''
  try {
    return formatPickleDisplay(pickleBase64ToValue(base64))
  } catch (e) {
    const detail = e instanceof Error ? e.message : String(e)
    return `${PICKLE_DECODE_ERR}\n${detail}\n${base64}`
  }
}

/** Pickle 只读（与 JavaSerial 一致），不支持写回 */
export function meDisplayToPickleBase64(_text: string): string {
  throw new Error(t('util.pickleReadonly'))
}

// #endregion
