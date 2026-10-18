<script setup lang="ts">
// #region 导入
import { listen } from '@tauri-apps/api/event'
import { computed, inject, onUnmounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import type { MeColumn } from '@/components/me-table-column'
import MeWebsite from '@/components/MeWebsite.vue'
import { shareProvideKey } from '@/types/me-interface'
import { meConfirm, meCopy, meCommands, meOk } from '@/utils/util'
import NodeList from '@/views/ext/NodeList.vue'
// #endregion

// #region 核心状态

const { t } = useI18n()
// 共享数据
const share = inject(shareProvideKey)!

const node = ref('')
const keyword = ref('')
const monitoring = ref(false)

// 与 Tauri `monitor` 事件 payload 一致（表格列 datetime / command）
interface MonitorRow {
  id?: string
  datetime?: string
  command?: string
}
const dataList = ref<MonitorRow[]>([])
const filterDataList = computed(() => {
  const key = keyword.value.toLowerCase()
  return dataList.value.filter(row => {
    if (!key) return true
    const text = `${row.command ?? ''} ${row.datetime ?? ''}`.toLowerCase()
    return text.includes(key)
  })
})

// 列描述：时间列导出/悬停保留完整日期时间（取 prop 原值），单元格经 #cell-datetime 只显时分秒（与迁移前一致）
const monitorColumns = computed<MeColumn[]>(() => [
  { key: 'datetime', prop: 'datetime', title: t('redisMonitor.time'), width: 118 },
  { key: 'command', prop: 'command', title: t('redisMonitor.command'), minWidth: 200 },
  { key: 'action', title: t('action'), width: 80, align: 'center' },
])

// 监控函数防抖
const loading = ref(false)
const monitor = async () => {
  loading.value = true
  try {
    if (monitoring.value) {
      unlisten?.()
      await meCommands.monitorStop(share.conn!.id)
      monitoring.value = false
      meOk(t('redisMonitor.monitorStopped'))
    } else {
      meConfirm(t('redisMonitor.monitorHint'), async () => {
        await tauriListen()
        await meCommands.monitor(share.conn!.id, node.value)
        monitoring.value = true
        meOk(t('redisMonitor.monitorStarted'))
      })
    }
  } finally {
    loading.value = false
  }
}

function clearData() {
  dataList.value = []
}

// 监听消息
let unlisten: (() => void) | null = null
async function tauriListen() {
  unlisten = await listen<MonitorRow>('monitor', event => {
    const payload = event.payload
    if (payload.id !== share.conn!.id) return
    dataList.value.unshift(payload)
  })
}

async function tauriUnlisten() {
  unlisten?.()
}
onUnmounted(() => tauriUnlisten())
// #endregion
</script>

<template>
  <div class="redis-monitor">
    <div class="me-flex header">
      <div class="me-flex">
        <me-button
          icon="el-icon-delete"
          :info="t('redisMonitor.clearMessage')"
          @click="clearData"
          :disabled="dataList.length === 0"
          placement="top" />
        <node-list v-model="node" init-node :disabled="monitoring" style="margin-left: 10px" />
        <me-website to="monitor" />
      </div>
      <div>
        <el-input
          v-model="keyword"
          :placeholder="t('redisMonitor.keyword')"
          style="width: 280px; margin: 0 10px"
          clearable />
        <me-button
          :icon="monitoring ? 'el-icon-video-pause' : 'el-icon-user'"
          @click="monitor"
          type="primary"
          :loading="loading">
          {{ monitoring ? t('redisMonitor.monitorStop') : t('redisMonitor.monitorStart') }}
        </me-button>
      </div>
    </div>
    <div class="table">
      <me-table
        :data="filterDataList"
        :columns="monitorColumns"
        :total="dataList.length"
        :default-sort="{ prop: 'datetime', order: 'descending' }"
        export-name="monitor">
        <template #cell-datetime="{ row }">{{ row.datetime?.slice(11) }}</template>
        <template #cell-action="{ row }">
          <me-icon
            :info="t('copy')"
            icon="el-icon-document-copy"
            class="icon-btn"
            style="justify-content: center"
            @click="meCopy(row.command)" />
        </template>
      </me-table>
    </div>
  </div>
</template>

<style scoped lang="scss">
.redis-monitor {
  height: 100%;
  overflow: hidden;

  display: flex;
  flex-direction: column;

  .header {
    :deep(.el-input-group__prepend) {
      padding: 0 14px;
    }
  }

  .table {
    flex-grow: 1;
    height: 0;
    margin: 10px 0;
  }
}
</style>
