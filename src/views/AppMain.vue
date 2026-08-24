<script setup lang="ts">
// #region 导入
import { getCurrentWindow } from '@tauri-apps/api/window'
import { check, type Update } from '@tauri-apps/plugin-updater'
import {
  computed,
  nextTick,
  onMounted,
  onUnmounted,
  provide,
  reactive,
  ref,
  shallowReactive,
  useTemplateRef,
  watch,
} from 'vue'
import { useI18n } from 'vue-i18n'

import {
  appProvideKey,
  connUiProvideKey,
  shareProvideKey,
  type AppMainInject,
  type AppMainShare,
  type ConnListWindowsSyncPayload,
  type ConnShortcutAction,
  type ServerCapabilities,
  type UiConn,
} from '@/types/me-interface'
import type { ConnConfig, RedisKey_Deserialize } from '@/types/tauri-specta'
import { openNewWindow, toggleAppFullscreen } from '@/utils/app-window'
import {
  collapseImportedConnGroups,
  collectKnownConnGroupNames,
  isConnMinimalMode,
  mergeConnGroupsFromList,
} from '@/utils/conn'
import { clearKeyTypeCacheForConn } from '@/utils/key-type-cache'
import { mergeImportedConnList } from '@/utils/rdm'
import {
  isAppFullscreenHotkeyBlocked,
  matchAppFullscreenHotkey,
  matchConnShortcutAction,
} from '@/utils/shortcut'
import {
  bus,
  CONN_LIST_WINDOWS_SYNC,
  CONN_REFRESH,
  INFO_REFRESH,
  meCommands,
  meJsonParse,
  meOk,
  meWarn,
} from '@/utils/util'
import ConnImport from '@/views/conn/ConnImport.vue'
import ConnSave from '@/views/conn/ConnSave.vue'
import KeyEmpty from '@/views/key/KeyEmpty.vue'
import KeyHeader from '@/views/KeyHeader.vue'
import KeyMain from '@/views/KeyMain.vue'
import TabConn from '@/views/TabConn.vue'

import ConnTabStrip from './ConnTabStrip.vue'
import TabMain from './TabMain.vue'
// #endregion

// #region 核心状态

const { t } = useI18n()

// 共享数据
const share = reactive<AppMainShare>({
  conn: null,
  connList: meTauri.connList as UiConn[],
  nodeList: [],
  loading: false,
  color: 'var(--el-color-primary)',
  readonly: false,
  redisKey: null,
  tabName: 'info',
  dbSizeMap: {},

  exportImporting: false,
  exportImportingTip: '',
  exportImportingPercentage: 0,

  capabilities: {
    version: '',
    isValkey: false,
    infoSupported: false,
    aclSupported: false,
    aclDryrunSupported: false,
    aclSelectorSupported: false,
    httlSupported: false,
    clusterDbSupported: false,
  },

  // 多TAB：已打开连接列表 + 激活连接 id
  openConns: [],
  activeConnId: null,
  // 多TAB：已打开键值列表 + 激活键值 id（全局状态，跨组件生命周期持久化）
  openKeys: [],
  activeKeyId: null,
})
provide(shareProvideKey, share)
// #endregion

// #region 连接切换（多TAB：连接常驻，切换不重连）

// 多TAB连接状态：已建立后端连接的 conn.id 集合 + capabilities 缓存
// 原则：首次打开才 meCommands.connect（Rust connect 非幂等，会重建 client）；
// 切换已打开的连接只复位 UI 状态（不重连，保留 db 选择）；关闭才 disconnect。
const connectedIds = new Set<string>()
const capsCache = new Map<string, ServerCapabilities>()
const MAX_CONN_TABS = 10

// 当环境发生变化时，销毁整个key和tag组件（避免状态保留）
onMounted(() => {
  bus.on(CONN_REFRESH, toggleKeyTag)
  window.addEventListener('keydown', onGlobalConnHotkey, true)
})
onUnmounted(() => {
  bus.off(CONN_REFRESH, toggleKeyTag)
  window.removeEventListener('keydown', onGlobalConnHotkey, true)
  // 窗口关闭兜底：断开所有仍处于连接态的连接
  for (const id of connectedIds) void meCommands.disconnect(id)
  connectedIds.clear()
  capsCache.clear()
})

// 切换连接时销毁key/tag组件
const connPrepared = ref(false)
function toggleKeyTag(): void {
  connPrepared.value = false
  void nextTick(() => {
    connPrepared.value = true
  })
}

// 多TAB：打开/聚焦连接 TAB
async function openConnTab(conn: UiConn): Promise<void> {
  const existing = share.openConns.find(c => c.id === conn.id)
  if (existing) {
    // 已打开：仅激活，不重连
    share.activeConnId = conn.id
    share.conn = conn
    return
  }
  if (share.openConns.length >= MAX_CONN_TABS) {
    meWarn(t('conn.connTabsMax'))
    return
  }
  share.loading = true
  try {
    if (!connectedIds.has(conn.id)) {
      const caps = await meCommands.connect(conn.id)
      connectedIds.add(conn.id)
      capsCache.set(conn.id, caps)
    }
    share.openConns.push(conn)
    share.activeConnId = conn.id
    share.conn = conn // 触发下方 watch 复位 caps/UI
  } catch {
    // 连接失败：不加入 TAB
  } finally {
    share.loading = false
  }
}

// 多TAB：关闭连接 TAB
async function closeConnTab(connId: string): Promise<void> {
  const idx = share.openConns.findIndex(c => c.id === connId)
  if (idx === -1) return
  share.openConns.splice(idx, 1)
  connectedIds.delete(connId)
  capsCache.delete(connId)
  clearKeyTypeCacheForConn(connId)
  void meCommands.disconnect(connId)
  if (share.activeConnId === connId) {
    // 切到相邻（优先右侧，否则左侧，否则无）
    const neighbor = share.openConns[idx] ?? share.openConns[idx - 1] ?? null
    share.activeConnId = neighbor?.id ?? null
    share.conn = neighbor // 触发 watch 复位；为 null 时进入无连接态
  }
}

// 切换连接时loading（多TAB：不重连，只复位 UI 状态 + 恢复缓存 caps）
watch(
  () => JSON.stringify(share.conn),
  async (newConnStr, oldConnStr) => {
    const newConn = meJsonParse(newConnStr) as UiConn | null
    const oldConn = meJsonParse(oldConnStr) as UiConn | null

    const index = share.connList.findIndex((c: UiConn) => c.id === newConn?.id)
    if (index !== -1 && newConn) {
      share.connList[index] = newConn
    }

    if (newConn?.id === oldConn?.id) return

    // 多TAB：如果是切换到已打开的连接（即已连接且缓存有效），则不销毁 TabMain，
    // 以保留左侧键列表展开状态和右侧已打开的键值 TAB
    const isAlreadyOpen = newConn?.id ? connectedIds.has(newConn.id) : false
    if (!isAlreadyOpen) {
      connPrepared.value = false
    }

    try {
      if (newConn) {
        share.color = newConn.color ?? 'var(--el-color-primary)'
        share.readonly = !!newConn.readonly
        // 多TAB：切换已打开连接不重连，从缓存恢复 capabilities
        const cached = capsCache.get(newConn.id)
        if (cached) Object.assign(share.capabilities, cached)
        share.tabName =
          isConnMinimalMode(newConn) || !share.capabilities.infoSupported ? 'value' : 'info'
        connPrepared.value = true
        // 触发 KeyMain / RedisInfo 等组件刷新（多TAB切换不重连，需显式通知）
        bus.emit(CONN_REFRESH)
        bus.emit(INFO_REFRESH)
      }
    } catch {
      if (!oldConn) share.conn = null
    } finally {
      share.loading = false
    }
  },
  { deep: true },
)
// #endregion

// #region 连接列表同步
const tauriWindow = getCurrentWindow()
const connListToString = computed(() => JSON.stringify(share.connList))
watch(
  connListToString,
  async newConnList => {
    const connList = meJsonParse(newConnList) as UiConn[]
    meTauri.connList = connList as MeTauriGlobal['connList']

    await meCommands.connList(connList as ConnConfig[])
    const payload: ConnListWindowsSyncPayload = { connList, label: tauriWindow.label }
    await tauriWindow.emit(CONN_LIST_WINDOWS_SYNC, payload)
  },
  { immediate: true },
)

onMounted(
  () =>
    void tauriWindow.listen(CONN_LIST_WINDOWS_SYNC, e => {
      share.connList = (e.payload as ConnListWindowsSyncPayload).connList
    }),
)
// #endregion

// #region 自动更新

// shallowReactive：`Update` 含私有字段，deep reactive 会解成普通对象导致与 `AppMainInject` 不兼容
const app = shallowReactive<AppMainInject>({
  update: null,
  downloading: false,
  downloadPercentage: 0,
})
provide(appProvideKey, app)
async function checkAutoUpdate(): Promise<void> {
  if (meTauri.isAppStore) return
  if (!meTauri.settings.autoUpdate) return
  app.update = (await check().catch((): null => null)) as Update | null
}
onMounted(checkAutoUpdate)
// #endregion

// #region 面板操作
function changeReadonly(): void {
  share.readonly = !share.readonly
  meOk(share.readonly ? t('appMain.readonlyTip') : t('appMain.writableTip'))
}

// 连接相关弹窗：ConnSave/Import 挂 AppMain；设置弹窗在 KeyHeader（始终挂载）
const connSaveRef = useTemplateRef<InstanceType<typeof ConnSave>>('connSave')
const connImportRef = useTemplateRef<InstanceType<typeof ConnImport>>('connImport')
const dialog = reactive({ conn: false, import: false })

const connUi = reactive({
  openConnSave(mode: 'add' | 'edit', conn?: UiConn): void {
    if (mode === 'add' && !conn && dialog.conn) {
      dialog.conn = false
      return
    }
    dialog.conn = true
    void nextTick(() => connSaveRef.value?.open(mode, conn))
  },
  openConnImport(): void {
    if (dialog.import) {
      dialog.import = false
      return
    }
    dialog.import = true
    void nextTick(() => connImportRef.value?.open())
  },
  /** KeyHeader onMounted 时注入，供菜单与全局快捷键共用 */
  openSetting(): void {},
  openShortcuts(): void {},
  /** KeyMain onMounted 时注入，供键值页等打开创建副本弹窗 */
  openKeyCopy(_redisKey: RedisKey_Deserialize): void {},
  /** KeyMain onMounted 时注入，供键值页定位当前键 */
  scrollKeyToTree(_redisKey: RedisKey_Deserialize): void {},
  runConnAction(action: ConnShortcutAction): void {
    if (action === 'add') connUi.openConnSave('add')
    else if (action === 'import') connUi.openConnImport()
    else if (action === 'setting') connUi.openSetting()
    else if (action === 'shortcuts') connUi.openShortcuts()
    else if (action === 'newWindow') void openNewWindow()
  },
  // 多TAB：打开/关闭连接 TAB（绑定到本组件作用域内的实现）
  openConnTab(conn: UiConn): void {
    void openConnTab(conn)
  },
  closeConnTab(connId: string): void {
    void closeConnTab(connId)
  },
})

function onGlobalConnHotkey(e: KeyboardEvent): void {
  const action = matchConnShortcutAction(e)
  if (action) {
    e.preventDefault()
    connUi.runConnAction(action)
    return
  }

  if (!matchAppFullscreenHotkey(e)) return
  if (isAppFullscreenHotkeyBlocked(e, { tabName: share.tabName })) return

  e.preventDefault()
  void toggleAppFullscreen()
}

function onConnImported(impConnList: UiConn[]): void {
  if (!Array.isArray(meTauri.settings.connGroups)) meTauri.settings.connGroups = []
  const knownGroups = collectKnownConnGroupNames(meTauri.settings.connGroups, share.connList)
  share.connList = mergeImportedConnList(share.connList, impConnList)
  mergeConnGroupsFromList(share.connList, meTauri.settings.connGroups)
  collapseImportedConnGroups(
    impConnList,
    knownGroups,
    meTauri.settings.connGroupExpanded as Record<string, boolean>,
  )
  meOk(t('conn.importOk'))
}

provide(connUiProvideKey, connUi)
// #endregion
</script>

<template>
  <div class="redis-main" v-loading="share.loading">
    <el-splitter>
      <!-- 左侧键 -->
      <el-splitter-panel :min="250" size="30%">
        <div class="redis-key">
          <KeyHeader />
          <KeyMain v-if="share.conn && connPrepared" />
          <KeyEmpty v-else />
        </div>
      </el-splitter-panel>

      <!-- 右侧值 -->
      <el-splitter-panel :min="250">
        <div class="redis-tab-wrap">
          <!-- 多TAB：连接条置于右侧最上方 -->
          <ConnTabStrip v-if="share.openConns.length" class="conn-tab-strip" />
          <div class="tab-body">
            <TabConn v-if="!share.conn || !connPrepared" />
            <template v-else>
              <TabMain />

              <!-- 只读/可写 -->
              <me-icon
                class="readonly-icon"
                plain
                :icon="share.readonly ? 'me-icon-lock' : 'me-icon-unlock'"
                :name="share.readonly ? t('appMain.readonly') : t('appMain.writable')"
                :hint="true"
                @click="changeReadonly" />

              <!-- 导入导出 -->
              <el-progress
                class="export-importing"
                type="dashboard"
                status="success"
                :percentage="share.exportImportingPercentage"
                v-if="share.exportImporting">
                <template #default="{ percentage }">
                  <div class="percentage-value">{{ percentage }}%</div>
                  <div class="percentage-label">{{ share.exportImportingTip }}</div>
                </template>
              </el-progress>
            </template>
          </div>
        </div>
      </el-splitter-panel>
    </el-splitter>

    <!-- 支持全局快捷键 -->
    <ConnSave ref="connSave" v-if="dialog.conn" @closed="dialog.conn = false" />
    <ConnImport
      ref="connImport"
      v-if="dialog.import"
      @import="onConnImported"
      @closed="dialog.import = false" />
  </div>
</template>

<style scoped lang="scss">
.redis-main {
  height: calc(100% - 30px);
  padding: 0px 5px 5px 5px;
  flex: 1;

  .redis-key {
    height: 100%;
    display: flex;
    flex-direction: column;
  }

  /* 多TAB：右侧连接条 + 值区主体布局 */
  .redis-tab-wrap {
    height: 100%;
    display: flex;
    flex-direction: column;
    position: relative; // 供内部绝对定位的只读图标/进度条作参照
  }

  .conn-tab-strip {
    flex-shrink: 0;
  }

  .tab-body {
    flex: 1;
    min-height: 0;
    position: relative; // 供 readonly-icon / export-importing 绝对定位
  }

  /* 中间分隔面板的样式调整 */
  :deep(.el-splitter-bar) {
    width: 5px !important;

    .el-splitter-bar__dragger-horizontal:before {
      width: 0; // 宽度为0，不显示原始的竖线
      background-color: transparent;
    }
  }

  /* 只读按钮图标 */
  :deep(.readonly-icon) {
    position: absolute;
    top: 10px;
    right: 10px;
    font-size: 20px;
    z-index: 100;
    color: var(--el-color-success);

    cursor: pointer;
    &:hover {
      color: var(--el-color-primary);
    }
  }

  // 版本升级过程中显示下载进度
  .export-importing {
    position: absolute;
    right: 20px;
    bottom: 0;
    z-index: 100;

    .percentage-value {
      font-size: 28px;
      color: var(--el-color-success);
    }

    .percentage-label {
      margin-top: 10px;
      font-size: 14px;
    }
  }
}
</style>
