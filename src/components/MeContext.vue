<script setup lang="ts">
// 说明: 自定义右键菜单（使用el-dropdown的虚拟触发实现）
import { ref, useTemplateRef } from 'vue'

defineExpose({ showMenu })
const emit = defineEmits(['handleCommand', 'handleClose'])

const dropdownRef = useTemplateRef('dropdownRef')
const position = ref({ top: 0, left: 0, bottom: 0, right: 0 })
const triggerRef = ref({ getBoundingClientRect: () => position.value })

function showMenu(e: MouseEvent): void {
  const { clientX, clientY } = e
  position.value = DOMRect.fromRect({ x: clientX, y: clientY })
  e.preventDefault()
  dropdownRef.value?.handleOpen()
}

function handleCommand(command: string): void {
  emit('handleCommand', command)
}

function handleClose(isOpen: boolean): void {
  if (!isOpen) {
    emit('handleClose')
  }
}
</script>

<template>
  <el-dropdown
    ref="dropdownRef"
    @command="handleCommand"
    @visible-change="handleClose"
    :virtual-ref="triggerRef"
    virtual-triggering
    :show-arrow="false"
    trigger="contextmenu"
    placement="bottom-start">
    <template #dropdown>
      <slot />
    </template>
  </el-dropdown>
</template>
