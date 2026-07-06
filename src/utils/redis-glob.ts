/** Redis SCAN MATCH 通配符：* ? [ */
export function isRedisGlob(pattern: string): boolean {
  return /[*?[]/.test(pattern)
}

/** 转义 minimatch 特殊字符，精确模式按字面键名过滤 */
export function escapeMinimatchLiteral(s: string): string {
  return s.replace(/[\\?*[\]{}()!@+]/g, '\\$&')
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
