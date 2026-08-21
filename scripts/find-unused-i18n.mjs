import fs from 'fs'
import path from 'path'
import { fileURLToPath } from 'url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const root = path.resolve(__dirname, '..')

function parseLangFile(filePath) {
  let content = fs.readFileSync(filePath, 'utf8')
  content = content.replace(/^export default\s+/, '').replace(/;\s*$/, '')
  return Function(`"use strict"; return (${content})`)()
}

function flattenKeys(o, prefix = '') {
  const keys = []
  for (const [k, v] of Object.entries(o)) {
    const p = prefix ? `${prefix}.${k}` : k
    if (v !== null && typeof v === 'object' && !Array.isArray(v)) {
      keys.push(...flattenKeys(v, p))
    } else {
      keys.push(p)
    }
  }
  return keys
}

function collectFiles(dir, out = []) {
  if (!fs.existsSync(dir)) return out
  for (const ent of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, ent.name)
    if (ent.isDirectory()) {
      if (['node_modules', 'locales'].includes(ent.name)) continue
      collectFiles(p, out)
    } else if (/\.(vue|ts|tsx|js|md|rs)$/.test(ent.name)) {
      out.push(p)
    }
  }
  return out
}

function extractUsedKeys(corpus) {
  const used = new Set()

  // t('key') / t("key") / t(`key`)
  for (const m of corpus.matchAll(/\bt\(\s*['"`]([^'"`$]+)['"`]/g)) {
    used.add(m[1])
  }

  // ConnImportParseError('conn.xxx')
  for (const m of corpus.matchAll(/ConnImportParseError\(\s*['"]([^'"]+)['"]/g)) {
    used.add(m[1])
  }

  // throw new ConnImportParseError(skipped ? 'conn.xxx' : 'conn.yyy') — ternary in same line
  for (const m of corpus.matchAll(/ConnImportParseError\([^)]*['"](conn\.[^'"]+)['"]/g)) {
    used.add(m[1])
  }

  // me-tab-pane name="info" → tabMain.info
  for (const m of corpus.matchAll(/<me-tab-pane\s+name="([^"]+)"/g)) {
    used.add(`tabMain.${m[1]}`)
  }

  return used
}

function isUsed(key, used) {
  if (used.has(key)) return true
  // parent prefix for dynamic: t(`tabMain.${name}`)
  const parts = key.split('.')
  if (parts.length >= 2) {
    const parent = parts.slice(0, -1).join('.')
    const last = parts[parts.length - 1]
    // tabMain.* built dynamically
    if (parent === 'tabMain' && [...used].some(u => u.startsWith('tabMain.'))) {
      // only mark used if this specific tab exists
      return used.has(key)
    }
  }
  return false
}

const langZh = path.join(root, 'src/locales/lang/zh-cn.ts')
const langEn = path.join(root, 'src/locales/lang/en.ts')
const allKeys = flattenKeys(parseLangFile(langZh))

const searchFiles = collectFiles(path.join(root, 'src')).concat(
  collectFiles(path.join(root, 'src-tauri')),
)
const corpus = searchFiles.map(f => fs.readFileSync(f, 'utf8')).join('\n')
const used = extractUsedKeys(corpus)

const unused = allKeys.filter(k => !isUsed(k, used))

// zh vs en parity
const enKeys = new Set(flattenKeys(parseLangFile(langEn)))
const zhOnly = allKeys.filter(k => !enKeys.has(k))
const enOnly = [...enKeys].filter(k => !new Set(allKeys).has(k))

console.log(`vue-i18n lang/zh-cn.ts 共 ${allKeys.length} 个键`)
console.log(`代码中静态引用 ${used.size} 个不同键`)
console.log(`\n未引用键 ${unused.length} 个：\n`)
unused.sort().forEach(k => console.log(`  ${k}`))

if (zhOnly.length || enOnly.length) {
  console.log(`\nzh/en 不对称: zh独有 ${zhOnly.length}, en独有 ${enOnly.length}`)
}
