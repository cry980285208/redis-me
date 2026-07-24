<script setup lang="ts">
/** 键值页编辑器快捷键说明弹框 */
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import MeShortcut from '@/components/MeShortcut.vue'
import { getValueShortcuts } from '@/utils/shortcut'

const { t } = useI18n()

const visible = ref(false)
const shortcuts = computed(() => getValueShortcuts(t))

function open() {
  visible.value = true
}

defineExpose({ open })
</script>

<template>
  <el-dialog
    v-model="visible"
    width="400"
    align-center
    draggable
    :show-close="false"
    header-class="me-shortcut-dialog__header">
    <div class="value-shortcut-title">{{ t('setting.shortcutCodeMirror') }}</div>
    <MeShortcut :items="shortcuts" />
  </el-dialog>
</template>

<style scoped lang="scss">
.value-shortcut-title {
  margin-bottom: 20px;
  font-size: 14px;
  font-weight: bold;
  color: var(--el-text-color-primary);
  text-align: center;
}
</style>
