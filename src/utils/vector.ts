/**
 * Vector Set 向量输入：UI 接受 JSON 数组或空白/逗号分隔浮点；
 * 提交前归一为 number[]。空/零向量不拦截，交给 Redis VADD 原错。
 */

/** 解析编辑区文本 → 浮点数组；仅格式非法时失败 */
export function parseVectorInput(text: string): { ok: true; nums: number[] } | { ok: false } {
  const t = text.trim()
  if (!t) return { ok: true, nums: [] }

  let nums: number[]
  try {
    if (t.startsWith('[')) {
      const parsed: unknown = JSON.parse(t)
      if (!Array.isArray(parsed)) return { ok: false }
      nums = parsed.map(Number)
    } else {
      nums = t
        .split(/[\s,]+/)
        .filter(Boolean)
        .map(Number)
    }
  } catch {
    return { ok: false }
  }

  if (nums.some(n => !Number.isFinite(n))) return { ok: false }
  return { ok: true, nums }
}
