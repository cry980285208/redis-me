/**
 * STRING 值 Auto 编码识别：基于 base64 wire 原始字节。
 * 优先级：JavaSerial(ACED) → Pickle(PROTO 0x80) → PhpSerial(a:/O:/C:) → MsgPack → StrJson → UTF-8 → Hex。
 * JavaSerial/Pickle/PhpSerial：特征前缀 + 全量试解，失败则继续下一种（展示层再解析一遍）。
 * MsgPack/StrJson/PhpSerial：仅 ≤ DETECT_TRY_MAX_BYTES 时试解。
 */
import { decode } from '@msgpack/msgpack'
import JSON5 from 'json5'

import { javaSerBase64ToValue } from '@/utils/javaserial'
import { phpSerialBase64ToValue } from '@/utils/phpserial'
import { pickleBase64ToValue } from '@/utils/pickle'

/** 超过此字节数跳过 MsgPack / StrJson / PhpSerial 试解 */
export const DETECT_TRY_MAX_BYTES = 512 * 1024

/** Auto 识别结果（不含 auto / binary / base64 / custom） */
export type DetectedViewFormat =
  | 'javaserial'
  | 'pickle'
  | 'phpserial'
  | 'msgpack'
  | 'strjson'
  | 'utf8'
  | 'hex'

const JAVA_STREAM_MAGIC_0 = 0xac
const JAVA_STREAM_MAGIC_1 = 0xed
/** Pickle PROTO opcode；下一字节为协议号（常见 0–5） */
const PICKLE_PROTO = 0x80
const PICKLE_PROTO_MAX = 5
/** java 序列化流：magic(2) + version(2)，至少 4 字节 */
const JAVA_STREAM_MIN_LEN = 4

const DETECTED_LABELS: Record<DetectedViewFormat, string> = {
  javaserial: 'JavaSerial',
  pickle: 'Pickle',
  phpserial: 'PhpSerial',
  msgpack: 'MsgPack',
  strjson: 'StrJson',
  utf8: 'UTF8',
  hex: 'Hex',
}

export function detectedViewLabel(view: DetectedViewFormat): string {
  return DETECTED_LABELS[view]
}

function base64ToBytes(base64: string): Uint8Array | null {
  if (!base64) return new Uint8Array()
  try {
    const binary = atob(base64)
    const bytes = new Uint8Array(binary.length)
    for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i)
    return bytes
  } catch {
    return null
  }
}

/** base64 wire → UTF-8 文本；无效序列返回 null */
export function base64ToUtf8Text(base64: string): string | null {
  const bytes = base64ToBytes(base64)
  if (!bytes) return null
  try {
    return new TextDecoder('utf-8', { fatal: true }).decode(bytes)
  } catch {
    return null
  }
}

function isDisplayableUtf8(text: string): boolean {
  if (!text) return true
  let control = 0
  for (let i = 0; i < text.length; i++) {
    const c = text.charCodeAt(i)
    // 允许常见空白；其余 C0/C1 控制符计为不可展示
    if (c === 0x09 || c === 0x0a || c === 0x0d) continue
    if (c < 0x20 || (c >= 0x7f && c < 0xa0)) control++
  }
  // 控制符过多则倾向 Hex
  return control / text.length < 0.1
}

/** 魔数 + 全量试解；失败不认（避免截断/坏数据锁死 Decode Error） */
function looksLikeJavaSerial(base64: string, bytes: Uint8Array): boolean {
  if (bytes.length < JAVA_STREAM_MIN_LEN) return false
  if (bytes[0] !== JAVA_STREAM_MAGIC_0 || bytes[1] !== JAVA_STREAM_MAGIC_1) return false
  try {
    javaSerBase64ToValue(base64)
    return true
  } catch {
    return false
  }
}

/**
 * 魔数（PROTO + 协议号）+ 全量试解。
 * 单字节 0x80（MsgPack 空 map）长度不足，不会误判为 Pickle。
 */
function looksLikePickle(base64: string, bytes: Uint8Array): boolean {
  if (bytes.length < 2) return false
  if (bytes[0] !== PICKLE_PROTO || bytes[1]! > PICKLE_PROTO_MAX) return false
  try {
    pickleBase64ToValue(base64)
    return true
  } catch {
    return false
  }
}

/**
 * 复合根（a:/O:/C:）+ 全量试解；标量根（s:/i: 等）与常见文本易撞，不参与 Auto。
 */
function looksLikePhpSerial(base64: string, bytes: Uint8Array): boolean {
  if (bytes.length < 5) return false
  const c0 = bytes[0]
  if (c0 !== 0x61 /* a */ && c0 !== 0x4f /* O */ && c0 !== 0x43 /* C */) return false
  if (bytes[1] !== 0x3a /* : */) return false
  try {
    phpSerialBase64ToValue(base64)
    return true
  } catch {
    return false
  }
}

/** 保守：仅根为 object/array 才认 MsgPack，避免短文本误判 */
function looksLikeMsgpack(bytes: Uint8Array): boolean {
  try {
    const decoded = decode(bytes)
    if (decoded === null || typeof decoded !== 'object') return false
    return true
  } catch {
    return false
  }
}

/** 双层 JSON 字符串：wire parse → string → 再 parse 为 object/array */
function looksLikeStrJson(utf8: string): boolean {
  try {
    const outer = JSON5.parse(utf8.trim())
    if (typeof outer !== 'string') return false
    const inner = JSON5.parse(outer.trim())
    return inner !== null && typeof inner === 'object'
  } catch {
    return false
  }
}

/**
 * 从 base64 wire 识别展示格式。
 * 空值 → utf8；大 value 跳过 MsgPack/StrJson 试解。
 */
export function detectViewFormat(base64: string): DetectedViewFormat {
  if (!base64) return 'utf8'

  const bytes = base64ToBytes(base64)
  if (!bytes) return 'hex'

  if (looksLikeJavaSerial(base64, bytes)) return 'javaserial'
  if (looksLikePickle(base64, bytes)) return 'pickle'

  const allowTry = bytes.length <= DETECT_TRY_MAX_BYTES
  if (allowTry && looksLikePhpSerial(base64, bytes)) return 'phpserial'
  if (allowTry && looksLikeMsgpack(bytes)) return 'msgpack'

  const utf8 = base64ToUtf8Text(base64)
  if (utf8 !== null) {
    if (allowTry && looksLikeStrJson(utf8)) return 'strjson'
    if (isDisplayableUtf8(utf8)) return 'utf8'
  }

  return 'hex'
}
