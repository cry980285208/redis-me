<script setup lang="ts">
import { ref } from 'vue'

// 多TAB 滚动条（纯展示组件）。
// 显示与位置由 src/utils/tab-scrollbar.ts 驱动：
// - 只有 TAB 溢出（EP 左右切换箭头出现）时才渲染
// - 拖动滑块/点击轨道 → 通知父组件滚动 EP 的 nav
const props = defineProps<{
  /** 滑块宽度（百分比） */
  thumbWidth: number
  /** 滑块左边距（百分比） */
  thumbLeft: number
}>()

const emit = defineEmits<{
  /** 按下滑块或轨道 */
  pointerdown: [e: PointerEvent, track: HTMLElement | null]
}>()

const trackRef = ref<HTMLElement | null>(null)

function onPointerDown(e: PointerEvent): void {
  emit('pointerdown', e, trackRef.value)
}
</script>

<template>
  <div ref="trackRef" class="tab-scrollbar" @pointerdown="onPointerDown">
    <div
      class="tab-scrollbar__thumb"
      :style="{ width: props.thumbWidth + '%', left: props.thumbLeft + '%' }" />
  </div>
</template>

<style scoped lang="scss">
.tab-scrollbar {
  height: 8px;
  padding: 0 2px;
  flex-shrink: 0;
  cursor: pointer;
  user-select: none;
  position: relative; // 滑块绝对定位的参照

  &__thumb {
    // 必须绝对定位，否则 left 百分比不生效（flex 子项只认 left 偏移量为 0）
    position: absolute;
    top: 50%;
    transform: translateY(-50%);
    height: 4px;
    min-width: 16px;
    border-radius: 2px;
    background-color: var(--el-border-color-darker);
    transition: background-color 0.2s;

    &:hover {
      background-color: var(--el-text-color-secondary);
    }
  }
}
</style>
