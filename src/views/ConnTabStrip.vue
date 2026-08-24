<script setup lang="ts">
// 多TAB：连接 TAB 条。置于右侧值区最上方。
// - 切换：点击 TAB → 激活该连接（不重连，由 AppMain watch 复位 UI）
// - 关闭：点 × → connUi.closeConnTab（disconnect + 移除 + 切相邻）
// - 新增：末尾「+」下拉列出尚未打开的连接 → connUi.openConnTab
import { computed, inject } from 'vue'
import { useI18n } from 'vue-i18n'

import { connUiProvideKey, shareProvideKey, type UiConn } from '@/types/me-interface'
import { getConnIcon } from '@/utils/conn'

const share = inject(shareProvideKey)!
const connUi = inject(connUiProvideKey)!
const { t } = useI18n()

// el-tabs 的 v-model：与 share.activeConnId 双向同步
const activeName = computed<{
  get: () => string
  set: (v: string) => void
}>({
  get: () => share.activeConnId ?? '',
  set: (name: string) => {
    const conn = share.openConns.find(c => c.id === name) as UiConn | undefined
    if (conn) {
      share.activeConnId = conn.id
      share.conn = conn
    }
  },
})

// 「+」下拉：尚未打开的连接
const candidates = computed(() => share.connList.filter(c => !share.openConns.some(o => o.id === c.id)))
const canAdd = computed(() => share.openConns.length < 10)

function onTabRemove(name: string | number): void {
  connUi.closeConnTab(String(name))
}

function onAdd(conn: UiConn): void {
  connUi.openConnTab(conn)
}

// 关闭所有已打开的连接 TAB
function closeAllConnections(): void {
  // 倒序或拷贝，避免 splice 导致索引错位
  for (const conn of [...share.openConns].reverse()) {
    connUi.closeConnTab(conn.id)
  }
}
</script>

<template>
  <div class="conn-tab-strip">
    <el-tabs
      v-model="activeName"
      type="card"
      @tab-remove="onTabRemove">
      <el-tab-pane
        v-for="conn in share.openConns"
        :key="conn.id"
        :name="conn.id"
        closable>
        <template #label>
          <span class="tab-label" :title="conn.name">{{ conn.name }}</span>
        </template>
      </el-tab-pane>
    </el-tabs>

    <el-dropdown
      v-if="canAdd && candidates.length"
      placement="bottom-start"
      trigger="click"
      @command="onAdd">
      <span class="add-btn" :title="t('connTabs.add')">
        <me-icon icon="el-icon-plus" />
      </span>
      <template #dropdown>
        <el-dropdown-menu>
          <el-dropdown-item
            v-for="conn in candidates"
            :key="conn.id"
            :command="conn">
            <div :style="{ color: conn?.color }">
              <me-icon :icon="getConnIcon(conn)" :name="conn.name" />
            </div>
          </el-dropdown-item>
        </el-dropdown-menu>
      </template>
    </el-dropdown>
    
    <span
      v-if="share.openConns.length"
      class="close-all-btn"
      :title="t('connTabs.closeAll')"
      @click="closeAllConnections">
      <me-icon icon="el-icon-circle-close" />
    </span>
  </div>
</template>

<style scoped lang="scss">
.conn-tab-strip {
  display: flex;
  align-items: center;
  padding: 0 10px; // 与 TabMain 的 padding 对齐
  // 添加与 TabMain 一致的边框（除了底部），确保内容区宽度完全对齐
  border: 1px solid var(--el-border-color);
  border-bottom: none;

  // 让 tabs 占据剩余空间，将按钮组推到最右侧
  :deep(.el-tabs) {
    flex: 1;
    min-width: 0;

    .el-tabs__header {
      margin: 0 0 -1px 0;
    }

    // 滚动容器及溢出箭头高度对齐
    .el-tabs__nav-wrap {
      height: 28px;
    }

    // 溢出时左右切换按钮：实心背景 + 高度/宽度对齐，与 TAB 行齐平
    .el-tabs__nav-prev,
    .el-tabs__nav-next {
      position: absolute;
      top: 0;
      height: 28px;
      width: 22px;
      line-height: 28px;
      display: flex;
      align-items: center;
      justify-content: center;
      background-color: var(--el-fill-color-light);
      border: 1px solid var(--el-border-color);
      border-radius: 4px;
      z-index: 2;
      cursor: pointer;
      color: var(--el-text-color-regular);
      transition: background-color 0.2s;

      &:hover {
        background-color: var(--el-fill-color);
        color: var(--el-color-primary);
      }

      .el-icon {
        font-size: 14px;
      }
    }
    .el-tabs__nav-prev {
      left: 0;
      border-right: none;
    }
    .el-tabs__nav-next {
      right: 0;
      border-left: none;
    }

    .el-tabs__item {
      width: auto;
      max-width: 280px;
      height: 28px;
      line-height: 28px;
      display: flex;
      align-items: center;
      overflow: hidden;
      // 去除默认 padding
      padding: 0 10px !important;
      box-sizing: border-box;
      // 确保没有 margin
      margin: 0;
      border: 1px solid var(--el-border-color) !important;
      border-bottom: none !important;
      // 激活状态下的样式
      &.is-active {
        border-bottom: 2px solid var(--el-color-primary) !important;
      }
      
      // 标签文本区域：超长省略
      .tab-label {
        flex: 1;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        margin-right: 6px;
        font-size: 13px;
      }
      
      // 关闭按钮常显且美观
      .el-icon-close,
      .is-icon-close {
        flex-shrink: 0;
        opacity: 1 !important;
        display: inline-flex !important;
        width: 14px;
        height: 14px;
        font-size: 12px;
        border-radius: 50%;
        justify-content: center;
        align-items: center;
        margin-left: 6px;
        color: var(--el-text-color-secondary);
        transition: background-color 0.2s, color 0.2s;
        
        &:hover {
          background-color: var(--el-color-danger-light-9);
          color: var(--el-color-danger);
        }
      }
    }
  }

  .add-btn {
    flex-shrink: 0;
    margin-left: auto; // 关键：推到最右侧
    margin-right: 6px;
    cursor: pointer;
    font-size: 18px;
    color: var(--el-color-primary);
    display: inline-flex;
    align-items: center;

    &:hover {
      color: var(--el-color-danger);
    }
  }
  
  .close-all-btn {
    flex-shrink: 0;
    margin-left: 6px;
    cursor: pointer;
    font-size: 18px;
    color: var(--el-text-color-secondary);
    display: inline-flex;
    align-items: center;

    &:hover {
      color: var(--el-color-danger);
    }
  }
}
</style>
