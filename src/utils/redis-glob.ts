/** Redis SCAN MATCH 通配符：* ? [ */
export function isRedisGlob(pattern: string): boolean {
  return /[*?[]/.test(pattern)
}

/** 转义 minimatch 特殊字符，精确模式按字面键名过滤 */
export function escapeMinimatchLiteral(s: string): string {
  return s.replace(/[\\?*[\]{}()!@+]/g, '\\$&')
}

/** 本地 minimatch 过滤（与 KeyMain / RedisValue 表格筛选一致） */
export const MINIMATCH_SCAN_OPTS = {
  nobrace: true,
  noglobstar: true,
  noext: true,
  nocase: true,
} as const

/**
 * 未 Enter 重扫时的本地过滤 pattern：exact 转义字面，否则用服务端 match。
 */
export function buildLocalFilterPattern(keyword: string, exact: boolean, match: string): string {
  const key = keyword.trim()
  if (!key) return ''
  if (exact) return escapeMinimatchLiteral(key)
  return match
}

/** 与后端 scan_0_batch_count 一致：pattern 去 * 后 ≤1 字符 COUNT=1000，否则 10000 */
export function computeScanBatchSize(match: string): number {
  const stripped = match.replace(/\*/g, '')
  return stripped.length <= 1 ? 1000 : 10000
}

/** 扫描进度环：按批次估算，finished 时 100% */
export function computeScanProgress(
  batchCount: number,
  batchSize: number,
  totalEstimate: number,
  finished: boolean,
): number {
  if (finished) return 100
  if (batchCount === 0) return 0
  if (totalEstimate > 0) {
    return Math.min(99, Math.round(((batchCount * batchSize) / totalEstimate) * 100))
  }
  return Math.min(99, batchCount * 5)
}

/**
 * 构建 SCAN 模式（目录扫描 loadFolder 时固定为 keyword:*）
 *
 * 关闭完全匹配：含 glob（* ? [）则原样，否则前后补 *
 * 开启完全匹配：原样传给后端 EXISTS 判断（含 * 也按字面键名）
 */
export function buildScanPattern(keyword: string, exact: boolean, loadFolder = false): string {
  if (loadFolder) return `${keyword}:*`
  if (exact) return keyword
  if (!keyword) return '*'
  if (isRedisGlob(keyword)) return keyword
  return `*${keyword}*`
}
