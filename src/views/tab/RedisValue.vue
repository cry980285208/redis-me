<script setup lang="ts">
/**
 * 键值详情页：fieldScan 拉取 → JSON 编辑器 / 表格展示 → set / field* 写回。
 * 数据流：bytesFormat 触发 refreshKey → syncDisplaySnapshot（Auto 时识别）→ showValue / dataList 渲染。
 */
// #region 导入
import dayjs from 'dayjs'
import { minimatch } from 'minimatch'
import {
  computed,
  inject,
  nextTick,
  onMounted,
  onUnmounted,
  ref,
  useTemplateRef,
  watch,
  watchEffect,
} from 'vue'
import { useI18n } from 'vue-i18n'

import { shareProvideKey, connUiProvideKey } from '@/types/me-interface'
import type {
  FieldScanResult,
  RedisFieldAsCommand_Deserialize,
  RedisFieldDel_Deserialize,
  RedisFieldGet_Deserialize,
  RedisFieldValue,
  RedisKey_Deserialize,
  RedisZsetRankResult,
  ScanCursor,
} from '@/types/tauri-specta'
import {
  base64ToUtf8Text,
  detectViewFormat,
  detectedViewLabel,
  type DetectedViewFormat,
} from '@/utils/detect-view-format'
import { useFavorites, addFavorite, removeFavorite, isFavorited } from '@/utils/favorite'
import {
  BYTES_FORMAT,
  EXT_FORMAT,
  customFormatName,
  customFormatValue,
  isCustomView,
  isReadonlyView,
  isStringOnlyView,
  isViewDecodeError,
  meFormatViewValue,
  meFormatViewValueAsync,
  meViewToWire,
  meViewToWireAsync,
  needsJsonNormalize,
  toWireFormat,
  viewFmtForField,
  type ViewBytesFormat,
} from '@/utils/format'
import { resolveKeyType } from '@/utils/key-type-cache'
import {
  buildScanPattern,
  buildLocalFilterPattern,
  computeScanProgress,
  MINIMATCH_SCAN_OPTS,
} from '@/utils/redis-glob'
import {
  bus,
  KEY_DELETE,
  KEY_REFRESH,
  meCommands,
  meConfirm,
  meCopy,
  meDeleteKey,
  meErr,
  meHumanSeconds,
  estimateStringMemory,
  meHumanSize,
  meFormatDisplayValue,
  meJsonNormal,
  meOk,
  meWarn,
  sleep,
} from '@/utils/util'
import ObjectInfo from '@/views/ext/ObjectInfo.vue'
import TableGroup from '@/views/ext/TableGroup.vue'
import TableHashKeys from '@/views/ext/TableHashKeys.vue'
import TableZsetRange from '@/views/ext/TableZsetRange.vue'
import TTLSet from '@/views/ext/TTLSet.vue'
import ValueShortcut from '@/views/ext/ValueShortcut.vue'
import KeyRename from '@/views/key/KeyRename.vue'

import CommandHelp from '../ext/CommandHelp.vue'
import CustomCodec from '../ext/CustomCodec.vue'
import FieldAdd from '../ext/FieldAdd.vue'
import FieldSet from '../ext/FieldSet.vue'
// #endregion

// #region 类型与本地工具
/** newValue：null 未编辑，'' 表示用户主动保存空串 */
type FieldScanViewState = FieldScanResult & { newValue: string | null }

/** fieldScan 的 `value` 在 Specta 中为 serde 联合类型，表格/拼接按行数组处理 */
function fieldValueRows(v: unknown): unknown[] {
  return v as unknown[]
}

function toViewState(data: FieldScanResult): FieldScanViewState {
  return { ...data, newValue: null }
}

/** 值表格行（fieldScan 各类型字段混合） */
type ValueTableRow = Record<string, unknown> & {
  key?: string
  value?: unknown
  id?: string
  score?: number
  ttl?: number
  /** List 行的真实 Redis 索引（后端 fieldScan 返回） */
  index?: number
}
// #endregion

// #region 共享上下文与权限
const { t } = useI18n()
const share = inject(shareProvideKey)!
const connUi = inject(connUiProvideKey)!
const canEdit = computed(() => !share.readonly)
// #endregion

// #region 核心状态（fieldScan 结果 / 游标 / 编辑）
const redisValue = ref<FieldScanViewState | null>(null)
const cursor = ref<ScanCursor | null>(null) // list/hash/set/zset/stream 分页游标
const loading = ref(false)
const isPretty = ref(true)
/** 表格工具栏关键词：Hash/Set/ZSet 兼扫描参数与本地过滤，List/Stream 仅本地过滤 */
const fieldKeyword = ref('')
const fieldExact = ref(false)
const fieldMatch = computed(() => buildScanPattern(fieldKeyword.value, fieldExact.value))
const scanCancelled = ref(false)
const scanPaused = ref(false)
const scanLoadAll = ref(false)
const scanBatchCount = ref(0)
const SCAN_CONTROL_MIN_BATCHES = 10
const showScanControl = computed(() => {
  const type = redisValue.value?.type
  if (!supportsTableView(type)) return false
  return scanPaused.value || (loading.value && scanBatchCount.value >= SCAN_CONTROL_MIN_BATCHES)
})
const showFieldExactCheckbox = computed(() => supportsFieldServerScan(redisValue.value?.type))
const fieldScanInputPlaceholder = computed(() => {
  const type = redisValue.value?.type
  if (type === 'list' || type === 'stream') {
    return t('redisValue.listStreamFilterPlaceholder')
  }
  return t('redisValue.fieldScanPlaceholder')
})
const scanToggleTip = computed(() =>
  loading.value ? t('keyMain.pauseScan') : t('keyMain.resumeScan'),
)
const FIELD_SCAN_FETCH_COUNT = computed(() => meTauri.settings.fieldScanCount as number)
/** 进度环估算：与 settings.fieldScanCount 一致 */
const scanBatchSize = computed(() => FIELD_SCAN_FETCH_COUNT.value)
const scanProgress = computed(() =>
  computeScanProgress(
    scanBatchCount.value,
    scanBatchSize.value,
    redisValue.value?.length ?? 0,
    Boolean(cursor.value?.finished),
  ),
)
const suppressCodeUpdate = ref(false)
/** fieldScan 成功后递增，强制 me-code 与服务器同步（未保存时 modelValue 字符串可能不变） */
const valueEditorRemountKey = ref(0)
/** 手动控制「加载更多」按钮，避免 cursor 变化导致按钮闪现 */
const showMore = ref(false)

/** STRING 全量加载阈值与预览长度，从 settings 读取 */
const VALUE_BYTE_LIMIT = computed(
  () => ((window.meTauri.settings.valueByteLimitMB as number) ?? 1) * 1024 * 1024,
)
const VALUE_PREVIEW_BYTES = computed(
  () => (window.meTauri.settings.valuePreviewBytes as number) ?? 2000,
)
/** 用户确认「仍要加载全部」后为 true，fieldScan 走 GET 全量 */
const forceFullValue = ref(false)
const valueTruncatedDismissed = ref(false)
const valueTruncated = computed(() => redisValue.value?.valueTruncated ?? false)
const showValueTruncatedAlert = computed(
  () => stringType.value && valueTruncated.value && !valueTruncatedDismissed.value,
)

/** Stream 扫描范围（meta 传给 fieldScan） */
const meta = ref({ maxId: '', minId: '' })
/** List 扫描范围与方向（经 meta 传给 fieldScan） */
const listIndexMin = ref('')
const listIndexMax = ref('')
/** true=升序扫描；false=降序 */
const listDescAsc = ref(true)
/** Stream 扫描方向：true=升序（XRANGE），false=降序（XREVRANGE） */
const streamDescAsc = ref(true)

function parseListIndexInput(raw: string): number | null {
  const s = raw.trim()
  if (!s) return null
  const n = Number.parseInt(s, 10)
  return Number.isFinite(n) ? n : null
}

function listRowRedisIndex(row: ValueTableRow): number {
  return typeof row.index === 'number' ? row.index : -1
}

function toggleListSortOrder() {
  listDescAsc.value = !listDescAsc.value
  void restartFieldScan()
}

function toggleStreamSortOrder() {
  streamDescAsc.value = !streamDescAsc.value
  void restartFieldScan()
}

/** List LPOP/RPOP / Set SPOP / ZSet ZPOPMIN/ZPOPMAX：统一走 field_pop API */
async function runFieldPop(mode: string) {
  const conn = share.conn
  const key = share.redisKey
  if (!conn || !key || !canEdit.value) return
  const data = await meCommands.fieldPop(conn.id, {
    key,
    mode,
    valFmt: toWireFormat(viewFmtForField(bytesFormat.value)),
  })
  meOk(data)
  await restartFieldScan()
}

function onPopCommand(command: string) {
  const confirmMap: Record<string, string> = {
    LPOP: 'redisValue.listLpopConfirm',
    RPOP: 'redisValue.listRpopConfirm',
    SPOP: 'redisValue.setPopConfirm',
    ZPOPMIN: 'redisValue.zpopMinConfirm',
    ZPOPMAX: 'redisValue.zpopMaxConfirm',
  }
  meConfirm(t(confirmMap[command]), () => runFieldPop(command))
}
// #endregion

// #region 键类型（派生）
const stringType = computed(() => 'string' === redisValue.value?.type)
const jsonType = computed(() => 'json' === redisValue.value?.type)
const streamType = computed(() => 'stream' === redisValue.value?.type)
const hashType = computed(() => 'hash' === redisValue.value?.type)
const listType = computed(() => 'list' === redisValue.value?.type)
const setType = computed(() => 'set' === redisValue.value?.type)
const zsetType = computed(() => 'zset' === redisValue.value?.type)
/** 服务端支持 HTTL 时，可选是否在 fieldScan 中拉取 Hash 字段 TTL */
const scanHashFieldTtl = ref(false)
const showHashFieldTtlOption = computed(() => hashType.value && share.capabilities.httlSupported)
// #endregion

// #region 视图模式：JSON 编辑器 / 表格
type FieldViewType = 'json' | 'table'
const viewTypeList: FieldViewType[] = ['json', 'table']
const viewType = ref<FieldViewType>('json')

function supportsFieldServerScan(type: string | undefined) {
  return type === 'hash' || type === 'set' || type === 'zset'
}

function pauseFieldScan() {
  scanCancelled.value = true
  scanPaused.value = true
}

function onFieldScanAction() {
  if (loading.value) pauseFieldScan()
  else if (scanPaused.value) {
    scanPaused.value = false
    void refreshKey(false, true, scanLoadAll.value, false)
  }
}

/** Enter / 搜索图标：保留 keyword，中断进行中的扫描后重扫（无 F5 快捷键） */
function restartFieldScan() {
  return refreshKey(false, false, false, true)
}

async function onFieldSearch() {
  await restartFieldScan()
}

/** 菜单 / 底部按钮手动刷新：保留 fieldKeyword，可 restart 中断扫描 */
function manualRefreshKey() {
  prepareManualKeyRefresh()
  return restartFieldScan()
}

/** 支持表格视图的类型（与底部 segmented 可见条件一致） */
function supportsTableView(type: string | undefined) {
  return (
    type === 'hash' || type === 'list' || type === 'set' || type === 'zset' || type === 'stream'
  )
}

/** field_get 可单行刷新的表格类型 */
function supportsFieldRowRefresh(type: string | undefined) {
  return type === 'hash' || type === 'list' || type === 'zset'
}

/** 切换键或 reset 时，按 settings.fieldShow 决定默认视图（可传入刚拿到的 type，避免等 finally） */
function applyDefaultViewType(type?: string) {
  const keyType = type ?? redisValue.value?.type
  if (!keyType || keyType === 'string' || keyType === 'json') {
    viewType.value = 'json'
    return
  }
  if (!supportsTableView(keyType)) {
    viewType.value = 'json'
    return
  }
  if (meTauri.settings.fieldShow === 'table') {
    viewType.value = 'table'
    return
  }
  // auto：默认表格，手动切换后沿用 fieldShowView（跨连接/键）
  viewType.value = meTauri.settings.fieldShowView === 'json' ? 'json' : 'table'
}

/** 写入 fieldScan 结果；换键时同步校正 viewType，避免先闪 JSON 再切表格 */
function commitFieldScanReplace(data: FieldScanResult, resetView: boolean) {
  redisValue.value = toViewState(data)
  if (resetView) applyDefaultViewType(data.type)
}

/** 自动模式下记录 segmented 手动切换，写入 settings 持久化 */
function onViewTypeChange(val: string | number | boolean) {
  if (meTauri.settings.fieldShow !== 'auto') return
  if (val === 'json' || val === 'table') {
    meTauri.settings.fieldShowView = val
  }
}

// string / json 仅支持 JSON 视图，强制切回
watchEffect(() => {
  if (stringType.value || jsonType.value) {
    viewType.value = 'json'
  }
})
// #endregion

// #region 字节格式与展示快照（wire ↔ 视图文本）
/** 下拉选中项，变更时触发 fieldScan；STRING 默认 Auto */
const bytesFormat = ref<ViewBytesFormat>('auto')
/**
 * 换键待探测标记（KEY_REFRESH 置位）。
 * 真正开跑时领取到本次局部变量，避免被上一次 finally 清掉。
 */
const pendingAutoDetect = ref(false)
/** 本次 refresh 是否按 base64 探测包拉取（领取后置位，首包结束后关闭） */
const probingAutoDetect = ref(false)
/** Auto 识别结果（左侧标签）；非 Auto 时不展示 */
const detectedView = ref<DetectedViewFormat>('utf8')
/** Auto 时为识别结果，否则等于 bytesFormat；驱动展示 / 保存 / 只读 */
const effectiveViewFormat = computed<ViewBytesFormat>(() =>
  bytesFormat.value === 'auto' ? detectedView.value : bytesFormat.value,
)
const detectedViewText = computed(() =>
  bytesFormat.value === 'auto' && stringType.value ? detectedViewLabel(detectedView.value) : '',
)

/** 仅编码变化时写入下拉，相同则保持不动 */
function commitBytesFormat(next: ViewBytesFormat) {
  if (bytesFormat.value !== next) bytesFormat.value = next
}

/** 仅探测结果变化时写入左侧标签，相同则保持不动 */
function commitDetectedView(next: DetectedViewFormat) {
  if (detectedView.value !== next) detectedView.value = next
}
const canSave = computed(
  () =>
    canEdit.value &&
    (stringType.value || jsonType.value) &&
    !isReadonlyView(effectiveViewFormat.value) &&
    !(valueTruncated.value && !forceFullValue.value),
)
/**
 * 展示层快照（防切换编码闪烁）：
 * - displayBytesFormat + displayWire：fieldScan 完成后才更新，供编辑器渲染
 * - resolvedWireView：custom 异步 decode 结果（仅 custom 时使用）
 * - Auto 时 displayBytesFormat 为识别结果，displayWire 对 utf8/strjson 已转为文本
 */
const displayBytesFormat = ref<ViewBytesFormat>('utf8')
const displayWire = ref('')
const customCodecVisible = ref(false)
/** STRING 单键：wire → 当前视图文本（custom 异步解码） */
const resolvedWireView = ref('')
/** custom 编解码失败时为 true，编辑器展示 resolvedWireView 中的错误信息 */
const customCodecFailed = ref(false)

const formatOptions = computed(() => {
  // Auto 置顶，仅 STRING 可用
  const builtin = [
    { label: 'Auto', value: 'auto' as ViewBytesFormat, disabled: !stringType.value },
    ...BYTES_FORMAT.map(item => ({
      label: item,
      value: item.toLowerCase() as ViewBytesFormat,
      disabled: false,
    })),
    ...EXT_FORMAT.map(label => ({
      label,
      value: label.toLowerCase() as ViewBytesFormat,
      disabled: !stringType.value,
    })),
  ]
  const custom = (window.meTauri.settings.customCodecs ?? []).map(f => ({
    label: f.name,
    value: customFormatValue(f.name),
    disabled: !stringType.value,
  }))
  return { builtin, custom }
})

const viewDecodeFailed = computed(() => {
  if (!stringType.value) return false
  const fmt = displayBytesFormat.value
  if (fmt === 'utf8' || fmt === 'hex' || fmt === 'binary' || fmt === 'base64') return false
  const wire = displayWire.value
  if (!wire) return false
  if (isCustomView(fmt)) return customCodecFailed.value
  return isViewDecodeError(meFormatViewValue(wire, fmt))
})

/** 自定义编解码被删或改名后，当前选中项失效则回退 utf8 */
watch(
  () => window.meTauri.settings.customCodecs,
  list => {
    if (!isCustomView(bytesFormat.value)) return
    const name = customFormatName(bytesFormat.value)
    if (!name || !list?.some(f => f.name === name)) {
      bytesFormat.value = stringType.value ? 'auto' : 'utf8'
      void refreshKey(false)
    }
  },
  { deep: true },
)

watch(stringType, isString => {
  if (!isString && isStringOnlyView(bytesFormat.value)) {
    commitBytesFormat('utf8')
  }
})

function setCustomCodecError(message: string) {
  resolvedWireView.value = message
  customCodecFailed.value = true
}

function syncDisplaySnapshot() {
  const rv = redisValue.value
  if (!rv || rv.value === null || rv.value === undefined) {
    displayWire.value = ''
    if (bytesFormat.value === 'auto' && stringType.value) {
      commitDetectedView('utf8')
      displayBytesFormat.value = 'utf8'
    } else {
      displayBytesFormat.value = bytesFormat.value
    }
    return
  }
  if (streamType.value) {
    displayWire.value = JSON.stringify(rv.value)
    displayBytesFormat.value = bytesFormat.value
    return
  }

  const wire = String(rv.value)
  // Auto：base64 wire 上识别；utf8/strjson 展示层改为文本，避免二次请求
  if (bytesFormat.value === 'auto' && stringType.value) {
    const nextDetected = detectViewFormat(wire)
    commitDetectedView(nextDetected)
    if (nextDetected === 'utf8' || nextDetected === 'strjson') {
      displayWire.value = base64ToUtf8Text(wire) ?? ''
    } else {
      displayWire.value = wire
    }
    displayBytesFormat.value = nextDetected
    return
  }

  displayWire.value = wire
  displayBytesFormat.value = bytesFormat.value
}

async function refreshResolvedWireView() {
  if (!stringType.value || !isCustomView(displayBytesFormat.value)) {
    resolvedWireView.value = ''
    customCodecFailed.value = false
    return
  }
  const wire = displayWire.value
  if (!wire) {
    resolvedWireView.value = ''
    customCodecFailed.value = false
    return
  }
  try {
    resolvedWireView.value = await meFormatViewValueAsync(wire, displayBytesFormat.value)
    customCodecFailed.value = false
  } catch (e) {
    setCustomCodecError(e instanceof Error ? e.message : String(e))
  }
}

function stringWireDisplayText(wire: string): string {
  if (stringType.value && isCustomView(displayBytesFormat.value)) {
    return resolvedWireView.value
  }
  return meFormatViewValue(wire, displayBytesFormat.value)
}

function formatTableCell(raw: unknown): string {
  return stringWireDisplayText(String(raw ?? ''))
}
// #endregion

// #region 编辑器展示内容（showValue）
const showValue = computed(() => {
  const obj = redisValue.value?.value
  if (obj === null || obj === undefined) return ''

  if (isPretty.value) {
    if (stringType.value) {
      const str = stringWireDisplayText(displayWire.value)
      return meFormatDisplayValue(str, isPretty.value)
    }
    return JSON.stringify(obj, null, 2)
  }

  if (
    'hash' === redisValue.value?.type ||
    'zset' === redisValue.value?.type ||
    'json' === redisValue.value?.type ||
    'stream' === redisValue.value?.type
  ) {
    return JSON.stringify(obj)
  }
  if (stringType.value) {
    return stringWireDisplayText(displayWire.value)
  }
  return obj.toString()
})

/** me-code 编辑回调：写入 redisValue.newValue，保存时由 setValue 读回 */
function onCodeUpdate(newValue: string) {
  if (suppressCodeUpdate.value || !redisValue.value) return
  redisValue.value.newValue = newValue
}

/** 值区有未保存修改（含改为空串；null 表示未编辑） */
const valueDirty = computed(() => {
  const rv = redisValue.value
  if (!rv || rv.newValue === null) return false
  return rv.newValue !== showValue.value
})
// #endregion

// #region 表格行数据与筛选
const dataList = computed(() => {
  const rv = redisValue.value
  if (rv === null || rv === undefined || rv.value === null || rv.value === undefined) return []

  const data: ValueTableRow[] = []
  fieldValueRows(rv.value).forEach(value => {
    // set 为裸字符串；list/hash/zset/stream 已是对象（list 含 index）
    if (rv.type === 'set') data.push({ value })
    else data.push(value as ValueTableRow)
  })
  return data
})

const filterDataList = computed(() => {
  const key = fieldKeyword.value.toLowerCase()
  return dataList.value.filter(row => {
    if (!key) return true
    if ((formatTableCell(row.key).toLowerCase() ?? '').indexOf(key) > -1) return true
    if ((row.id?.toLowerCase() ?? '').indexOf(key) > -1) return true
    const cell = streamType.value ? JSON.stringify(row.value) : formatTableCell(row.value)
    if (cell.toLowerCase().indexOf(key) > -1) return true
    if ((row.score?.toString() ?? '').indexOf(key) > -1) return true
    if (String(row.index ?? '').indexOf(key) > -1) return true
    return false
  })
})

/** 切换 exact 未 Enter 时本地 minimatch（与 KeyMain filterKeyList 一致） */
const filterFieldPattern = computed(() =>
  buildLocalFilterPattern(fieldKeyword.value, fieldExact.value, fieldMatch.value),
)

const filterFieldList = computed(() => {
  if (!filterFieldPattern.value) return dataList.value
  return dataList.value.filter(row => {
    const name = row.key ? formatTableCell(row.key) : formatTableCell(row.value)
    return minimatch(name, filterFieldPattern.value, MINIMATCH_SCAN_OPTS)
  })
})

const tableDisplayList = computed(() => {
  const type = redisValue.value?.type
  if (type === 'hash' || type === 'set' || type === 'zset') return filterFieldList.value
  return filterDataList.value
})

/** 值表各类型默认排序列（与可见 sortable 列 prop 一致）；List 不设 default-sort，保持 fieldScan 返回顺序（含升/降序扫描） */
const tableDefaultSort = computed(
  (): { prop: string; order: 'ascending' | 'descending' } | undefined => {
    switch (redisValue.value?.type) {
      case 'hash':
        return { prop: 'key', order: 'ascending' }
      case 'zset':
        return { prop: 'score', order: 'ascending' }
      case 'set':
        return { prop: 'value', order: 'ascending' }
      default:
        return undefined
    }
  },
)
// #endregion

// #region 键刷新 fieldScan

/** 切换键或全量刷新时清空表格筛选等 UI 状态 */
function resetParam() {
  fieldKeyword.value = ''
  fieldExact.value = false
  scanHashFieldTtl.value = false
  listIndexMin.value = ''
  listIndexMax.value = ''
  listDescAsc.value = true
  streamDescAsc.value = true
}

/** 续扫时 cursor 非空，跳过 TYPE/TTL/MEMORY/HLEN 等元数据命令 */
function fieldScanIncludeMeta(): boolean {
  return cursor.value == null
}

/** 组装 fieldScan 参数：count 来自 settings.fieldScanCount（HSCAN COUNT + 前端续扫阈值） */
function buildFieldScanParam() {
  const type = redisValue.value?.type
  const serverScan = supportsFieldServerScan(type)
  const includeMeta = fieldScanIncludeMeta()
  return {
    key: share.redisKey!,
    count: meTauri.settings.fieldScanCount ?? 10,
    cursor: cursor.value,
    match: serverScan ? fieldMatch.value : '*',
    exact: serverScan ? fieldExact.value : false,
    meta: {
      ...meta.value,
      listMinIndex: parseListIndexInput(listIndexMin.value),
      listMaxIndex: parseListIndexInput(listIndexMax.value),
      listDesc: listType.value ? !listDescAsc.value : null,
      streamDesc: streamType.value ? !streamDescAsc.value : null,
      valueByteLimit: VALUE_BYTE_LIMIT.value,
      valuePreviewBytes: VALUE_PREVIEW_BYTES.value,
      forceFullValue: forceFullValue.value,
    },
    // 探测首包或 Auto：base64，便于魔数识别
    bytesFormat:
      probingAutoDetect.value || bytesFormat.value === 'auto'
        ? 'base64'
        : toWireFormat(bytesFormat.value),
    includeMeta,
    keyType: includeMeta ? null : (type ?? null),
    includeFieldTtl: scanHashFieldTtl.value,
  }
}

function toggleHashFieldTtl() {
  scanHashFieldTtl.value = !scanHashFieldTtl.value
  void restartFieldScan()
}

function dismissValueTruncated() {
  valueTruncatedDismissed.value = true
}

/** 用户主动刷新键时重新展示大值预览提示（与切换键时的 reset 不同，保留 forceFullValue） */
function prepareManualKeyRefresh() {
  valueTruncatedDismissed.value = false
}

async function loadFullValue() {
  if (loading.value) return
  forceFullValue.value = true
  await refreshKey(false)
}

/**
 * 「加载更多」专用：把新一页行追加到已有 redisValue.value，避免整表重渲染。
 * 仅 hash/list/set/zset/stream 的行数组可拼接；string/json 等走整包替换。
 * @returns true 已就地 merge；false 调用方应 set replaceData 整包换
 */
function mergeFieldScanPage(
  prev: FieldScanViewState,
  data: FieldScanResult,
  includeMeta: boolean,
): boolean {
  if (!supportsTableView(data.type)) return false
  const merged: unknown[] = [...fieldValueRows(prev.value), ...fieldValueRows(data.value)]
  ;(prev as { value: unknown }).value = merged
  if (includeMeta) {
    // length/ttl/size 随服务端最新统计更新（length 为键内总条数，非当前已加载数）
    prev.length = data.length
    prev.ttl = data.ttl
    prev.size = data.size
  }
  return true
}

/**
 * fieldScan 成功或失败都会走 finally：统一收尾，保证 loading 关闭、编辑器与展示层一致。
 * replaceData 有值表示本次需整包替换 redisValue；undefined 表示已 merge 或无需换对象。
 */
async function finalizeAfterFieldScan(reset: boolean, replaceData?: FieldScanResult) {
  if (replaceData) {
    commitFieldScanReplace(replaceData, reset)
  } else if (reset) {
    // 换键路径若中途已 commit，这里再校正一次；失败清空时仍落到 json
    applyDefaultViewType()
  }
  // 清空未保存编辑；fieldScan 结果即当前权威内容
  if (redisValue.value) {
    redisValue.value.newValue = null
  }
  suppressCodeUpdate.value = false

  // 键类型可能在 scan 后才确定，nextTick 等 computed 更新后再校正编码下拉
  await nextTick(() => {
    if (jsonType.value) {
      commitBytesFormat('utf8')
    } else if (!stringType.value && isStringOnlyView(bytesFormat.value)) {
      commitBytesFormat('utf8')
    }
  })
  // displayWire / displayBytesFormat 与 resolvedWireView 对齐，供 me-code 渲染
  syncDisplaySnapshot()
  await refreshResolvedWireView()
  // 强制 me-code remount：未保存时 modelValue 字符串可能不变，子组件 watch 不触发
  valueEditorRemountKey.value++
  loading.value = false
}

async function fieldScanCore(
  useCursor: boolean,
): Promise<{ count: number; replaceData?: FieldScanResult }> {
  const includeMeta = fieldScanIncludeMeta()
  const data = await meCommands.fieldScan(share.conn!.id, buildFieldScanParam())
  cursor.value = data.cursor
  scanBatchCount.value++

  if (useCursor) {
    const prev = redisValue.value
    if (prev && mergeFieldScanPage(prev, data, includeMeta)) {
      return { count: fieldValueRows(data.value).length }
    }
  }
  return { count: fieldValueRows(data.value).length, replaceData: data }
}

async function fieldScanAuto(fetchedCount = 0): Promise<void> {
  if (!cursor.value || cursor.value.finished) return
  if (scanCancelled.value) return
  if (fetchedCount >= FIELD_SCAN_FETCH_COUNT.value) return

  const { count } = await fieldScanCore(true)
  await fieldScanAuto(fetchedCount + count)
}

async function fieldScanAll(): Promise<void> {
  if (!cursor.value || cursor.value.finished) return
  if (scanCancelled.value) return

  await fieldScanCore(true)
  await fieldScanAll()
}

function shouldFieldScanAuto(type: string | undefined, exact: boolean) {
  if (exact || !type) return false
  // Hash/Set/ZSet pattern 扫描、List/Stream 前端分页循环
  return supportsFieldServerScan(type) || type === 'list' || type === 'stream'
}

/**
 * 拉取/刷新当前键（fieldScan → 更新 redisValue → 同步编辑器）。
 * - reset=true：切换键，清空 fieldKeyword
 * - restart=true：手动刷新 / Enter 搜索，保留 keyword 并中断进行中的扫描
 * 值面板无 F5；F5 仅 KeyMain 刷新键列表。
 */
async function refreshKey(
  reset: boolean = true,
  useCursor: boolean = false,
  loadAll: boolean = false,
  restart: boolean = false,
) {
  if (!share.conn || !share.redisKey) return

  if (loading.value) {
    if (!restart) return
    scanCancelled.value = true
    scanPaused.value = false
    while (loading.value) {
      await sleep(20)
    }
  }

  // 等上一轮结束后再领取，避免被上一轮 finally 清掉后漏探测
  let detectThisLoad = !useCursor && pendingAutoDetect.value
  if (detectThisLoad) pendingAutoDetect.value = false

  fieldSetInit()
  suppressCodeUpdate.value = true
  scanLoadAll.value = loadAll

  if (reset) {
    resetParam()
    forceFullValue.value = false
    valueTruncatedDismissed.value = false
  }
  if (!useCursor) cursor.value = null

  loading.value = true
  scanCancelled.value = false
  if (!useCursor) scanPaused.value = false

  // base64 探测只服务 STRING Auto；已知 List/Hash 等则直接 utf8，避免双次 fieldScan
  if (detectThisLoad && share.conn && share.redisKey) {
    const knownType = await resolveKeyType(share.conn.id, share.conn.db, share.redisKey)
    if (knownType && knownType !== 'STRING') {
      commitBytesFormat('utf8')
      detectThisLoad = false
    }
  }
  probingAutoDetect.value = detectThisLoad

  try {
    if (!useCursor) scanBatchCount.value = 0

    let first = await fieldScanCore(useCursor)
    if (first.replaceData) {
      commitFieldScanReplace(first.replaceData, reset)
    }

    let scanType = redisValue.value?.type
    // 换键探测：目标编码进中间量，与当前相同时不改下拉
    if (detectThisLoad) {
      const nextFormat: ViewBytesFormat = scanType === 'string' ? 'auto' : 'utf8'
      commitBytesFormat(nextFormat)
      // 首包探测结束；非 STRING 需按 utf8 重拉（类型缓存未命中时的兜底）
      probingAutoDetect.value = false
      if (nextFormat === 'utf8') {
        cursor.value = null
        scanBatchCount.value = 0
        first = await fieldScanCore(false)
        if (first.replaceData) {
          commitFieldScanReplace(first.replaceData, reset)
        }
        scanType = redisValue.value?.type
      }
    } else if (
      !useCursor &&
      scanType &&
      scanType !== 'string' &&
      isStringOnlyView(bytesFormat.value)
    ) {
      // 手动停在 Auto 时打开了非 STRING（兜底）
      commitBytesFormat('utf8')
      cursor.value = null
      scanBatchCount.value = 0
      first = await fieldScanCore(false)
      if (first.replaceData) {
        commitFieldScanReplace(first.replaceData, reset)
      }
      scanType = redisValue.value?.type
    }

    if (loadAll) {
      await fieldScanAll()
    } else if (shouldFieldScanAuto(scanType, fieldExact.value)) {
      await fieldScanAuto(first.count)
    }

    showMore.value = !cursor.value?.finished
    const rvDone = redisValue.value
    if (rvDone) await setTimer(rvDone.ttl)
  } finally {
    probingAutoDetect.value = false
    if (!stringType.value && isStringOnlyView(bytesFormat.value)) {
      commitBytesFormat('utf8')
    }
    await finalizeAfterFieldScan(reset)
    if (cursor.value?.finished) scanPaused.value = false
  }
}
// #endregion

// #region TTL 倒计时
let timer: ReturnType<typeof setInterval> | null = null

async function setTimer(seconds: number) {
  const rv = redisValue.value
  if (!rv) return
  rv.ttl = seconds
  if (timer !== null) clearInterval(timer)
  timer = null
  if (rv.ttl > 0) {
    timer = setInterval(() => {
      const cur = redisValue.value
      if (cur && cur.ttl > 0) cur.ttl--
    }, 1000)
  }
}

const ttlSetRef = useTemplateRef('ttlSetRef')
function updateTTL() {
  if (!canEdit.value) return
  const rv = redisValue.value
  if (!rv) return
  ttlSetRef.value?.open({ ttl: rv.ttl })
}

const ttlDisplayText = computed(() => {
  const rv = redisValue.value
  if (!rv) return ''
  return rv.ttl === -1 ? t('redisValue.ttlForever') : meHumanSeconds(rv.ttl)
})

const ttlIconHint = computed(() => {
  return canEdit.value ? t('redisValue.ttlHint') : t('redisValue.ttlHintReadonly')
})
// #endregion

// #region 键级操作（重命名 / 删除）
function deleteKey(_payload?: RedisKey_Deserialize) {
  redisValue.value = null
}

function delKey() {
  meDeleteKey(share.conn!.id, share.redisKey!)
}

const keyRenameRef = useTemplateRef<InstanceType<typeof KeyRename>>('keyRenameRef')
function renameKey() {
  if (!share.redisKey) return
  keyRenameRef.value?.open({ redisKey: share.redisKey })
}

const objectInfoRef = useTemplateRef<InstanceType<typeof ObjectInfo>>('objectInfoRef')

function duplicateKey() {
  if (!share.redisKey) return
  connUi.openKeyCopy(share.redisKey)
}

const copyAsCommandLoading = ref(false)

async function copyAsCommand() {
  const conn = share.conn
  const rk = share.redisKey
  if (!conn || !rk || copyAsCommandLoading.value) return
  copyAsCommandLoading.value = true
  try {
    const text = await meCommands.getKeyAsCommand(conn.id, rk)
    if (!text.trim()) {
      meWarn(t('redisValue.copyCommandEmpty'))
      return
    }
    meCopy(text, t('redisValue.copyCommandOk'))
  } finally {
    copyAsCommandLoading.value = false
  }
}

async function onFooterRefreshKey() {
  await manualRefreshKey()
  meOk(t('redisValue.refreshKeyOk'))
}

function buildFieldAsCommandParam(row: ValueTableRow): RedisFieldAsCommand_Deserialize | null {
  const rv = redisValue.value
  const rk = share.redisKey
  if (!rv || !rk) return null
  const fieldViewFmt = viewFmtForField(bytesFormat.value)
  const param: RedisFieldAsCommand_Deserialize = {
    key: rk,
    fieldKey: row.key || '',
    fieldValue: String(row.value ?? ''),
    streamId: row.id || '',
    fieldIndex: -1,
    valFmt: toWireFormat(fieldViewFmt),
  }
  if (rv.type === 'list') {
    param.fieldIndex = listRowRedisIndex(row)
  }
  if (rv.type === 'stream') {
    param.fieldValue = ''
  }
  return param
}

async function copyFieldAsCommand(row: ValueTableRow) {
  const conn = share.conn
  const param = buildFieldAsCommandParam(row)
  if (!conn || !param) return
  const text = await meCommands.getFieldAsCommand(conn.id, param)
  if (!text.trim()) {
    meWarn(t('redisValue.copyCommandEmpty'))
    return
  }
  meCopy(text, t('redisValue.copyCommandOk'))
}

// 收藏（与 KeyTree 右键菜单一致）
const favorites = useFavorites()
const isCurrentKeyFavorited = computed(() => {
  const conn = share.conn
  const rk = share.redisKey
  if (!conn || !rk) return false
  return isFavorited(favorites.value, conn.id, conn.db, rk.bytes)
})

function toggleFavorite() {
  const conn = share.conn
  const rk = share.redisKey
  if (!conn || !rk) return
  if (isCurrentKeyFavorited.value) {
    favorites.value = removeFavorite(favorites.value, conn.id, conn.db, rk.bytes)
    meOk(t('keyTree.unfavoriteOk'))
  } else {
    favorites.value = addFavorite(favorites.value, conn.id, conn.db, rk)
    meOk(t('keyTree.favoriteOk'))
  }
}

async function onKeyMoreCommand(command: string) {
  if (command === 'refreshKey') {
    await onFooterRefreshKey()
  } else if (command === 'copyKey') {
    meCopy(showKey.value)
  } else if (command === 'copyValue') {
    meCopy(showValue.value)
  } else if (command === 'copyAsCommand') {
    void copyAsCommand()
  } else if (command === 'renameKey') {
    renameKey()
  } else if (command === 'duplicateKey') {
    duplicateKey()
  } else if (command === 'objectInfo') {
    objectInfoRef.value?.open()
  } else if (command === 'showSlot') {
    void showSlot()
  } else if (command === 'showLocation') {
    void showLocation()
  } else if (command === 'commandHelp') {
    openCommandHelp()
  } else if (command === 'keyShort') {
    openKeyShortDialog()
  }
}
// #endregion

// #region 保存整键值（STRING / JSON）
async function setValue() {
  const rv = redisValue.value
  if (!rv || rv.newValue === null) return
  let value = rv.newValue

  try {
    if (jsonType.value) {
      if (value === '') {
        meErr(t('fieldAdd.jsonValidator'))
        return
      }
      value = meJsonNormal(value)
    } else if (stringType.value && needsJsonNormalize(effectiveViewFormat.value)) {
      value = value === '' ? '' : meJsonNormal(value)
    }
    if (stringType.value && isCustomView(effectiveViewFormat.value)) {
      value = await meViewToWireAsync(value, effectiveViewFormat.value)
    } else if (stringType.value && effectiveViewFormat.value !== 'utf8') {
      value = meViewToWire(value, effectiveViewFormat.value)
    }
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e)
    if (stringType.value && isCustomView(effectiveViewFormat.value)) {
      setCustomCodecError(msg)
      rv.newValue = null
      valueEditorRemountKey.value++
      return
    }
    meErr(msg)
    return
  }

  await meCommands.set(share.conn!.id, {
    key: share.redisKey!,
    value,
    ttl: rv.ttl,
    keyType: rv.type,
    inputFormat: toWireFormat(effectiveViewFormat.value),
  })
  meOk(t('saveOk'))
  await refreshKey()
}
// #endregion

// #region 字段级操作（新增 / 编辑 / 删除）
const fieldAddRef = useTemplateRef('fieldAddRef')
function fieldAdd() {
  const rv = redisValue.value
  if (!rv) return
  fieldAddRef.value?.open({
    mode: 'field',
    type: rv.type,
    valFmt: toWireFormat(viewFmtForField(bytesFormat.value)),
    viewValFmt: viewFmtForField(bytesFormat.value),
    key: { ...share.redisKey! },
  })
}

const fieldSetIndex = ref(-1)
const fieldSetReadonly = ref(false)
/** 单行刷新：list 在 value 数组中的下标；hash 为字段 wire key */
const fieldEditIndex = ref(-1)
const fieldEditKey = ref('')
/** 编辑面板当前行（分页下不能用 fieldSetIndex 索引 filterDataList） */
const fieldSetRow = ref<ValueTableRow | null>(null)
const fieldSetRef = useTemplateRef('fieldSetRef')

function pageRowIndexFromEvent(event: MouseEvent): number {
  const tr = event.currentTarget as HTMLElement | null
  if (!tr) return -1
  for (const className of tr.classList) {
    if (className.startsWith('table-row-index-')) {
      return Number.parseInt(className.slice('table-row-index-'.length), 10)
    }
  }
  return -1
}

function fieldSetInit() {
  fieldSetIndex.value = -1
  fieldSetReadonly.value = false
  fieldEditIndex.value = -1
  fieldEditKey.value = ''
  fieldSetRow.value = null
  fieldSetRef.value?.close()
}

function prepareFieldRowContext(row: ValueTableRow) {
  const rv = redisValue.value
  fieldEditKey.value = row.key || ''
  fieldEditIndex.value = -1
  if (rv?.type === 'list') {
    fieldEditIndex.value = listRowRedisIndex(row)
  }
}

function formatFieldTtl(ttl: number | undefined): string {
  if (ttl === undefined || ttl === null) return '-'
  if (ttl === -1) return t('redisValue.ttlForever')
  return String(meHumanSeconds(ttl))
}

function buildFieldGetParam(row?: ValueTableRow): RedisFieldGet_Deserialize | null {
  const rv = redisValue.value
  const rk = share.redisKey
  if (!rv || !rk) return null
  return {
    key: rk,
    fieldIndex: fieldEditIndex.value,
    fieldKey: fieldEditKey.value,
    fieldValue: rv.type === 'zset' && row ? String(row.value ?? '') : '',
    valFmt: toWireFormat(viewFmtForField(bytesFormat.value)),
    includeFieldTtl: rv.type === 'hash' ? scanHashFieldTtl.value : null,
  }
}

function fieldRowDisplayValue(row: ValueTableRow): string {
  return streamType.value ? JSON.stringify(row.value) : formatTableCell(row.value)
}

/** 值列排序：与单元格展示一致（Stream 等为 JSON 字符串） */
function compareFieldRowValue(a: ValueTableRow, b: ValueTableRow): number {
  return fieldRowDisplayValue(a).localeCompare(fieldRowDisplayValue(b), undefined, {
    numeric: true,
    sensitivity: 'base',
  })
}

function openFieldPanel(row: ValueTableRow, index: number, readonly: boolean) {
  const rv = redisValue.value
  if (!rv) return
  fieldSetIndex.value = index
  fieldSetReadonly.value = readonly
  fieldSetRow.value = row
  prepareFieldRowContext(row)
  const rowValWire =
    rv.type === 'stream' ? JSON.stringify(row.value ?? {}) : String(row.value ?? '')
  const params = {
    fieldKey: row.key || '',
    fieldScore: row.score || 0,
    fieldTtl: row.ttl ?? -1,
    srcFieldValue: rowValWire,
    wireFieldKey: row.key || '',
    keyWireFmt: toWireFormat(bytesFormat.value),
    keyViewFmt: bytesFormat.value,
    type: rv.type,
    key: share.redisKey!,
    fieldIndex: -1,
    streamId: row.id || '',
    readonly,
  }
  if (rv.type === 'list') {
    params.fieldIndex = fieldEditIndex.value
  }
  fieldSetRef.value?.open(params)
}

function rowClassName({ rowIndex }: { row: ValueTableRow; rowIndex: number }) {
  const classes = [`table-row-index-${rowIndex}`]
  if (fieldSetIndex.value === rowIndex) classes.push('field-set-row')
  return classes.join(' ')
}

function rowDblClick(row: ValueTableRow, _column: unknown, event: MouseEvent) {
  if ((event.target as HTMLElement)?.closest('.field-row-actions')) return
  const rowIndex = pageRowIndexFromEvent(event)
  if (rowIndex < 0) return
  openFieldPanel(row, rowIndex, !(canEdit.value && !streamType.value))
}

function rowClick(row: ValueTableRow, _column: unknown, event: MouseEvent) {
  if (fieldSetIndex.value === -1) return
  const rowIndex = pageRowIndexFromEvent(event)
  if (rowIndex < 0) return
  openFieldPanel(row, rowIndex, fieldSetReadonly.value)
}

/** 编辑面板打开时：点表格行切换内容；点面板外空白/表头等关闭 */
function onFieldPanelOutsideClick(e: MouseEvent) {
  if (fieldSetIndex.value === -1) return
  const el = e.target as HTMLElement | null
  if (!el) return
  if (el.closest('.field-set')) return
  if (el.closest('.el-table__body tbody tr')) return
  fieldSetInit()
}

/** 将 field_get 结果写回表格对应行（就地更新，避免整表 fieldScan） */
function applyFieldGetResult(rv: FieldScanViewState, data: RedisFieldValue, row: ValueTableRow) {
  if (rv.type === 'hash') {
    const rows = fieldValueRows(rv.value) as ValueTableRow[]
    const idx = rows.findIndex(r => r.key === (row.key || fieldEditKey.value))
    if (idx >= 0) {
      rows[idx] = {
        key: data.fieldKey,
        value: data.fieldValue,
        ttl: scanHashFieldTtl.value ? data.fieldTtl : (rows[idx].ttl ?? row.ttl),
      }
    }
  } else if (rv.type === 'list') {
    const rows = fieldValueRows(rv.value) as ValueTableRow[]
    const redisIndex = fieldEditIndex.value >= 0 ? fieldEditIndex.value : listRowRedisIndex(row)
    const idx = rows.findIndex(r => r.index === redisIndex)
    if (idx >= 0) {
      rows[idx] = { index: rows[idx].index, value: data.fieldValue }
    }
  } else if (rv.type === 'zset') {
    const rows = fieldValueRows(rv.value) as ValueTableRow[]
    const idx = rows.findIndex(r => r.value === row.value)
    if (idx >= 0) {
      rows[idx] = { value: data.fieldValue, score: data.fieldScore ?? row.score }
    }
  }
}

/** 单行 field_get 刷新；不支持的类型回退 refreshKey */
async function refreshFieldRow(row: ValueTableRow) {
  const rv = redisValue.value
  const conn = share.conn
  if (!rv || !conn || !share.redisKey) return
  prepareFieldRowContext(row)

  if (rv.type === 'hash' || rv.type === 'list' || rv.type === 'zset') {
    const param = buildFieldGetParam(row)
    if (!param) return
    try {
      const data = await meCommands.fieldGet(conn.id, param, false)
      applyFieldGetResult(rv, data, row)
      meOk(t('redisValue.refreshFieldRowOk'))
      return
    } catch {
      // 回退整表刷新
    }
  }
  await refreshKey(false)
}

function onFieldRowMoreCommand(command: string, row: ValueTableRow) {
  if (command === 'refreshRow') {
    void refreshFieldRow(row)
  } else if (command === 'copyKey') {
    meCopy(String(row.key ?? ''))
  } else if (command === 'copyValue') {
    meCopy(fieldRowDisplayValue(row))
  } else if (command === 'copyIndex') {
    meCopy(String(row.index ?? ''))
  } else if (command === 'copyStreamId') {
    meCopy(String(row.id ?? ''))
  } else if (command === 'copyScore') {
    meCopy(String(row.score ?? ''))
  } else if (command === 'copyAsCommand') {
    void copyFieldAsCommand(row)
  } else if (command === 'showZsetRank') {
    void showZsetRank(row)
  }
}

async function showZsetRank(row: ValueTableRow) {
  const conn = share.conn
  const rk = share.redisKey
  if (!conn || !rk) return
  const member = fieldRowDisplayValue(row)
  const data: RedisZsetRankResult = await meCommands.zsetRank(conn.id, {
    key: rk,
    member,
    valFmt: toWireFormat(viewFmtForField(bytesFormat.value)),
  })
  const rankText = data.rank !== null ? String(data.rank) : t('redisValue.rankNotFound')
  const revRankText = data.revRank !== null ? String(data.revRank) : t('redisValue.rankNotFound')
  meOk(
    `${t('redisValue.rank')}: ${rankText}<br>${t('redisValue.revRank')}: ${revRankText}`,
    true,
    t('redisValue.rankTitle'),
    { dangerouslyUseHTMLString: true },
  )
}

function onFieldSetRefreshed(data: RedisFieldValue) {
  const rv = redisValue.value
  const row = fieldSetRow.value
  if (!rv || !row) return
  applyFieldGetResult(rv, data, row)
}

/** 字段保存成功后优先 field_get 刷新单行；不支持或失败时回退整表 refreshKey */
async function onFieldSetSuccess() {
  const rv = redisValue.value
  if (!rv || !share.redisKey || (rv.type !== 'hash' && rv.type !== 'list')) {
    await refreshKey(false)
    fieldSetInit()
    return
  }

  const param = buildFieldGetParam()
  if (!param) {
    await refreshKey(false)
    fieldSetInit()
    return
  }
  try {
    const data = await meCommands.fieldGet(share.conn!.id, param, false)
    const row = fieldSetRow.value
    if (row) applyFieldGetResult(rv, data, row)
    fieldSetInit()
  } catch {
    await refreshKey(false)
    fieldSetInit()
  }
}

async function fieldDel(row: ValueTableRow) {
  const rv = redisValue.value
  if (!rv) return
  const fieldViewFmt = viewFmtForField(bytesFormat.value)
  const param: RedisFieldDel_Deserialize = {
    fieldKey: row.key || '',
    fieldValue: String(row.value ?? ''),
    key: share.redisKey!,
    streamId: row.id || '',
    fieldIndex: -1,
    valFmt: toWireFormat(fieldViewFmt),
  }
  if (rv.type === 'list') {
    param.fieldIndex = listRowRedisIndex(row)
  }
  if (rv.type === 'stream') {
    param.fieldValue = ''
  }

  await meCommands.fieldDel(share.conn!.id, param)
  meOk(t('deleteOk'))
  await refreshKey()
}
// #endregion

// #region Stream 扩展（Groups / ID 时间）
function streamIdToDate(id: string) {
  try {
    const timestamp = Number.parseInt(id.split('-')[0]!, 10)
    if (!Number.isFinite(timestamp)) return ''
    return dayjs(timestamp).format('YYYY-MM-DD HH:mm:ss.SSS')
  } catch {
    return ''
  }
}

const tableGroupRef = useTemplateRef('tableGroupRef')
function showGroups() {
  tableGroupRef.value?.open()
}

const hashKeysRef = useTemplateRef('hashKeysRef')
function hashListValFmt() {
  return toWireFormat(viewFmtForField(bytesFormat.value))
}
function showAllHashKeys() {
  hashKeysRef.value?.open(hashListValFmt(), 'keys')
}
function showAllHashValues() {
  hashKeysRef.value?.open(hashListValFmt(), 'values')
}

const zsetRangeRef = useTemplateRef('zsetRangeRef')
function showZsetRange() {
  zsetRangeRef.value?.open(hashListValFmt())
}
// #endregion

// #region 底部信息栏（内存 / 条数 / 槽位）
const textMemory = computed(() => {
  const rv = redisValue.value
  if (!rv) return ''
  let sz = rv.size
  let estimated = false
  // 兼容不支持 MEMORY USAGE 的 Redis 变体：String 按键名+值长度粗估
  if (sz <= 0 && stringType.value) {
    const key = share.redisKey?.key ?? ''
    sz = estimateStringMemory(key, rv.length)
    estimated = true
  }
  if (sz <= 0) return ''
  const label = estimated ? t('redisValue.textMemoryEstimate') : t('redisValue.textMemory')
  return label + meHumanSize(sz)
})

/** 与 textLength 同一位置：String/单字段为字节长度，集合类型为总数 */
const textLength = computed(() => {
  const rv = redisValue.value
  if (!rv || jsonType.value) return ''
  if (stringType.value) {
    return t('redisValue.textLength') + rv.length
  }
  if (rv.length <= 0) return ''
  return t('redisValue.totalCount') + rv.length
})

const textEntries = computed(() => {
  const rv = redisValue.value
  if (!rv || jsonType.value || stringType.value) return ''
  const filtered = tableDisplayList.value.length
  const loaded = fieldValueRows(rv.value).length
  return t('redisValue.textEntries') + `${filtered} / ${loaded}`
})

const showKey = computed(() => {
  const rk = share.redisKey
  if (!rk) return ''
  return rk.key
})

async function showSlot() {
  const data = await meCommands.keySlot(share.conn!.id, share.redisKey!)
  meOk(String(data), true, t('redisValue.slotTitle'))
}

async function showLocation() {
  const data = await meCommands.keyNode(share.conn!.id, share.redisKey!)
  const msg = data.map(item => item.node + ' | ' + item.flags.toUpperCase()).join('<br>')
  meOk(msg, true, t('redisValue.locationTitle'), { dangerouslyUseHTMLString: true })
}

function locateKeyInTree(): void {
  const rk = share.redisKey
  if (!rk) return
  connUi.scrollKeyToTree(rk)
}
// #endregion

// #region 快捷键说明弹窗
const valueShortcutRef = useTemplateRef('valueShortcutRef')
function openKeyShortDialog() {
  valueShortcutRef.value?.open()
}
// #endregion

// #region 命令帮助弹窗
const commandHelpRef = useTemplateRef<InstanceType<typeof CommandHelp>>('commandHelpRef')

/** 键类型到命令分组 group 的映射 */
const KEY_TYPE_TO_GROUP: Record<string, string> = {
  string: 'string',
  hash: 'hash',
  list: 'list',
  set: 'set',
  zset: 'sorted-set',
  stream: 'stream',
  json: 'json',
}

function openCommandHelp() {
  const type = redisValue.value?.type
  const group = type ? KEY_TYPE_TO_GROUP[type] : ''
  commandHelpRef.value?.open({ group })
}
// #endregion

// #region 事件总线与生命周期
/** 选中键时加载值（KEY_REFRESH）；与 KeyMain F5 刷新键列表无关 */
const onKeyRefreshBus = () => {
  // 换键标记待探测；非 STRING 会在 refreshKey 内用类型缓存跳过 base64 首包
  pendingAutoDetect.value = true
  // restart：快速连点不同键时不丢弃后一次
  void refreshKey(true, false, false, true)
}

onMounted(() => {
  bus.on(KEY_REFRESH, onKeyRefreshBus)
  bus.on(KEY_DELETE, deleteKey)
})

onUnmounted(() => {
  bus.off(KEY_REFRESH, onKeyRefreshBus)
  bus.off(KEY_DELETE, deleteKey)
  if (timer) clearInterval(timer)
})
// #endregion
</script>

<template>
  <!-- 扫描进度由搜索框内的进度环展示，避免 loading 遮罩拦截暂停/继续操作 -->
  <div class="redis-value">
    <template v-if="share.redisKey && redisValue">
      <!-- 上方键 -->
      <div class="value-header">
        <div class="value-header-main">
          <el-input type="text" v-model="showKey" readonly class="value-header-input">
            <template #prepend>
              <me-icon
                icon="me-icon-location"
                class="suffix-ttl icon-btn"
                icon-left
                :name="redisValue.type.toUpperCase()"
                :info="t('redisValue.locateKeyHint')"
                placement="top"
                @click.stop="locateKeyInTree" />
            </template>
            <template #suffix>
              <span class="ttl-suffix-separator">|</span>
              <me-icon
                icon="el-icon-timer"
                class="suffix-ttl icon-btn"
                icon-left
                :name="ttlDisplayText"
                :info="ttlIconHint"
                placement="top"
                @click.stop="updateTTL" />
            </template>
          </el-input>
        </div>

        <div class="value-header-actions">
          <me-icon
            :icon="isCurrentKeyFavorited ? 'el-icon-star-filled' : 'el-icon-star'"
            :class="['icon-btn', { 'is-favorited': isCurrentKeyFavorited }]"
            :name="isCurrentKeyFavorited ? t('keyTree.unfavoriteKey') : t('keyTree.favoriteKey')"
            hint
            placement="top"
            @click="toggleFavorite" />
          <me-icon
            v-if="canEdit"
            icon="el-icon-delete"
            class="icon-btn"
            :name="t('redisValue.deleteKey')"
            hint
            placement="top"
            @click="delKey" />
          <el-dropdown placement="bottom-end" @command="onKeyMoreCommand">
            <me-icon icon="el-icon-more-filled" class="icon-btn" />
            <template #dropdown>
              <el-dropdown-menu>
                <el-dropdown-item command="refreshKey">
                  <me-icon icon="el-icon-refresh-right" :name="t('redisValue.refreshKey')" />
                </el-dropdown-item>
                <el-dropdown-item command="copyKey">
                  <me-icon icon="el-icon-document-copy" :name="t('keyTree.copyKey')" />
                </el-dropdown-item>
                <el-dropdown-item command="copyValue">
                  <me-icon icon="el-icon-document-copy" :name="t('redisValue.copyValue')" />
                </el-dropdown-item>
                <el-dropdown-item command="copyAsCommand" :disabled="copyAsCommandLoading">
                  <me-icon icon="me-icon-copy-command" :name="t('redisValue.copyAsCommand')" />
                </el-dropdown-item>
                <el-dropdown-item v-if="canEdit" command="renameKey">
                  <me-icon icon="el-icon-edit" :name="t('redisValue.renameKey')" />
                </el-dropdown-item>
                <el-dropdown-item v-if="canEdit" command="duplicateKey">
                  <me-icon icon="el-icon-copy-document" :name="t('redisValue.duplicateKey')" />
                </el-dropdown-item>
                <el-dropdown-item v-if="share.conn?.cluster" command="showSlot" divided>
                  <me-icon icon="me-icon-slot" :name="t('redisValue.slotTitle')" />
                </el-dropdown-item>
                <el-dropdown-item v-if="share.conn?.cluster" command="showLocation">
                  <me-icon icon="el-icon-location" :name="t('redisValue.locationTitle')" />
                </el-dropdown-item>
                <el-dropdown-item command="objectInfo" divided>
                  <me-icon icon="el-icon-info-filled" :name="t('redisValue.objectInfo')" />
                </el-dropdown-item>
                <el-dropdown-item command="commandHelp">
                  <me-icon icon="el-icon-help" :name="t('redisValue.commandHelp')" />
                </el-dropdown-item>
                <el-dropdown-item command="keyShort">
                  <me-icon icon="me-icon-keyshort" :name="t('redisValue.keyShortHint')" />
                </el-dropdown-item>
              </el-dropdown-menu>
            </template>
          </el-dropdown>
        </div>
      </div>

      <!-- 中间值 -->
      <div class="value-main">
        <el-alert
          v-if="showValueTruncatedAlert"
          type="warning"
          :title="t('redisValue.valueTruncatedTitle')"
          show-icon
          :closable="false"
          class="value-truncated-alert">
          <p class="value-truncated-desc">
            {{
              t('redisValue.valueTruncatedDesc', {
                size: meHumanSize(redisValue?.length ?? 0),
                limit: meHumanSize(VALUE_BYTE_LIMIT),
                preview: VALUE_PREVIEW_BYTES,
              })
            }}
          </p>
          <div class="value-truncated-actions">
            <el-button size="small" @click="dismissValueTruncated">
              {{ t('redisValue.valueTruncatedDismiss') }}
            </el-button>
            <el-button size="small" type="warning" plain :disabled="loading" @click="loadFullValue">
              {{ t('redisValue.valueTruncatedLoadAll') }}
            </el-button>
          </div>
        </el-alert>
        <!-- json显示 -->
        <me-code
          v-if="viewType === 'json'"
          :key="valueEditorRemountKey"
          :modelValue="showValue"
          @update:modelValue="onCodeUpdate"
          :read-only="!canSave" />

        <!-- 表格显示 -->
        <div
          class="me-flex"
          style="flex-direction: column; height: 100%"
          v-else
          @click="onFieldPanelOutsideClick">
          <div class="me-flex table-toolbar">
            <el-input
              v-model="fieldKeyword"
              :placeholder="fieldScanInputPlaceholder"
              :readonly="loading"
              clearable
              class="field-scan-input"
              @keyup.enter="onFieldSearch">
              <template #suffix>
                <div class="keyword-suffix">
                  <el-tooltip
                    v-if="showScanControl"
                    :content="scanToggleTip"
                    placement="bottom"
                    :show-after="1000">
                    <div class="scan-control" @click.stop="onFieldScanAction">
                      <el-progress
                        type="circle"
                        :percentage="scanProgress"
                        :width="22"
                        :stroke-width="2"
                        :show-text="false"
                        color="var(--el-color-danger)"
                        class="scan-ring" />
                      <me-icon
                        :icon="loading ? 'el-icon-video-pause' : 'el-icon-video-play'"
                        class="scan-icon" />
                    </div>
                  </el-tooltip>
                  <el-tooltip
                    v-if="showFieldExactCheckbox"
                    :content="t('redisValue.fieldExactSearch')"
                    placement="bottom"
                    raw-content
                    :show-after="1000">
                    <el-checkbox size="small" v-model="fieldExact" class="suffix-exact-checkbox" />
                  </el-tooltip>
                </div>
              </template>
            </el-input>

            <div v-if="streamType" class="stream-range-inputs">
              <el-input
                @keyup.enter="restartFieldScan()"
                v-model.trim="meta.minId"
                placeholder="MinId"
                clearable />
              <span class="stream-range-sep">-</span>
              <el-input
                @keyup.enter="restartFieldScan()"
                v-model.trim="meta.maxId"
                placeholder="MaxId"
                clearable />
            </div>

            <div v-if="listType" class="list-range-inputs">
              <el-input
                @keyup.enter="restartFieldScan()"
                v-model.trim="listIndexMin"
                :placeholder="t('redisValue.listIndexMin')"
                clearable />
              <span class="list-range-sep">-</span>
              <el-input
                @keyup.enter="restartFieldScan()"
                v-model.trim="listIndexMax"
                :placeholder="t('redisValue.listIndexMax')"
                clearable />
            </div>

            <!-- 右侧更多+插入行 -->
            <div class="table-toolbar-actions">
              <me-button
                v-if="showHashFieldTtlOption"
                icon="el-icon-clock"
                :type="scanHashFieldTtl ? 'primary' : 'default'"
                style="margin-left: 10px"
                @click="toggleHashFieldTtl">
                HTTL
              </me-button>
              <el-button
                v-if="streamType"
                :icon="streamDescAsc ? 'el-icon-sort-up' : 'el-icon-sort-down'"
                @click="toggleStreamSortOrder"
                style="margin-left: 10px">
                {{ streamDescAsc ? t('redisValue.listSortAsc') : t('redisValue.listSortDesc') }}
              </el-button>
              <el-button
                icon="el-icon-grid"
                @click="showGroups"
                style="margin-left: 10px"
                v-if="streamType">
                Groups
              </el-button>
              <el-button
                v-if="hashType"
                icon="el-icon-key"
                @click="showAllHashKeys"
                style="margin-left: 10px">
                {{ t('redisValue.allHashKeys') }}
              </el-button>
              <el-button
                v-if="hashType"
                icon="el-icon-document"
                @click="showAllHashValues"
                style="margin-left: 10px">
                {{ t('redisValue.allHashValues') }}
              </el-button>
              <el-button
                v-if="listType"
                :icon="listDescAsc ? 'el-icon-sort-up' : 'el-icon-sort-down'"
                @click="toggleListSortOrder"
                style="margin-left: 10px">
                {{ listDescAsc ? t('redisValue.listSortAsc') : t('redisValue.listSortDesc') }}
              </el-button>
              <me-button
                v-if="zsetType"
                icon="me-icon-rank"
                @click="showZsetRange"
                style="margin-left: 10px">
                {{ t('redisValue.zsetRange') }}
              </me-button>
              <el-dropdown
                v-if="(listType || setType || zsetType) && canEdit"
                placement="bottom-end"
                @command="onPopCommand"
                style="margin-left: 10px">
                <el-button icon="el-icon-arrow-down">
                  {{ t('redisValue.fieldCommands') }}
                </el-button>
                <template #dropdown>
                  <el-dropdown-menu>
                    <el-dropdown-item v-if="listType" command="LPOP">LPOP</el-dropdown-item>
                    <el-dropdown-item v-if="listType" command="RPOP">RPOP</el-dropdown-item>
                    <el-dropdown-item v-if="setType" command="SPOP">SPOP</el-dropdown-item>
                    <el-dropdown-item v-if="zsetType" command="ZPOPMIN">ZPOPMIN</el-dropdown-item>
                    <el-dropdown-item v-if="zsetType" command="ZPOPMAX">ZPOPMAX</el-dropdown-item>
                  </el-dropdown-menu>
                </template>
              </el-dropdown>
              <el-button icon="el-icon-plus" @click="fieldAdd" style="margin-left: 10px">{{
                t('redisValue.insertRow')
              }}</el-button>
            </div>
          </div>
          <div class="table-view">
            <me-table
              :key="redisValue?.type"
              layout="sizes, prev, pager, next, jumper"
              :data="tableDisplayList"
              :default-sort="tableDefaultSort"
              border
              stripe
              ref="table"
              height="100%"
              export-name="value"
              :row-class-name="rowClassName"
              @row-click="rowClick"
              @row-dblclick="rowDblClick">
              <!-- 索引 -->
              <el-table-column
                label="#"
                type="index"
                width="50"
                align="center"
                show-overflow-tooltip>
                <template #default="scope">
                  <div class="index-cell">
                    <template v-if="fieldSetIndex !== scope.$index">{{
                      scope.$index + 1
                    }}</template>
                    <me-icon
                      v-else
                      :icon="fieldSetReadonly ? 'el-icon-view' : 'el-icon-edit'"
                      :style="{ color: share.color }"></me-icon>
                  </div>
                </template>
              </el-table-column>

              <!-- Stream ID -->
              <el-table-column
                :label="t('redisValue.id')"
                prop="id"
                width="350"
                sortable
                show-overflow-tooltip
                v-if="redisValue.type === 'stream'">
                <template #default="{ row }">
                  <div class="me-flex" style="width: 100%">
                    <span>{{ row.id }}</span>
                    <span v-if="streamIdToDate(row.id)" style="color: var(--el-color-info)">
                      {{ streamIdToDate(row.id) }}
                    </span>
                  </div>
                </template>
              </el-table-column>

              <!-- 哈希键 -->
              <el-table-column
                :label="t('redisValue.key')"
                prop="key"
                sortable
                show-overflow-tooltip
                v-if="redisValue.type === 'hash'">
                <template #default="scope">
                  {{ formatTableCell(scope.row.key) }}
                </template>
              </el-table-column>

              <!-- List 索引 -->
              <el-table-column
                :label="t('redisValue.index')"
                prop="index"
                width="100"
                sortable
                show-overflow-tooltip
                v-if="redisValue.type === 'list'" />

              <!-- 字段值 -->
              <el-table-column
                :label="t('redisValue.value')"
                prop="value"
                min-width="200"
                sortable
                :sort-method="compareFieldRowValue"
                show-overflow-tooltip>
                <template #default="scope">
                  {{ fieldRowDisplayValue(scope.row) }}
                </template>
              </el-table-column>

              <!-- 分数 -->
              <el-table-column
                :label="t('redisValue.score')"
                prop="score"
                width="140"
                sortable
                show-overflow-tooltip
                v-if="redisValue.type === 'zset'" />

              <!-- TTL -->
              <el-table-column
                :label="t('redisValue.ttl')"
                width="140"
                prop="ttl"
                v-if="showHashFieldTtlOption && scanHashFieldTtl">
                <template #default="scope">
                  {{ formatFieldTtl(scope.row.ttl) }}
                </template>
              </el-table-column>

              <!-- 操作 -->
              <el-table-column :label="t('action')" width="80" fixed="right" align="center">
                <template #default="scope">
                  <div class="field-row-actions me-flex" style="justify-content: center; gap: 8px">
                    <me-icon
                      v-if="canEdit && !streamType"
                      :info="t('edit')"
                      icon="el-icon-edit"
                      class="icon-btn"
                      @click.stop="openFieldPanel(scope.row, scope.$index, false)" />
                    <me-icon
                      v-else
                      :info="t('view')"
                      icon="el-icon-view"
                      class="icon-btn"
                      @click.stop="openFieldPanel(scope.row, scope.$index, true)" />
                    <el-popconfirm
                      v-if="canEdit"
                      :hide-after="0"
                      :title="t('redisValue.deleteConfirm')"
                      @confirm.stop="fieldDel(scope.row)">
                      <template #reference>
                        <me-icon :info="t('delete')" icon="el-icon-delete" class="icon-btn" />
                      </template>
                    </el-popconfirm>
                    <el-dropdown
                      trigger="click"
                      placement="bottom-end"
                      @command="(cmd: string) => onFieldRowMoreCommand(cmd, scope.row)">
                      <me-icon icon="el-icon-more-filled" class="icon-btn" />
                      <template #dropdown>
                        <el-dropdown-menu>
                          <el-dropdown-item
                            v-if="supportsFieldRowRefresh(redisValue.type)"
                            command="refreshRow">
                            <me-icon
                              icon="el-icon-refresh-right"
                              :name="t('redisValue.refreshFieldRow')" />
                          </el-dropdown-item>
                          <el-dropdown-item v-if="hashType" command="copyKey">
                            <me-icon icon="el-icon-document-copy" :name="t('redisValue.copyKey')" />
                          </el-dropdown-item>
                          <el-dropdown-item v-if="listType" command="copyIndex">
                            <me-icon
                              icon="el-icon-document-copy"
                              :name="t('redisValue.copyIndex')" />
                          </el-dropdown-item>
                          <el-dropdown-item v-if="streamType" command="copyStreamId">
                            <me-icon
                              icon="el-icon-document-copy"
                              :name="t('redisValue.copyStreamId')" />
                          </el-dropdown-item>
                          <el-dropdown-item command="copyValue">
                            <me-icon
                              icon="el-icon-document-copy"
                              :name="t('redisValue.copyValue')" />
                          </el-dropdown-item>
                          <el-dropdown-item v-if="zsetType" command="copyScore">
                            <me-icon
                              icon="el-icon-document-copy"
                              :name="t('redisValue.copyScore')" />
                          </el-dropdown-item>
                          <el-dropdown-item command="copyAsCommand">
                            <me-icon
                              icon="me-icon-copy-command"
                              :name="t('redisValue.copyAsCommand')" />
                          </el-dropdown-item>
                          <el-dropdown-item v-if="zsetType" command="showZsetRank">
                            <me-icon icon="me-icon-rank" :name="t('redisValue.showZsetRank')" />
                          </el-dropdown-item>
                        </el-dropdown-menu>
                      </template>
                    </el-dropdown>
                  </div>
                </template>
              </el-table-column>
            </me-table>
            <!-- 字段编辑 -->
            <FieldSet
              ref="fieldSetRef"
              :pretty="isPretty"
              :hash-field-ttl-enabled="scanHashFieldTtl"
              @success="onFieldSetSuccess"
              @refreshed="onFieldSetRefreshed"
              @closed="fieldSetInit"
              class="field-set" />
          </div>
        </div>
      </div>

      <!-- 功能区 -->
      <div class="value-footer me-flex" @click="onFieldPanelOutsideClick">
        <div class="me-flex" style="align-items: center">
          <!-- 美化/复制 -->
          <me-icon
            placement="top-start"
            :info="t('redisValue.prettyHint')"
            class="icon-btn"
            :style="{ opacity: isPretty ? 1 : 0.2 }"
            icon="el-icon-magic-stick"
            @click="isPretty = !isPretty" />

          <me-icon
            style="font-size: 18px; margin-left: 5px"
            :info="t('redisValue.copyValue')"
            class="icon-btn"
            icon="el-icon-document-copy"
            @click="meCopy(showValue)"
            placement="top-start" />

          <me-icon
            placement="top-start"
            :info="t('redisValue.refreshKey')"
            class="icon-btn"
            :class="{ rotating: loading }"
            style="font-size: 18px; margin-left: 5px"
            :icon="loading ? 'el-icon-loading' : 'el-icon-refresh-right'"
            @click="onFooterRefreshKey" />

          <el-divider direction="vertical" v-if="textMemory" />

          <!-- 内存占用 -->
          <el-text> {{ textMemory }} </el-text>

          <el-divider direction="vertical" v-if="textLength" />

          <!-- 字节长度 / 总数（同一位置，按类型切换标签） -->
          <el-text> {{ textLength }} </el-text>

          <el-divider direction="vertical" v-if="textEntries" />

          <!-- 已扫描：筛选 / 已加载 -->
          <el-text> {{ textEntries }} </el-text>
        </div>

        <div class="me-flex" style="position: relative">
          <!-- Auto 识别结果：下拉左侧，下拉本身保持 Auto -->
          <el-text
            v-if="detectedViewText"
            class="bytes-format-auto-label"
            style="margin-right: 8px; white-space: nowrap"
            :title="t('redisValue.autoDetected')">
            {{ detectedViewText }}
          </el-text>
          <el-select
            v-model="bytesFormat"
            :disabled="jsonType || streamType"
            :class="{ 'is-auto-format': bytesFormat === 'auto' }"
            popper-class="bytes-format-select"
            style="width: 100px"
            @change="refreshKey(false)">
            <template #header>
              <div
                class="me-flex"
                style="align-items: center; justify-content: space-evenly; width: 100%">
                <el-text style="font-weight: bold">{{ t('redisValue.viewCodec') }}</el-text>
                <me-icon
                  v-if="canEdit"
                  icon="el-icon-edit"
                  :name="t('customCodec.title')"
                  hint
                  class="icon-btn"
                  style="margin-left: 5px"
                  @click.stop="customCodecVisible = true" />
              </div>
            </template>
            <el-option
              v-for="item in formatOptions.builtin"
              :key="item.value"
              :label="item.label"
              :value="item.value"
              :disabled="item.disabled"
              :class="{ 'bytes-format-auto-option': item.value === 'auto' }" />
            <el-option
              v-for="(item, index) in formatOptions.custom"
              :key="item.value"
              :label="item.label"
              :value="item.value"
              :disabled="item.disabled" />
          </el-select>
          <!-- 加载更多、加载全部 -->
          <div class="me-flex" style="width: 45px; margin-left: 10px" v-if="showMore">
            <me-icon
              :name="t('redisValue.loadMore')"
              icon="me-icon-load-more"
              hint
              placement="top"
              class="icon-btn"
              @click="refreshKey(false, true, false)" />
            <me-icon
              :name="t('redisValue.loadAll')"
              icon="me-icon-load-all"
              hint
              placement="top"
              class="icon-btn"
              @click="refreshKey(false, true, true)" />
          </div>

          <!-- 保存 -->
          <me-button
            style="margin-left: 10px"
            :disabled="viewDecodeFailed || !valueDirty"
            v-if="canSave"
            :info="t('save')"
            type="primary"
            icon="me-icon-save"
            @click="setValue"
            placement="top" />

          <!-- string / json 类型不显示 -->
          <el-segmented
            style="margin-left: 10px"
            v-model="viewType"
            :options="viewTypeList"
            @change="onViewTypeChange"
            v-if="!(stringType || jsonType)">
            <template #default="scope">
              <me-icon
                :name="t('redisValue.jsonView')"
                icon="me-icon-json"
                hint
                placement="top"
                v-if="scope.item === 'json'" />
              <me-icon
                :name="t('redisValue.tableView')"
                icon="me-icon-table"
                hint
                placement="top"
                v-else />
            </template>
          </el-segmented>
        </div>
      </div>
    </template>

    <!-- 未选择键时Empty显示 -->
    <el-empty v-else :description="t('redisValue.noKeySelected')"></el-empty>

    <!-- 更新TTL, 字段新增 -->
    <TTLSet ref="ttlSetRef" @success="setTimer" />
    <FieldAdd ref="fieldAddRef" @success="refreshKey" />
    <KeyRename ref="keyRenameRef" />
    <ObjectInfo ref="objectInfoRef" />
    <CustomCodec v-model="customCodecVisible" />

    <!-- Stream 消费者组 -->
    <TableGroup ref="tableGroupRef" />
    <!-- Hash 全量字段名/值（HKEYS/HVALS） -->
    <TableHashKeys ref="hashKeysRef" />
    <!-- ZSet TopN 范围查询 -->
    <TableZsetRange ref="zsetRangeRef" />
    <!-- 值编辑器快捷键说明 -->
    <ValueShortcut ref="valueShortcutRef" />
    <!-- 命令帮助 -->
    <CommandHelp ref="commandHelpRef" />
  </div>
</template>

<style scoped lang="scss">
.redis-value {
  height: 100%;
  overflow: hidden;

  display: flex;
  flex-direction: column;

  .value-header {
    margin-right: 5px;

    :deep(.el-input-group__prepend) {
      padding: 0 12px;
    }

    display: flex;
    align-items: center;
    gap: 10px;

    .value-header-main {
      flex: 1;
      min-width: 0;
      display: flex;
      align-items: center;
      gap: 10px;
    }

    .value-header-input {
      flex: 1;
      min-width: 0;
    }

    .value-header-hash {
      width: 200px;
      flex-shrink: 0;
    }

    .suffix-ttl {
      cursor: pointer;
      font-size: 13px;
      color: var(--el-text-color-secondary);

      &:hover {
        color: var(--el-color-primary);
      }
    }

    .ttl-suffix-separator {
      margin-right: 6px;
      color: var(--el-border-color);
      user-select: none;
    }

    .value-header-actions {
      display: flex;
      align-items: center;
      gap: 5px;
      flex-shrink: 0;

      :deep(.icon-btn) {
        font-size: 18px;
      }

      .is-favorited {
        color: #f7ba2a;
      }
    }
  }

  .value-main {
    margin: 10px 0 5px 0;
    position: relative;
    flex-grow: 1;
    overflow: hidden;

    .value-truncated-alert {
      margin-bottom: 8px;

      .value-truncated-desc {
        margin: 0 0 8px;
        line-height: 1.5;
      }

      .value-truncated-actions {
        display: flex;
        gap: 8px;
      }
    }

    .table-toolbar {
      width: 100%;
      align-items: center;

      .stream-range-inputs {
        display: flex;
        gap: 5px;
        margin-left: 10px;
        flex-shrink: 0;
        align-items: center;

        :deep(.el-input) {
          width: 120px;
        }
      }

      .list-range-inputs {
        display: flex;
        gap: 5px;
        margin-left: 10px;
        flex-shrink: 0;
        align-items: center;

        :deep(.el-input) {
          width: 120px;
        }
      }

      .list-range-sep {
        color: var(--el-text-color-secondary);
        flex-shrink: 0;
      }

      .stream-range-sep {
        color: var(--el-text-color-secondary);
        flex-shrink: 0;
      }

      .table-toolbar-actions {
        margin-left: auto;
        display: flex;
        align-items: center;
        flex-shrink: 0;
      }

      .field-scan-input {
        width: 250px;
        flex-shrink: 0;

        .keyword-suffix {
          display: flex;
          align-items: center;
          gap: 6px;
          margin-left: 6px;

          :deep(.suffix-exact-checkbox) {
            height: auto;

            .el-checkbox__inner {
              border-color: var(--el-text-color-secondary);
              background-color: transparent;
            }

            &:hover .el-checkbox__inner {
              border-color: var(--el-color-primary);
            }

            &.is-checked .el-checkbox__inner {
              background-color: var(--el-color-primary);
              border-color: var(--el-color-primary);
            }
          }
        }

        .scan-control {
          position: relative;
          width: 24px;
          height: 24px;
          display: flex;
          align-items: center;
          justify-content: center;
          cursor: pointer;
          flex-shrink: 0;

          .scan-ring {
            position: absolute;
            top: 50%;
            left: 50%;
            transform: translate(-50%, -50%);
            line-height: 1;
          }

          .scan-icon {
            position: relative;
            z-index: 1;
            font-size: 16px;

            :deep(.icon),
            :deep(svg) {
              width: 16px;
              height: 16px;
            }
          }
        }
      }
    }

    .table-view {
      margin-top: 10px;
      flex-grow: 1;
      height: 0;
      width: 100%;
      position: relative;

      :deep(.el-table) {
        .field-set-row {
          --el-table-tr-bg-color: var(--el-color-warning-light-9);
        }

        // 序号列：编辑态图标与行号均居中
        .index-cell {
          display: flex;
          align-items: center;
          justify-content: center;
        }

        .field-row-actions {
          :deep(.icon-btn) {
            font-size: 16px;
          }
        }
      }

      .field-set {
        position: absolute;
        top: 0;
        right: 0;
        z-index: 20;
        width: 60%;
        height: 100%;
      }
    }
  }

  .value-footer {
    height: 30px;
    font-size: 20px;

    @keyframes rotate {
      from {
        transform: rotate(0deg);
      }
      to {
        transform: rotate(360deg);
      }
    }

    .rotating {
      animation: rotate 1s linear infinite;
    }

    .bytes-format-auto-label {
      color: var(--el-color-primary);
      font-weight: 600;
    }

    .is-auto-format {
      :deep(.el-select__selected-item) {
        color: var(--el-color-primary);
        font-weight: 600;
      }
    }

    :deep(.el-select__wrapper) {
      min-height: 0;
      height: 30px;
      padding: 4px 4px 4px 10px;
      //box-shadow: 0 0 0 1px var(--el-border-color);
    }

    :deep(.el-select-dropdown__item) {
      padding: 0 20px 0 20px;
    }
  }
}
</style>

<!-- 下拉挂到 body，需非 scoped -->
<style lang="scss">
.bytes-format-select {
  .bytes-format-auto-option {
    color: var(--el-color-primary);
    font-weight: 600;
  }
}
</style>
