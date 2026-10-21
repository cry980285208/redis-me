// 多TAB（多键值窗口）：同一连接/库下并行打开多个 key 的详情。
//
// 设计约束（为了以后好合并上游）：
// 1. 状态全部放在全局 share 里；本文件只提供纯函数（操作 share）+ 一个状态机 composable；
// 2. 状态机挂在 AppMain（常驻组件）上：RedisValue 会因切连接被销毁重建，
//    状态机若挂在它身上，切连接时「自动激活首个 TAB / 接收 KEY_OPEN_TAB」就会丢；
// 3. 上游 RedisValue/index.vue 只保留几行接入代码，冲突面最小；
// 4. 只在当前连接/库的 TAB 之间切换，不会串台。
import { computed, onMounted, onUnmounted, watch } from 'vue'

import i18n from '@/locales'
import type { AppMainShare } from '@/types/me-interface'
import type { RedisKey_Deserialize } from '@/types/tauri-specta'
import { bus, KEY_OPEN_TAB, KEY_REFRESH, meWarn } from '@/utils/util'

const t = i18n.global.t

/** 多TAB：同一连接/库下同时打开的键值 TAB 上限 */
export const MAX_KEY_TABS = 10

/** 多TAB：一个键值 TAB */
export interface KeyTab {
  /** TAB 唯一 id */
  id: string
  /** 所属连接 id（多连接时用来隔离） */
  connId: string
  /** 所属库号 */
  db: number
  /** 键快照（类型/TTL 可能随刷新变化，聚焦时会更新） */
  redisKey: RedisKey_Deserialize
}

/** 当前连接/库可见的键值 TAB（纯函数） */
export function visibleKeyTabs(share: AppMainShare): KeyTab[] {
  if (!share.conn?.id) return []
  return share.openKeys.filter(k => k.connId === share.conn!.id && k.db === share.conn!.db)
}

/** 激活某个 TAB 并通知值区刷新 */
function activate(share: AppMainShare, tab: KeyTab): void {
  share.activeKeyId = tab.id
  share.redisKey = tab.redisKey
  share.tabName = 'value'
  bus.emit(KEY_REFRESH)
}

/**
 * 打开/聚焦键值 TAB：同名键去重（只更新快照）；超上限提示。
 * 非当前连接的请求直接忽略，避免多连接时串台。
 */
export function openKeyTab(
  share: AppMainShare,
  connId: string,
  redisKey: RedisKey_Deserialize,
): void {
  if (share.conn?.id !== connId) return
  const existing = share.openKeys.find(
    k => k.connId === connId && k.db === share.conn!.db && k.redisKey.key === redisKey.key,
  )
  if (existing) {
    existing.redisKey = redisKey // 更新为最新引用（类型/TTL 可能变）
    activate(share, existing)
    return
  }
  // 上限按「当前连接 + 当前库」算，不是全局算
  const count = share.openKeys.filter(
    k => k.connId === connId && k.db === share.conn!.db,
  ).length
  if (count >= MAX_KEY_TABS) {
    meWarn(t('redisValue.keyTabsMax'))
    return
  }
  const tab: KeyTab = {
    id: `kt_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`,
    connId: connId,
    db: share.conn!.db,
    redisKey,
  }
  share.openKeys.push(tab)
  activate(share, tab)
}

/** 切换激活的键值 TAB 并加载该键 */
export function switchKeyTab(share: AppMainShare, id: string): void {
  const tab = share.openKeys.find(k => k.id === id)
  if (!tab) return
  activate(share, tab)
}

/** 关闭键值 TAB；若关的是激活项，切到相邻（优先右侧，否则左侧），都没有则清空 */
export function closeKeyTab(share: AppMainShare, id: string): void {
  const idx = share.openKeys.findIndex(k => k.id === id)
  if (idx === -1) return
  // 先在同连接的 TAB 里定位邻居（优先右侧），再移除
  const siblings = share.openKeys.filter(
    k => k.connId === share.conn?.id && k.db === share.conn?.db,
  )
  const pos = siblings.findIndex(k => k.id === id)
  const neighbor = siblings[pos + 1] ?? siblings[pos - 1] ?? null
  share.openKeys.splice(idx, 1)
  if (share.activeKeyId !== id) return
  if (neighbor) {
    activate(share, neighbor)
  } else {
    share.activeKeyId = null
    share.redisKey = null
  }
}

/** 关闭当前连接/库的全部键值 TAB */
export function closeAllKeyTabs(share: AppMainShare): void {
  if (!share.conn) return
  const keep = share.openKeys.filter(
    k => !(k.connId === share.conn!.id && k.db === share.conn!.db),
  )
  // 用 splice 保持数组引用不变，避免别处持有旧引用
  share.openKeys.splice(0, share.openKeys.length, ...keep)
  share.activeKeyId = null
  share.redisKey = null
}

/**
 * 多TAB 键值状态机（挂在 AppMain 上，常驻）。
 * 负责：切连接/切库时自动激活、接收键区与内存页的「打开键值 TAB」请求。
 */
export function useKeyTabs(share: AppMainShare) {
  /** 当前连接/库可见的键值 TAB */
  const visibleKeys = computed(() => visibleKeyTabs(share))

  // 切连接 / 切库：自动激活该连接/库的第一个 TAB，没有则清空（所有 TAB 数据都保留）
  watch(
    [() => share.conn?.id, () => share.conn?.db],
    () => {
      if (!share.conn?.id) {
        share.activeKeyId = null
        share.redisKey = null
        return
      }
      const currentTabs = visibleKeys.value
      if (currentTabs.length === 0) {
        share.activeKeyId = null
        share.redisKey = null
        return
      }
      // 当前激活的 TAB 属于新连接就保留，否则切到第一个
      const stillValid = share.activeKeyId && currentTabs.find(k => k.id === share.activeKeyId)
      if (!stillValid) {
        activate(share, currentTabs[0])
      }
    },
  )

  // 键区 / 内存页点键 → 打开或聚焦对应的键值 TAB
  const onKeyOpenTabBus = (payload: { connId: string; redisKey: RedisKey_Deserialize }) =>
    openKeyTab(share, payload.connId, payload.redisKey)
  onMounted(() => bus.on(KEY_OPEN_TAB, onKeyOpenTabBus))
  onUnmounted(() => bus.off(KEY_OPEN_TAB, onKeyOpenTabBus))

  return { visibleKeys }
}
