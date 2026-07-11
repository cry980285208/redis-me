<script setup lang="ts">
/** Hash 全量字段名/值（HKEYS/HVALS）弹框：me-table 前端分页，避免万级卡顿 */
import { computed, inject, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import { shareProvideKey } from '@/types/me-interface'
import type { BytesFormat } from '@/types/tauri-specta'
import { meCommands, meErr } from '@/utils/util'

type HashListMode = 'keys' | 'values'

const { t } = useI18n()
const share = inject(shareProvideKey)!

const visible = ref(false)
const loading = ref(false)
const mode = ref<HashListMode>('keys')
const itemList = ref<string[]>([])
const keyword = ref('')

const columnProp = computed(() => (mode.value === 'keys' ? 'key' : 'value'))

const displayList = computed(() => {
  const kw = keyword.value.trim().toLowerCase()
  const rows = kw ? itemList.value.filter(s => s.toLowerCase().includes(kw)) : itemList.value
  const prop = columnProp.value
  return rows.map(text => ({ [prop]: text }))
})

const dialogTitle = computed(() => {
  const label = mode.value === 'keys' ? t('redisValue.allHashKeys') : t('redisValue.allHashValues')
  if (loading.value) return label
  return `${label} (${itemList.value.length})`
})

const dialogIcon = computed(() => (mode.value === 'keys' ? 'el-icon-key' : 'el-icon-document'))

const emptyText = computed(() =>
  mode.value === 'keys' ? t('redisValue.hashKeysEmpty') : t('redisValue.hashValuesEmpty'),
)

const exportName = computed(() => (mode.value === 'keys' ? 'hash-keys' : 'hash-values'))

async function open(valFmt: BytesFormat | null, listMode: HashListMode = 'keys') {
  const conn = share.conn
  const redisKey = share.redisKey
  if (!conn || !redisKey) return

  mode.value = listMode
  visible.value = true
  loading.value = true
  keyword.value = ''
  const param = { key: redisKey, valFmt }
  try {
    itemList.value =
      listMode === 'keys'
        ? await meCommands.hashKeys(conn.id, param)
        : await meCommands.hashValues(conn.id, param)
  } catch (e) {
    visible.value = false
    meErr(e)
  } finally {
    loading.value = false
  }
}

defineExpose({ open })
</script>

<template>
  <me-dialog :title="dialogTitle" :icon="dialogIcon" v-model="visible" width="700">
    <div v-loading="loading" class="table-hash-keys">
      <el-input v-model="keyword" :placeholder="t('redisValue.tableKeyword')" clearable />
      <div class="table-hash-keys-main">
        <me-table
          v-if="displayList.length"
          layout="sizes, prev, pager, next, jumper"
          :data="displayList"
          :export-name="exportName"
          height="100%"
          stripe
          border
          :default-sort="{ prop: columnProp, order: 'ascending' }">
          <el-table-column type="index" label="#" width="60" align="center" />
          <el-table-column
            :label="mode === 'keys' ? t('redisValue.key') : t('redisValue.value')"
            :prop="columnProp"
            show-overflow-tooltip
            sortable />
        </me-table>
        <el-empty v-else-if="!loading" :description="emptyText" />
      </div>
    </div>
  </me-dialog>
</template>

<style scoped lang="scss">
.table-hash-keys {
  height: 100%;
  overflow: hidden;
  display: flex;
  flex-direction: column;

  .table-hash-keys-main {
    margin-top: 10px;
    flex: 1;
    min-height: 0;
  }
}
</style>
