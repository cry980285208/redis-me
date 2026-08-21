// 校验 locales 下 info、config、client 及 cmd 的中英文 key 是否一致
// 运行：node scripts/check-locale-keys.mjs  或  vp run check-locale-keys
//
// 规则：
// - 「仅中文有、英文没有」→ 始终 ERROR 退出 1
// - 「仅英文有、中文没有」→ cmd 始终 ERROR；info/config/client 默认 WARN（缺译可渐进补）
// - 加 --strict-full：上述「仅英文有」也一律 ERROR（CI 全量对齐用）

import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const root = path.join(__dirname, '..')

function read(p) {
  return fs.readFileSync(path.join(root, p), 'utf8')
}

/** 从 `export const name = {` 起截取完整 `{ ... }`（略过字符串/模板内的花括号） */
function extractExportedObject(text, exportName) {
  const prefix = `export const ${exportName}`
  const start = text.indexOf(prefix)
  if (start < 0) throw new Error(`missing export: ${exportName}`)
  let i = text.indexOf('{', start)
  if (i < 0) throw new Error(`missing { for ${exportName}`)
  const objStart = i
  let depth = 0
  let inTemplate = false
  let inSingle = false
  let inDouble = false
  for (; i < text.length; i++) {
    const c = text[i]
    if (inTemplate) {
      if (c === '\\' && i + 1 < text.length) {
        i++
        continue
      }
      if (c === '`') inTemplate = false
      continue
    }
    if (inSingle) {
      if (c === '\\' && i + 1 < text.length) {
        i++
        continue
      }
      if (c === "'") inSingle = false
      continue
    }
    if (inDouble) {
      if (c === '\\' && i + 1 < text.length) {
        i++
        continue
      }
      if (c === '"') inDouble = false
      continue
    }
    if (c === '`') {
      inTemplate = true
      continue
    }
    if (c === "'") {
      inSingle = true
      continue
    }
    if (c === '"') {
      inDouble = true
      continue
    }
    if (c === '{') depth++
    if (c === '}') {
      depth--
      if (depth === 0) return text.slice(objStart, i + 1)
    }
  }
  throw new Error(`unclosed object for ${exportName}`)
}

/** tip 文件：顶层 key 为 `id:` 或 `'a-b':`（两空格缩进） */
function extractTipKeys(objectLiteral) {
  const inner = objectLiteral.slice(1, -1)
  const keys = []
  const re = /^  (?:([a-zA-Z_]\w*)|'((?:\\.|[^'\\])*)')\s*:/gm
  let m
  while ((m = re.exec(inner)) !== null) {
    keys.push(m[1] ?? m[2].replace(/\\(.)/g, '$1'))
  }
  return keys
}

/** cmd zh-cn：`  'KEY':` 或合法标识符键 `  ACL:` */
function extractCmdZhKeys(objectLiteral) {
  const inner = objectLiteral.slice(1, -1)
  const keys = []
  const re = /^  (?:'((?:\\.|[^'\\])*)'|([A-Za-z_]\w*))\s*:/gm
  let m
  while ((m = re.exec(inner)) !== null) {
    keys.push(m[1] != null ? m[1].replace(/\\(.)/g, '$1') : m[2])
  }
  return keys
}

/** cmd en.ts：`key: 'FOO',` */
function extractCmdEnKeys(text) {
  const keys = []
  const re = /^\s+key:\s*'((?:\\.|[^'\\])*)'\s*,?\s*$/gm
  let m
  while ((m = re.exec(text)) !== null) {
    keys.push(m[1].replace(/\\(.)/g, '$1'))
  }
  return keys
}

const strictFull = process.argv.includes('--strict-full')

/**
 * @param {string} name
 * @param {string[]} zhKeys
 * @param {string[]} enKeys
 * @param {{ missingEnIsError?: boolean }} [opt]
 * @returns {{ errors: number, warns: number }}
 */
function compareSets(name, zhKeys, enKeys, opt = {}) {
  const missingEnIsError = opt.missingEnIsError ?? false
  const zh = new Set(zhKeys)
  const en = new Set(enKeys)
  const onlyZh = [...zh].filter(k => !en.has(k)).sort()
  const onlyEn = [...en].filter(k => !zh.has(k)).sort()
  let errors = 0
  let warns = 0
  if (onlyZh.length === 0 && onlyEn.length === 0) {
    console.log(`ok  ${name}  (${zh.size} keys)`)
    return { errors: 0, warns: 0 }
  }
  if (onlyZh.length) {
    console.error(`ERROR  ${name}`)
    console.error(
      `  仅中文侧有（英文缺键，多为拼写错误）: ${onlyZh.length}\n    ${onlyZh.join('\n    ')}`,
    )
    errors += onlyZh.length
  }
  if (onlyEn.length) {
    if (strictFull || missingEnIsError) {
      console.error(`ERROR  ${name}${strictFull ? ' (strict-full)' : ''}`)
      console.error(`  仅英文侧有（中文缺译）: ${onlyEn.length}\n    ${onlyEn.join('\n    ')}`)
      errors += onlyEn.length
    } else {
      console.warn(`WARN  ${name}`)
      console.warn(`  仅英文侧有（中文未翻译，可渐进补齐）: ${onlyEn.length}`)
      if (onlyEn.length <= 20) console.warn(`    ${onlyEn.join('\n    ')}`)
      else console.warn(`    （共 ${onlyEn.length} 条，略）首条: ${onlyEn[0]}`)
      warns += onlyEn.length
    }
  }
  if (onlyZh.length === 0 && onlyEn.length > 0 && !strictFull && !missingEnIsError) {
    console.log(`ok* ${name}  (${zh.size} keys, ${onlyEn.length} 条英文独有 → 见 WARN)`)
  }
  return { errors, warns }
}

let totalErrors = 0
let totalWarns = 0

for (const { dir, zhExport, enExport } of [
  { dir: 'info', zhExport: 'zhInfoTip', enExport: 'enInfoTip' },
  { dir: 'config', zhExport: 'zhConfigTip', enExport: 'enConfigTip' },
  { dir: 'client', zhExport: 'zhClientTip', enExport: 'enClientTip' },
]) {
  const zhPath = `src/locales/${dir}/zh-cn.ts`
  const enPath = `src/locales/${dir}/en.ts`
  const zhText = read(zhPath)
  const enText = read(enPath)
  const zhKeys = extractTipKeys(extractExportedObject(zhText, zhExport))
  const enKeys = extractTipKeys(extractExportedObject(enText, enExport))
  const r = compareSets(`${dir}  ${zhExport} vs ${enExport}`, zhKeys, enKeys)
  totalErrors += r.errors
  totalWarns += r.warns
}

{
  const zhText = read('src/locales/cmd/zh-cn.ts')
  const enText = read('src/locales/cmd/en.ts')
  const zhKeys = extractCmdZhKeys(extractExportedObject(zhText, 'redisCmdSummaryZhCn'))
  const enKeys = extractCmdEnKeys(enText)
  const r = compareSets('cmd  redisCmdSummaryZhCn vs enCommands[].key', zhKeys, enKeys, {
    missingEnIsError: true,
  })
  totalErrors += r.errors
  totalWarns += r.warns
}

if (totalErrors > 0) {
  console.error(
    `\n失败：共 ${totalErrors} 个问题（仅中文有 / 或 strict-full 与 cmd 下「仅英文有」）`,
  )
  process.exit(1)
}
if (totalWarns > 0) {
  console.log(`\n通过（${totalWarns} 条英文独有键已 WARN，加 --strict-full 则失败）`)
} else {
  console.log('\n全部通过')
}
