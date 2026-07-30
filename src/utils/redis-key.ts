/**
 * Redis 键身份：SCAN 对合法 UTF-8 可省略 bytes（空串），比对不能只用 bytes ===。
 * - 双方都有 bytes → 比 base64（二进制 / 旧数据）
 * - 双方都无 bytes → 比 key
 * - 仅一方有 bytes → 尝试把 base64 严格解成 UTF-8 再与另一方 key 比（兼容旧收藏）
 */
import { base64ToUtf8Text } from '@/utils/detect-view-format'
import { utf8TextToBase64 } from '@/utils/format'

export type RedisKeyLike = { key: string; bytes: string }

export function sameRedisKey(a: RedisKeyLike, b: RedisKeyLike): boolean {
  if (a.bytes && b.bytes) return a.bytes === b.bytes
  if (!a.bytes && !b.bytes) return a.key === b.key
  const withBytes = a.bytes ? a : b
  const without = a.bytes ? b : a
  const decoded = base64ToUtf8Text(withBytes.bytes)
  return decoded !== null && decoded === without.key
}

/**
 * 稳定缓存/集合 id：合法 UTF-8（含旧数据带 base64）统一为 `k\0${text}`，
 * 仅非法 UTF-8 用 `b\0${base64}`，避免省略 bytes 前后缓存键分裂。
 */
export function redisKeyId(rk: RedisKeyLike): string {
  if (rk.bytes) {
    const decoded = base64ToUtf8Text(rk.bytes)
    if (decoded !== null) return `k\0${decoded}`
    return `b\0${rk.bytes}`
  }
  return `k\0${rk.key}`
}

/** 重命名/复制切 hex 等编码时：有 bytes 用 bytes，否则用 key 的 UTF-8 wire */
export function redisKeyWireBase64(rk: RedisKeyLike): string {
  return rk.bytes || utf8TextToBase64(rk.key)
}
