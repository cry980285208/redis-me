<script setup lang="ts">
// 多TAB：键值 TAB 条。纯展示组件，状态与逻辑在 src/utils/key-tabs.ts。
// - 点击：切换激活的键（父组件负责切换并刷新值）
// - 关闭：点 × → 关闭该键值 TAB
// - 右侧按钮：关闭当前连接/库的全部键值 TAB
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import TabScrollbar from '@/components/TabScrollbar.vue'
import type { KeyTab } from '@/utils/key-tabs'
import { useTabScrollbar } from '@/utils/tab-scrollbar'

const props = defineProps<{
  /** 当前连接/库可见的键值 TAB */
  tabs: KeyTab[]
  /** 当前激活的 TAB id */
  activeId: string | null
}>()

const emit = defineEmits<{
  /** 切换激活的 TAB */
  switch: [id: string]
  /** 关闭某个 TAB */
  close: [id: string]
  /** 关闭全部 TAB */
  closeAll: []
}>()

const { t } = useI18n()

// 多TAB：TAB 过多时出现滚动条（与 EP 左右切换箭头同步出现）
const tabsRef = ref()
const { scrollable, thumbWidth, thumbLeft, onThumbPointerDown } = useTabScrollbar(tabsRef)

// el-tabs v-model：get 返回当前激活 id；set 由用户点击触发切换
const activeName = computed({
  get: () => props.activeId ?? '',
  set: (id: string) => emit('switch', id),
})

function onTabClick(tab: { props: { name: string } }): void {
  // v-model 的 setter 只在「切换到别的 TAB」时触发；
  // 这里只补上「点击已激活的 TAB → 重新加载该键」，避免一次点击触发两次加载
  if (tab.props.name === props.activeId) emit('switch', tab.props.name)
}

function onTabRemove(name: string | number): void {
  emit('close', String(name))
}
</script>

<template>
  <div class="key-tab-strip">
    <div class="tabs-area">
      <el-tabs
        ref="tabsRef"
        v-model="activeName"
        type="card"
        @tab-click="onTabClick"
        @tab-remove="onTabRemove">
        <el-tab-pane v-for="tab in props.tabs" :key="tab.id" :name="tab.id" closable>
          <template #label>
            <span class="key-tab-label" :title="tab.redisKey.key">{{ tab.redisKey.key }}</span>
          </template>
        </el-tab-pane>
      </el-tabs>

      <!-- 多TAB：溢出时才出现（与左右切换箭头同步） -->
      <TabScrollbar
        v-if="scrollable"
        :thumb-width="thumbWidth"
        :thumb-left="thumbLeft"
        @pointerdown="onThumbPointerDown" />
    </div>
    <span
      class="close-all-keys-btn"
      :title="t('redisValue.keyTabsCloseAll')"
      @click="emit('closeAll')">
      <me-icon icon="el-icon-circle-close" />
    </span>
  </div>
</template>

<style scoped lang="scss">
.key-tab-strip {
  display: flex;
  align-items: center;
  padding: 0 10px;
  border: 1px solid var(--el-border-color);
  border-bottom: none;
  flex-shrink: 0;

  :deep(.el-tabs) {
    flex-shrink: 0; // 宽度由 .tabs-area 控制，自身不参与纵向伸缩
    min-width: 0;

    .el-tabs__header {
      margin: 0 0 -1px 0;
    }

    .el-tabs__nav-wrap {
      height: 28px;
    }

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
      padding: 0 10px !important;
      box-sizing: border-box;
      margin: 0;
      border: 1px solid var(--el-border-color) !important;
      border-bottom: none !important;

      &.is-active {
        border-bottom: 2px solid var(--el-color-primary) !important;
      }

      .key-tab-label {
        flex: 1;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        margin-right: 6px;
        font-size: 13px;
      }

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

  // tabs 区域：纵向排列（tabs + 滚动条），占满剩余宽度
  .tabs-area {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  .close-all-keys-btn {
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
