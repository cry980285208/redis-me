<script setup lang="ts">
// 列头筛选：标题 + 下拉复选面板，放在 MeTable 的 header-${key} 插槽里用。
// el-table-v2 无内置筛选 UI，故自渲染；勾选即时生效、全不选即清除，
// 选中值经 v-model 回传，实际过滤由调用方在数据层完成（与关键词过滤同一处，便于叠加）。
import { useI18n } from 'vue-i18n'

import type { MeFilterValue } from './me-table-column'

const { t } = useI18n()

withDefaults(
  defineProps<{
    /** 表头文字 */
    title?: string
    /** 备选项：text 展示、value 参与比对（须与行字段值全等） */
    options: { text: string; value: MeFilterValue }[]
    /** 标题与图标居中（配合 align=center 的列） */
    center?: boolean
  }>(),
  { title: '', center: false },
)

const selected = defineModel<MeFilterValue[]>({ default: () => [] })
</script>

<template>
  <div class="me-col-filter" :class="{ 'is-center': center }">
    <span>{{ title }}</span>
    <el-dropdown trigger="click">
      <me-icon
        icon="el-icon-arrow-down"
        class="me-col-filter__trigger"
        :class="{ 'is-active': !!selected.length }" />
      <template #dropdown>
        <div>
          <div class="me-col-filter__wrap">
            <el-checkbox-group v-model="selected">
              <el-checkbox v-for="opt in options" :key="String(opt.value)" :value="opt.value">
                {{ opt.text }}
              </el-checkbox>
            </el-checkbox-group>
          </div>
          <div class="me-col-filter__bottom">
            <button
              type="button"
              class="me-col-filter__reset"
              :disabled="!selected.length"
              @click="selected = []">
              {{ t('meTable.filterReset') }}
            </button>
          </div>
        </div>
      </template>
    </el-dropdown>
  </div>
</template>

<style scoped lang="scss">
.me-col-filter {
  display: flex;
  align-items: center;
  gap: 4px;

  &.is-center {
    justify-content: center;
  }

  &__trigger {
    cursor: pointer;
    font-size: 12px;
    color: var(--el-text-color-placeholder);

    &.is-active {
      color: var(--el-color-primary);
    }
  }
}

// 面板：下拉 popper 传送到 body、脱离表头祖先，嵌套在 .me-col-filter 下写会失配，须顶层写；
// 备选项区单独滚动，底栏（重置）固定在面板底部不被滚走
.me-col-filter__wrap {
  padding: 8px 12px;
  max-height: 280px;
  overflow-y: auto;

  :deep(.el-checkbox-group) {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  :deep(.el-checkbox) {
    margin-right: 0;
    height: 20px;
  }
}

// 底栏对齐 el-table 内置筛选面板（.el-table-filter__bottom）：横线与备选项分隔；仅一个重置按钮，居中
.me-col-filter__bottom {
  border-top: 1px solid var(--el-border-color-lighter);
  padding: 8px;
  text-align: center;
}

.me-col-filter__reset {
  border: none;
  background: none;
  margin: 0;
  padding: 0 3px;
  cursor: pointer;
  // 原生 button 不继承字体（与 MeShortcut 同款处理）
  font-family: inherit;
  color: var(--el-text-color-regular);
  font-size: var(--el-font-size-small);

  &:hover:not(:disabled) {
    color: var(--el-color-primary);
  }

  &:disabled {
    color: var(--el-disabled-text-color);
    cursor: not-allowed;
  }
}
</style>
