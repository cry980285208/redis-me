<script setup lang="ts">
import { ref } from 'vue'

// 说明: 自定义弹框，支持最大化
const fullscreen = ref(false)
withDefaults(defineProps<{ title?: string; icon?: string }>(), { title: '', icon: '' })
</script>

<template>
  <el-dialog
    v-bind="$attrs"
    align-center
    draggable
    :fullscreen="fullscreen"
    @closed="fullscreen = false"
    destroy-on-close
    append-to-body>
    <template #header>
      <div class="me-flex">
        <me-icon :name="title" :icon="icon" />
        <me-icon
          :icon="fullscreen ? 'me-icon-window-restore' : 'me-icon-window-maximize'"
          class="fullscreen-icon"
          @click="fullscreen = !fullscreen" />
      </div>
    </template>

    <template #default>
      <div :style="{ height: fullscreen ? 'calc(100vh - 70px)' : '60vh' }">
        <slot name="default" />
      </div>
    </template>
  </el-dialog>
</template>
