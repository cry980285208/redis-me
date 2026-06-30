<script setup lang="ts">
// 说明: 支持tooltip的按钮
withDefaults(defineProps<{ info?: string; placement?: string; icon?: string }>(), {
  info: '',
  placement: 'auto',
  icon: '',
})
</script>

<template>
  <el-tooltip :disabled="!info" :content="info" :show-after="1000" :placement>
    <!-- Element Plus 原生图标 -->
    <el-button v-bind="$attrs" :icon="icon" v-if="icon.startsWith('el-icon')">
      <template v-for="(, key) in $slots" v-slot:[key]>
        <slot :name="key" />
      </template>
    </el-button>

    <!-- 项目中自定义的SVG图标 -->
    <el-button v-bind="$attrs" v-else>
      <template #icon>
        <SvgIcon :name="icon" />
      </template>
      <template v-for="(, key) in $slots" v-slot:[key]>
        <slot :name="key" />
      </template>
    </el-button>
  </el-tooltip>
</template>
