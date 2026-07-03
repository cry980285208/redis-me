<script setup lang="ts">
import { useI18n } from 'vue-i18n'

defineProps<{ name: string; icon: string }>()

const { t } = useI18n()
</script>

<template>
  <el-tab-pane :name>
    <template #label>
      <me-icon :name="t(`tabMain.${name}`)" :icon />
    </template>
    <slot />
  </el-tab-pane>
</template>
