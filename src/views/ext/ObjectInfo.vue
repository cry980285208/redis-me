<script setup lang="ts">
/** 键 OBJECT 自省弹框：ENCODING / IDLETIME / REFCOUNT / FREQ，表格展示并附编码与不可用原因提示 */
import { computed, inject, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import { shareProvideKey } from '@/types/me-interface'
import type { RedisObjectInfo } from '@/types/tauri-specta'
import { meCommands, meHumanSeconds } from '@/utils/util'

const { t } = useI18n()
const share = inject(shareProvideKey)!

const visible = ref(false)
const loading = ref(false)
const info = ref<RedisObjectInfo | null>(null)

type ObjectRow = {
  command: string
  item: string
  value: string
  tip?: string
  unavailable?: boolean
}

const rows = computed<ObjectRow[]>(() => {
  const data = info.value
  if (!data) return []
  const na = t('redisValue.objectInfoNA')

  const idleUnavailable = !!data.idleTimeError
  const idleValue =
    data.idleTime !== null
      ? `${data.idleTime} ${t('timeUnit.second', data.idleTime)} (${meHumanSeconds(data.idleTime)})`
      : na
  const idleTip = idleUnavailable
    ? [t('redisValue.objectIdleTimeUnavailable'), data.idleTimeError].filter(Boolean).join('<br/>')
    : undefined

  const freqUnavailable = !!data.freqError
  const freqValue = data.freq !== null ? String(data.freq) : na
  const freqTip = freqUnavailable
    ? [t('redisValue.objectFreqUnavailable'), data.freqError].filter(Boolean).join('<br/>')
    : undefined

  return [
    {
      command: 'ENCODING',
      item: t('redisValue.objectEncoding'),
      value: data.encoding ?? na,
      tip: t('redisValue.objectEncodingTip'),
    },
    {
      command: 'IDLETIME',
      item: t('redisValue.objectIdleTime'),
      value: idleValue,
      tip: idleTip,
      unavailable: idleUnavailable,
    },
    {
      command: 'REFCOUNT',
      item: t('redisValue.objectRefcount'),
      value: data.refcount !== null ? String(data.refcount) : na,
    },
    {
      command: 'FREQ',
      item: t('redisValue.objectFreq'),
      value: freqValue,
      tip: freqTip,
      unavailable: freqUnavailable,
    },
  ]
})

async function open() {
  const conn = share.conn
  const rk = share.redisKey
  if (!conn || !rk) return
  visible.value = true
  loading.value = true
  info.value = null
  try {
    info.value = await meCommands.objectInfo(conn.id, rk)
  } finally {
    loading.value = false
  }
}

defineExpose({ open })
</script>

<template>
  <el-dialog v-model="visible" width="560px" destroy-on-close align-center draggable>
    <template #header>
      <me-icon icon="el-icon-info-filled" :name="t('redisValue.objectInfo')" />
    </template>

    <el-table v-loading="loading" :data="rows" border stripe>
      <el-table-column :label="t('redisValue.objectInfoCommand')" prop="command" width="110" />
      <el-table-column :label="t('redisValue.objectInfoItem')" prop="item" width="160">
        <template #default="{ row }">
          <me-icon
            v-if="row.tip && !row.unavailable"
            icon="el-icon-question-filled"
            :name="row.item"
            :info="row.tip"
            :icon-left="false"
            raw-content />
          <template v-else>{{ row.item }}</template>
        </template>
      </el-table-column>
      <el-table-column :label="t('redisValue.objectInfoValue')" prop="value" min-width="200">
        <template #default="{ row }">
          <me-icon
            v-if="row.unavailable && row.tip"
            icon="el-icon-warning-filled"
            :name="row.value"
            :info="row.tip"
            :icon-left="false"
            raw-content />
          <template v-else>{{ row.value }}</template>
        </template>
      </el-table-column>
    </el-table>
  </el-dialog>
</template>
