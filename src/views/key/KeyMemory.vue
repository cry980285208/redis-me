<script setup lang="ts">
// #region 导入
import { useVirtualList } from '@vueuse/core'
import { computed, inject, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import { shareProvideKey } from '@/types/me-interface'
import type { RedisKeySize_Serialize } from '@/types/tauri-specta'
import { meHumanSize, meCommands } from '@/utils/util'
// #endregion

// #region 核心状态
const { t } = useI18n()
defineExpose({ open })
function open(data: { match: string }) {
  keyList.value = []
  visible.value = true
  form.value.match = data.match
  keyMemory()
}

// 共享数据
const share = inject(shareProvideKey)!

// 表单数据
const visible = ref(false)
const loading = ref(false)
const form = ref({
  match: '',
  needKeyType: false,
  sizeLimit: 0,
  countLimit: 10000,
  scanCount: 10000,
  scanTotal: 0,
  sleepMillis: 0,
})
// 内存分析
const keyList = ref<RedisKeySize_Serialize[]>([])
async function keyMemory() {
  loading.value = true
  try {
    const data = await meCommands.memoryUsage(share.conn!.id, form.value)
    keyList.value = data
  } finally {
    loading.value = false
  }
}
// #endregion

// #region 计算属性
const totalSize = computed(() =>
  keyList.value.map(item => item.size).reduce((sum, cur) => sum + cur, 0),
)
// 虚拟列表
const items = computed(() => keyList.value)
const { list, containerProps, wrapperProps } = useVirtualList(items, { itemHeight: 14 })
// #endregion
</script>

<template>
  <el-dialog :title="t('keyMemory.title')" v-model="visible" :width="600">
    <el-form label-position="top">
      <el-form-item :label="t('keyMemory.match')">
        <!-- 此处保留可编辑，使用更加方便 -->
        <el-input type="text" v-model="form.match" disabled />
      </el-form-item>

      <el-form-item
        :label="
          t('keyMemory.info', { total: keyList.length, size: meHumanSize(totalSize) }) +
          (keyList.length >= form.countLimit ? t('keyMemory.limit', { size: form.countLimit }) : '')
        "
        :loading="loading">
        <div v-bind="containerProps" :style="{ height: '300px', width: '100%' }">
          <div v-bind="wrapperProps">
            <div v-for="item in list" :key="item.index" class="key me-flex">
              <div class="single-line-ellipsis">{{ item.data.key }}</div>
              <div>{{ meHumanSize(item.data.size) }}</div>
            </div>
          </div>
        </div>
      </el-form-item>
    </el-form>
  </el-dialog>
</template>

<style scoped lang="scss">
.key {
  font-size: 14px;
  line-height: 14px;
  padding: 3px 4px;
  color: var(--el-color-info);
}
</style>
