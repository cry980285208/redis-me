<script setup lang="ts">
/** Array ARINFO 元数据弹框：对标 ObjectInfo，展示 count/len/next-insert-index 等 */
import { inject, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import { shareProvideKey } from '@/types/me-interface'
import type { RedisArInfoItem } from '@/types/tauri-specta'
import { meCommands } from '@/utils/util'

const { t } = useI18n()
const share = inject(shareProvideKey)!

const visible = ref(false)
const loading = ref(false)
const rows = ref<RedisArInfoItem[]>([])

async function open() {
  const conn = share.conn
  const rk = share.redisKey
  if (!conn || !rk) return
  visible.value = true
  loading.value = true
  rows.value = []
  try {
    rows.value = await meCommands.arInfo(conn.id, rk)
  } finally {
    loading.value = false
  }
}

defineExpose({ open })
</script>

<template>
  <el-dialog v-model="visible" width="560px" destroy-on-close align-center draggable>
    <template #header>
      <me-icon icon="el-icon-info-filled" :name="t('redisValue.arInfo')" />
    </template>

    <el-table v-loading="loading" :data="rows" border stripe>
      <el-table-column :label="t('redisValue.arInfoField')" prop="field" width="200" />
      <el-table-column :label="t('redisValue.arInfoValue')" prop="value" min-width="200" />
    </el-table>
  </el-dialog>
</template>
