<script setup lang="ts">
// #region 导入
// 统一图标使用方式，支持el-icon-xxx图标和自定义的svg图标me-icon-xxx
import { computed } from 'vue'

const props = withDefaults(
  defineProps<{
    icon?: string
    iconLeft?: boolean
    name?: string
    hint?: boolean
    info?: string
    placement?: string
    rawContent?: boolean
    showAfter?: number
  }>(),
  {
    icon: '',
    iconLeft: true,
    name: '',
    hint: false,
    info: '',
    placement: 'auto',
    rawContent: false,
    showAfter: 1000,
  },
)
// #endregion

// #region 计算属性
// 与 MeButton 一致：el-icon-xxx 走 Element Plus 原生图标
const isElIcon = computed(() => props.icon.startsWith('el-icon-'))
// info 优先；纯 hint 时用 name 作 tooltip
const tooltipEnabled = computed(() => !!props.info || props.hint)
const tooltipContent = computed(() => props.info || (props.hint ? props.name : ''))
// info 或「非 hint 的 name」旁注；纯 hint 不显示旁注（与旧分支一致）
const showLabel = computed(() => !!props.name && (!props.hint || !!props.info))
// #endregion
</script>

<template>
  <div class="icon-main">
    <span v-if="showLabel && !iconLeft" style="margin-right: 5px">{{ name }}</span>
    <el-tooltip
      :disabled="!tooltipEnabled"
      :placement="placement"
      :content="tooltipContent"
      :raw-content="rawContent"
      :show-after="showAfter">
      <el-icon v-if="isElIcon">
        <Component :is="icon" />
      </el-icon>
      <SvgIcon v-else :name="icon" :class="{ icon: !!info }" />
    </el-tooltip>
    <span v-if="showLabel && iconLeft" style="margin-left: 5px">{{ name }}</span>
  </div>
</template>

<style scoped lang="scss">
.icon-main {
  display: flex;
  align-items: center;

  // 避免下拉框里面自带的 .el-dropdown-menu__item i 导致宽度过大
  i {
    margin-right: 0px;
  }
}
</style>
