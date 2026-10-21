// 多TAB（多连接窗口）：同一窗口内并行打开多个 Redis 连接。
//
// 设计约束（为了以后好合并上游）：
// 1. 全部状态与逻辑收敛在本文件，上游 AppMain.vue 只留「接入点」，冲突面最小；
// 2. 连接常驻：首次打开才 connect（Rust connect 非幂等，会重建 client），
//    切换已打开的连接只复位 UI（不重连，保留各连接自己的 db 选择），关闭才 disconnect；
// 3. 不依赖任何 Rust 侧改动——上游的 clients 本来就是按 conn.id 缓存的，天然支持多连接。
import i18n from '@/locales'
import type { AppMainShare, ServerCapabilities, UiConn } from '@/types/me-interface'
import { clearKeyMemoryCacheForConn } from '@/utils/key-memory-cache'
import { clearKeyTypeCacheForConn } from '@/utils/key-type-cache'
import { bus, CONN_REFRESH, meCommands, meWarn } from '@/utils/util'

/** 多TAB：同时打开的连接 TAB 上限 */
export const MAX_CONN_TABS = 10

/**
 * 多TAB 连接状态机。
 * @param share 全局共享状态（App.vue provide 的那一份，多TAB 字段都在里面）
 */
export function useConnTabs(share: AppMainShare) {
  const t = i18n.global.t

  /** 已建立后端连接的 conn.id 集合 */
  const connectedIds = new Set<string>()
  /** 每个连接的 capabilities 缓存：切换 TAB 时免重连直接恢复 */
  const capsCache = new Map<string, ServerCapabilities>()

  /** 该连接是否还处于连接态（用于判断切换时能否免重连） */
  function isConnected(connId?: string | null): boolean {
    return !!connId && connectedIds.has(connId)
  }

  /** 取该连接缓存的 capabilities */
  function getCaps(connId: string): ServerCapabilities | undefined {
    return capsCache.get(connId)
  }

  /**
   * 确保该连接处于连接态：已连接则直接返回缓存的 capabilities（不重连），
   * 未连接才 connect 一次并写入缓存。
   */
  async function ensureConnected(conn: UiConn): Promise<ServerCapabilities | undefined> {
    const cached = capsCache.get(conn.id)
    if (cached) return cached
    const caps = await meCommands.connect(conn.id)
    connectedIds.add(conn.id)
    capsCache.set(conn.id, caps)
    return caps
  }

  /**
   * 打开/聚焦连接 TAB。
   * - 已打开：只激活，不重连
   * - 超过上限：提示后返回
   * - 首次打开：先 connect，成功后才加入 TAB（连接失败不占位）
   */
  async function openConnTab(conn: UiConn): Promise<void> {
    const existing = share.openConns.find(c => c.id === conn.id)
    if (existing) {
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
      await ensureConnected(conn)
      share.openConns.push(conn)
      share.activeConnId = conn.id
      share.conn = conn // 触发 AppMain 里对 share.conn 的 watch，复位 caps/UI
    } catch {
      // 连接失败：不加入 TAB
    } finally {
      share.loading = false
    }
  }

  /** 关闭连接 TAB：disconnect + 移除 + 切到相邻（优先右侧，否则左侧） */
  async function closeConnTab(connId: string): Promise<void> {
    const idx = share.openConns.findIndex(c => c.id === connId)
    if (idx === -1) return
    share.openConns.splice(idx, 1)
    connectedIds.delete(connId)
    capsCache.delete(connId)
    // 该连接的键类型/内存占用缓存一并清掉，避免重连后读到旧值
    clearKeyTypeCacheForConn(connId)
    clearKeyMemoryCacheForConn(connId)
    void meCommands.disconnect(connId)
    if (share.activeConnId === connId) {
      const neighbor = share.openConns[idx] ?? share.openConns[idx - 1] ?? null
      share.activeConnId = neighbor?.id ?? null
      share.conn = neighbor // 为 null 时进入无连接态
    }
  }

  /** 窗口关闭兜底：断开所有仍处于连接态的连接 */
  function disposeAll(): void {
    for (const id of connectedIds) void meCommands.disconnect(id)
    connectedIds.clear()
    capsCache.clear()
  }

  /**
   * 重新连接并刷新能力（顶栏「刷新连接」）。
   * 走这里而不是直接 connect，是为了让能力缓存同步更新，切回该 TAB 时不会用旧能力。
   */
  async function refreshConn(conn: UiConn): Promise<void> {
    const caps = await meCommands.connect(conn.id)
    connectedIds.add(conn.id)
    capsCache.set(conn.id, caps)
    Object.assign(share.capabilities, caps)
    bus.emit(CONN_REFRESH)
  }

  return {
    openConnTab,
    closeConnTab,
    isConnected,
    getCaps,
    ensureConnected,
    disposeAll,
    refreshConn,
  }
}
