// Windows 上 file URL 的盘符大小写会被 Node 当成两个模块。
// 小写 c: 启动时，Vitest 自己的 runner 和测试里 import 的 describe 各一份，
// describe 一调用就读不到 config。这里只把 Vitest 相关模块的盘符收成大写。
import { register } from 'node:module'

const marker = 'REDIS_ME_WIN_DRIVE_HOOK'

if (process.platform === 'win32' && !process.env[marker]) {
  process.env[marker] = '1'
  register(import.meta.url, { parentURL: import.meta.url })
}

/** @param {string} url */
function normalizeWinDrive(url) {
  if (!url.startsWith('file:///')) return url
  // 只收 Vitest 自己的模块。其余包保持原样，避免改盘符后 CJS 绝对路径解析失败。
  if (!/\/node_modules\/(?:@vitest\/|vitest\/)/.test(url)) return url
  return url.replace(/^file:\/\/\/([a-z])(?=:|%3A)/, (_, drive) => `file:///${drive.toUpperCase()}`)
}

export async function resolve(specifier, context, nextResolve) {
  const resolved = await nextResolve(specifier, context)
  const url = normalizeWinDrive(resolved.url)
  if (url === resolved.url) return resolved
  return { ...resolved, url }
}
