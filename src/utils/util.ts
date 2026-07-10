import { getAllWebviewWindows, WebviewWindow } from '@tauri-apps/api/webviewWindow'
// 应用级通用工具；以下 `// #region` / `// #endregion` 可在 VS Code / Cursor 中折叠浏览。
import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window'
import { openUrl } from '@tauri-apps/plugin-opener'
import { type } from '@tauri-apps/plugin-os'
import { relaunch } from '@tauri-apps/plugin-process'
import {
  check,
  type CheckOptions,
  type DownloadEvent,
  type Update,
} from '@tauri-apps/plugin-updater'
import { saveWindowState, StateFlags } from '@tauri-apps/plugin-window-state'
import { useClipboard, useDark } from '@vueuse/core'
import { ElMessage, ElMessageBox } from 'element-plus'
import type { ElMessageBoxOptions } from 'element-plus'
import JSON5 from 'json5'
import { applyEdits, format } from 'jsonc-parser'
import { sampleSize, sortBy } from 'lodash'
import mitt from 'mitt'
import { nanoid } from 'nanoid'
import { computed, h } from 'vue'

import i18n from '@/locales'
import type {
  MeAppUpdateState,
  MeCommands,
  EnrichedRedisNode,
  KeyTypeListItem,
} from '@/types/me-interface'
import { commands as spectaCommands } from '@/types/tauri-specta'
import type { RedisKey_Deserialize, RedisNode } from '@/types/tauri-specta'
import { invalidateKeyType } from '@/utils/key-type-cache'

/** 全局 `bus` 事件载荷（与 `bus.emit` / `bus.on` 一致） */
export type MeBusEvents = {
  KEY_DELETE: RedisKey_Deserialize
  /** 载荷未使用；监听器应 `() => refreshKey()` 包装，避免与多参函数签名冲突 */
  KEY_REFRESH: undefined
  INFO_REFRESH: boolean | undefined
  CONN_REFRESH: void
}

export type {
  EnrichedRedisNode,
  KeyTypeListItem,
  MeAppUpdateState,
  MeCommands,
} from '@/types/me-interface'

// #region 本文件内部类型（Specta / 应用错误载荷）
type SpectaResult<T> = { status: 'ok'; data: T } | { status: 'error'; error: unknown }

interface AppErrorPayload {
  code: string
  [key: string]: unknown
}
// #endregion

// #region 全局总线、常量、Redis 键类型与节点列表 enrich
// 全局事件总线：setup 直接导入，app 全局属性也添加
export const bus = mitt<MeBusEvents>()

// 常量
export const KEY_DELETE = 'KEY_DELETE'
export const KEY_REFRESH = 'KEY_REFRESH'
export const INFO_REFRESH = 'INFO_REFRESH'
export const CONN_REFRESH = 'CONN_REFRESH'
export const CONN_LIST_WINDOWS_SYNC = 'CONN_LIST_WINDOWS_SYNC'
export const TREE_KEY_ID_PREFIX = '_TREE_KEY_ID_PREFIX_'

// 预设颜色
export const PREDEFINE_COLORS = [
  '#409eff', // primary
  '#67c23a', // success
  '#e6a23c', // warning
  '#f56c6c', // danger
  '#909399', // info
] as const

// 键类型
export const KEY_TYPE_LIST: KeyTypeListItem[] = [
  { short: 'S', value: 'STRING', type: 'primary' },
  { short: 'H', value: 'HASH', type: 'primary' },
  { short: 'L', value: 'LIST', type: 'danger' },
  { short: 'E', value: 'SET', type: 'danger' },
  { short: 'Z', value: 'ZSET', type: 'danger' },
  { short: 'X', value: 'STREAM', type: 'warning' },
  { short: 'J', value: 'JSON', type: 'warning' },
]

const keyTypeMap = new Map(KEY_TYPE_LIST.map(item => [item.value, item.type]))
const keyShortMap = new Map(KEY_TYPE_LIST.map(item => [item.value, item.short]))

/**
 * 键类型：el-text, el-tag 的 type
 */
export function meType(keyType: string | undefined | null): string {
  if (!keyType) return 'info'
  return keyTypeMap.get(keyType?.toUpperCase() ?? '') || 'info'
}

/**
 * 键类型短：避免 String、Set 的简称都是 S
 */
export function meKeyShort(keyType: string | undefined | null, defaultValue = '?'): string {
  if (!keyType) return defaultValue
  return keyShortMap.get(keyType?.toUpperCase() ?? '') || defaultValue
}

/**
 * 将 node_list 接口数据排序并补充与 UI 一致的字段。
 * 展示顺序：M1/M2/… 在上，同编号的 S1/S2/… 在下。
 */
export function enrichNodeList(rawList: RedisNode[] | null | undefined): EnrichedRedisNode[] {
  if (!rawList?.length) return []
  const sorted = sortBy(rawList, 'node') as EnrichedRedisNode[]

  let masterIndex = 0
  const masterMap = new Map<string, { idx: number; slots: string | null }>()
  sorted.forEach(item => {
    item.isMaster = item.flags?.includes('master') ?? false
    item.isSlave = item.flags?.includes('slave') ?? false
    if (item.isMaster) {
      masterIndex++
      item.shortLabel = 'M' + masterIndex
      masterMap.set(item.node, { idx: masterIndex, slots: item.slots })
    }
  })
  sorted.forEach(item => {
    if (item.isSlave && item.slaveOfNode) {
      const master = masterMap.get(item.slaveOfNode)
      if (master) {
        item.shortLabel = 'S' + master.idx
        item.masterSlots = master.slots
      }
    }

    if (item.isMaster && item.slots) {
      item.slotsTooltip = t('nodeList.slotsTooltip', { slots: item.slots })
    } else if (item.isSlave && item.masterSlots) {
      item.slotsTooltip = t('nodeList.slotsReplicaTooltip', { slots: item.masterSlots })
    } else {
      item.slotsTooltip = ''
    }

    if (!item.shortLabel) {
      item.shortLabel = item.flags?.slice(0, 1).toUpperCase() || 'F'
    }
  })

  return sorted.sort((a, b) => {
    const rank = (item: EnrichedRedisNode) => (item.isMaster ? 0 : item.isSlave ? 1 : 2)
    const num = (label: string) => {
      const m = /^[MS](\d+)$/.exec(label)
      return m ? Number(m[1]) : 999
    }
    return (
      rank(a) - rank(b) || num(a.shortLabel) - num(b.shortLabel) || a.node.localeCompare(b.node)
    )
  })
}
// #endregion

// #region 开发日志、界面语言、暗色主题
const isDev = import.meta.env.DEV
const t = i18n.global.t

// 打印日志（仅开发环境）
export function meLog(...args: unknown[]): void {
  if (isDev) {
    console.log(...args)
  }
}

// 是否是中文模式
export const isZh = computed(() => {
  const language =
    meTauri.settings.language === 'system' ? meTauri.systemLanguage : meTauri.settings.language
  return language?.startsWith('zh') ?? false
})

// 是否黑色主题
export const isDark = useDark()
// #endregion

// #region Specta 命令包装（meCommands）
// 流程：Specta Result → 解包 → 成功则记日志并重置 EOF 计数；失败则 EOF 有限重试，否则弹窗（可静默）并抛出字符串化错误。
const SPECTA_EOF_MESSAGE = 'unexpected end of file'
/** 与原先 `spectaEofRetries <= 3` 一致：最多额外重试 4 次 */
const SPECTA_EOF_MAX_RETRY = 3

let spectaEofRetryCount = 0

function errString(e: unknown): string {
  if (e instanceof Error) return e.message
  if (typeof e === 'string') return e
  try {
    return JSON.stringify(e)
  } catch {
    return Object.prototype.toString.call(e)
  }
}

/** 若 `errorStr` 为带 `code` 的应用错误 JSON 则走 i18n，否则原样返回（供弹窗） */
function formatSpectaErrorForUser(errorStr: string): string {
  try {
    const parsed = JSON.parse(errorStr) as unknown
    if (
      !parsed ||
      typeof parsed !== 'object' ||
      !('code' in parsed) ||
      typeof (parsed as AppErrorPayload).code !== 'string'
    ) {
      return errorStr
    }
    const { code, ...params } = parsed as AppErrorPayload
    const key = `errors.${code}`
    const message = t(key, params as Record<string, unknown>)
    return message === key ? `${code}: ${JSON.stringify(params)}` : message
  } catch {
    return errorStr
  }
}

function unwrapSpecta<T>(raw: SpectaResult<T>): T {
  if (raw.status === 'ok') return raw.data
  throw raw.error
}

async function invokeSpectaCommand<T>(
  name: string,
  args: readonly unknown[],
  run: () => Promise<SpectaResult<T>>,
  alert: boolean,
): Promise<T> {
  const t0 = Date.now()
  try {
    const data = unwrapSpecta(await run())
    meLog(`命令：${name}, 耗时：${Date.now() - t0}ms, 参数：`, args, '结果：', data)
    spectaEofRetryCount = 0
    return data
  } catch (e) {
    const msg = errString(e)
    if (msg === SPECTA_EOF_MESSAGE && spectaEofRetryCount <= SPECTA_EOF_MAX_RETRY) {
      spectaEofRetryCount++
      meLog(`第${spectaEofRetryCount}次重试：${name}`)
      return invokeSpectaCommand(name, args, run, alert)
    }
    if (alert) {
      const title = t('error') + (isDev ? ': ' + name : '')
      meErr(formatSpectaErrorForUser(msg), title)
    }
    meLog(`命令：${name}, 耗时：${Date.now() - t0}ms, 参数:`, args, `, 错误：${msg}`)
    throw msg
  }
}

type SpectaCommandFn = (...a: unknown[]) => Promise<SpectaResult<unknown>>

/** 与 Specta `commands` 同键；末尾多传 `false` 时失败不弹窗 */
function bindMeCommand(name: string, fn: unknown): unknown {
  if (typeof fn !== 'function') return fn
  const spectaFn = fn as SpectaCommandFn
  return (...args: unknown[]) => {
    const silent = args.length > 0 && args[args.length - 1] === false
    const pass = silent ? args.slice(0, -1) : args
    return invokeSpectaCommand(String(name), pass, () => spectaFn(...pass), !silent)
  }
}

export const meCommands = Object.fromEntries(
  Object.entries(spectaCommands).map(([name, fn]) => [name, bindMeCommand(name, fn)]),
) as MeCommands
// #endregion

// #region Element Plus 提示、确认框、剪贴板
export const DoNothing = (): void => {}

export function meOk(
  message: string,
  isAlert = false,
  title = '',
  options: Record<string, unknown> = {},
): void {
  if (isAlert) {
    const finalOptions = { type: 'success' as const, draggable: true, ...options }
    void ElMessageBox.alert(message, title || t('info'), finalOptions).then(DoNothing)
  } else {
    ElMessage.success(message)
  }
}

export function meWarn(message: string): void {
  ElMessage.warning(message)
}

export function meErr(message: unknown, title: string = t('error')): void {
  const raw =
    message instanceof Error
      ? message.message
      : typeof message === 'string'
        ? message
        : errString(message)
  const text = formatSpectaErrorForUser(raw)
  void ElMessageBox.alert(text, title, { type: 'error', draggable: true }).then(DoNothing)
}

/** 错误弹窗（HTML 换行，用于自定义编解码测试等） */
export function meErrHtml(message: string, title: string = t('error')): void {
  void ElMessageBox.alert(message, title, {
    type: 'error',
    draggable: true,
    dangerouslyUseHTMLString: true,
  }).then(DoNothing)
}

export function meConfirm(
  message: string,
  thenFun: () => void | Promise<void>,
  boxOptions: ElMessageBoxOptions = {},
): void {
  ElMessageBox.confirm(message, boxOptions?.type === 'info' ? t('info') : t('warn'), {
    type: 'warning',
    ...boxOptions,
  })
    .then(thenFun)
    .catch(DoNothing)
}

export function mePrompt(
  message: string,
  options: ElMessageBoxOptions,
  thenFun: (result: { value: string }) => void | Promise<void>,
): void {
  ElMessageBox.prompt(message, options).then(thenFun).catch(DoNothing)
}

// 复制文本
export function meCopy(text: string, hintContent?: string, hint = true): void {
  void useClipboard({ legacy: true }).copy(text)
  if (hint) {
    meOk(hintContent || t('copyOk'))
  }
}
// #endregion

// #region 随机串、可读数量/时间、表格列过滤
const CHAR_ARRAY = Array.from('abcdefghigklmnopqrstuvwxyz0123456789')
export function meRandomString(n: number): string {
  return sampleSize(CHAR_ARRAY, n).join('')
}

const humanUnits = [
  { threshold: 1, symbol: 'B' },
  { threshold: 1024, symbol: 'K' },
  { threshold: 1024 ** 2, symbol: 'M' },
  { threshold: 1024 ** 3, symbol: 'G' },
  { threshold: 1024 ** 4, symbol: 'T' },
] as const

/** MEMORY USAGE 不可用时的 String 键内存粗估：键名 + 值字节 + Redis 对象/SDS 固定开销 */
export function estimateStringMemory(key: string, valueByteLen: number): number {
  const keyBytes = new TextEncoder().encode(key).length
  const OVERHEAD = 56
  return keyBytes + valueByteLen + OVERHEAD
}

export function meHumanSize(size: number, zeroShow = '0B', fractionDigits = 2): string {
  if (!size) return zeroShow || ''

  for (let i = humanUnits.length - 1; i >= 0; i--) {
    const u = humanUnits[i]!
    if (size >= u.threshold) {
      const value = size / u.threshold
      return value.toFixed(fractionDigits) + u.symbol
    }
  }

  return size + 'B'
}

const humanNums = [
  { threshold: 1, symbol: '' },
  { threshold: 1000, symbol: 'K' },
  { threshold: 1000 ** 2, symbol: 'M' },
  { threshold: 1000 ** 3, symbol: 'B' },
] as const

export function meHumanNums(size: number, zeroShow = '0', fractionDigits = 2): string {
  if (!size) return zeroShow || ''

  for (let i = humanNums.length - 1; i >= 0; i--) {
    const u = humanNums[i]!
    if (size >= u.threshold) {
      const value = size / u.threshold
      return value.toFixed(fractionDigits) + u.symbol
    }
  }

  return String(size)
}

export function meHumanSeconds(seconds: number | undefined | null): string | number {
  if (seconds === undefined || seconds === null) return '-'
  if (seconds <= 0) return seconds

  let rest = seconds
  const days = Math.floor(rest / (3600 * 24))
  rest %= 3600 * 24

  const hours = Math.floor(rest / 3600)
  rest %= 3600

  const minutes = Math.floor(rest / 60)
  rest %= 60

  const formattedHours = String(hours).padStart(2, '0')
  const formattedMinutes = String(minutes).padStart(2, '0')
  const formattedSeconds = String(rest).padStart(2, '0')

  let result = `${formattedHours}:${formattedMinutes}:${formattedSeconds}`
  if (days > 0) {
    result = `${days}${t('util.days', days)} ${result}`
  }
  return result
}

export function meTtlSeconds(intValue: number, unit: string): number {
  if (intValue === -1) return -1
  if (unit === 'second') return intValue
  if (unit === 'minute') return intValue * 60
  if (unit === 'hour') return intValue * 60 * 60
  if (unit === 'day') return intValue * 60 * 60 * 24
  return intValue
}

export function meFilterHandler<T extends Record<string, unknown>>(
  value: unknown,
  row: T,
  column: { property?: string },
): boolean {
  const property = column.property
  if (!property) return false
  return row[property] === value
}
// #endregion

// #region Redis 键：删除 / 重命名（组合确认框与 meCommands）

export function meDeleteKey(id: string, redisKey: RedisKey_Deserialize, thenFn?: () => void): void {
  meConfirm(t('util.deleteKey', { key: redisKey.key }), async () => {
    await meCommands.del(id, redisKey)
    invalidateKeyType(id, redisKey)
    bus.emit(KEY_DELETE, redisKey)
    meOk(t('deleteOk'))
    thenFn?.()
  })
}

// #endregion

// #region 应用内自动更新（Tauri updater）
export async function meCheckUpdate(
  quiet = true,
  checkOptions: CheckOptions = {},
  app: MeAppUpdateState,
): Promise<void> {
  if (window?.meTauri?.isAppStore) {
    meLog('应用商店内部的应用更新，忽略检查接口')
    return
  }

  if (!quiet) {
    ElMessage.primary(t('util.checking'))
  }

  const update = await check(checkOptions).catch(DoNothing)
  if (update) {
    await meDownloadUpdate(quiet, update, app)
  } else if (update === null) {
    if (!quiet) {
      ElMessage.success(t('util.latestVersion'))
    }
  } else {
    if (!quiet) {
      ElMessage.error(t('util.checkUpdateErr'))
    }
  }
}

const manualCloseOptions: ElMessageBoxOptions = {
  closeOnClickModal: false,
  closeOnPressEscape: false,
  type: 'info',
}

export async function meDownloadUpdate(
  quiet = true,
  update: Update,
  app: MeAppUpdateState,
): Promise<void> {
  meLog('检查结果:', update)
  const hint = t('util.updateHint', { version: update.version })
  const changelog = t('util.changelog')
  const changelogUrl = t('util.changelogUrl')
  const message = () =>
    h('p', null, [
      h('span', hint),
      h(
        'a',
        {
          style:
            'color: var(--el-color-primary); text-decoration: none; margin-left: 5px; cursor: pointer; ',
          onClick: () => {
            void openUrl(changelogUrl)
          },
        },
        changelog,
      ),
    ])

  meConfirm(
    'MessageInvalid',
    async () => {
      try {
        app.downloading = true
        app.downloadPercentage = 0

        let downloaded = 0
        let contentLength = 0
        const downloadingHandle = (event: DownloadEvent) => {
          switch (event.event) {
            case 'Started':
              contentLength = event.data.contentLength ?? 0
              break
            case 'Progress':
              downloaded += event.data.chunkLength
              app.downloadPercentage = contentLength
                ? Math.round((downloaded / contentLength) * 100)
                : 0
              break
            case 'Finished':
              app.downloadPercentage = 100
              break
          }
        }

        const isWindows = type() === 'windows'
        const isMacOS = type() === 'macos'
        if (isWindows) {
          await update.download(downloadingHandle)
          meConfirm(t('util.downloadDown'), async () => await update.install(), manualCloseOptions)
        } else {
          await update.downloadAndInstall(downloadingHandle)
          // macOS：relaunch 会与 single-instance 竞态，改走 Rust 延迟 open 重启
          meConfirm(
            t('util.updateDone'),
            async () => {
              if (isMacOS) {
                await meCommands.restartAfterUpdate()
              } else {
                await relaunch()
              }
            },
            manualCloseOptions,
          )
        }
      } catch (e) {
        meErr(t('util.updateErr', { message: errString(e) }))
      } finally {
        app.downloading = false
      }
    },
    { ...manualCloseOptions, message },
  )
}
// #endregion

// #region 新窗口
/** 与 tauri.conf.json 默认窗口尺寸一致 */
export const DEFAULT_WINDOW_SIZE = { width: 1200, height: 800 } as const

/** 当前窗口恢复默认大小并居中，同时写入 window-state 持久化 */
export async function resetWindowToDefault(): Promise<void> {
  const win = getCurrentWindow()
  if (await win.isFullscreen()) {
    await win.setFullscreen(false)
    await sleep(100)
  }
  if (await win.isMaximized()) {
    await win.unmaximize()
    // Windows 取消最大化后需等布局稳定，否则 setSize 可能被忽略
    await sleep(100)
  }
  await win.setSize(new LogicalSize(DEFAULT_WINDOW_SIZE.width, DEFAULT_WINDOW_SIZE.height))
  await win.center()
  await sleep(50)
  await saveWindowState(StateFlags.ALL)
}

/** F11 切换当前 Tauri 窗口全屏；与全局快捷键「全屏应用」一致 */
export async function toggleAppFullscreen(): Promise<void> {
  const win = getCurrentWindow()
  await win.setFullscreen(!(await win.isFullscreen()))
}

/** 新建 Tauri 窗口（与 KeyHeader 菜单「新窗口」一致） */
export async function openNewWindow(): Promise<void> {
  const isMacOS = type() === 'macos'
  const windows = await getAllWebviewWindows()
  const hasMainWindow = !!windows.find(item => item.label === 'main')
  const label = hasMainWindow ? 'Window' + nanoid() : 'main'

  const appWindow = new WebviewWindow(label, {
    url: 'index.html',
    title: 'RedisME',
    hiddenTitle: true,
    width: 1200,
    height: 800 + 25,
    dragDropEnabled: false,
    titleBarStyle: 'overlay',
    decorations: isMacOS,
  })

  appWindow.once('tauri://created', () => {})
  appWindow.once('tauri://error', () => {
    meErr(i18n.global.t('keyHeader.newWindowError'))
  })
}
// #endregion

// #region sleep、JSON 格式化与解析
export function sleep(ms: number): Promise<void> {
  return new Promise(resolve => setTimeout(resolve, ms))
}

export function meJsonFormat(jsonString: string): string {
  return applyEdits(jsonString, format(jsonString, undefined, { insertSpaces: true, tabSize: 2 }))
}

/** 单字段/字符串值的展示格式化（与 RedisValue isPretty 规则对齐） */
export function meFormatDisplayValue(raw: string, pretty: boolean): string {
  if (!pretty || !raw) return raw
  const trimmed = raw.trim()
  if (trimmed.startsWith('{') || trimmed.startsWith('[')) {
    try {
      return meJsonFormat(raw)
    } catch {
      return raw
    }
  }
  return raw
}

export function meJsonParse(jsonString: string | null | undefined): unknown {
  if (!jsonString) return null
  if (jsonString === 'undefined') return null
  if (jsonString === 'null') return null
  return JSON5.parse(jsonString)
}

export function meJsonNormal(jsonString: string): string {
  return JSON.stringify(JSON5.parse(jsonString), null, 2)
}
// #endregion
