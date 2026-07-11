<script setup lang="ts">
/** Hash 全量字段名（HKEYS）弹框：me-table 前端分页，避免万级字段卡顿 */
import { computed, inject, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import { shareProvideKey } from '@/types/me-interface'
import type { BytesFormat } from '@/types/tauri-specta'
import { meCommands, meErr } from '@/utils/util'

const { t } = useI18n()
const share = inject(shareProvideKey)!

const visible = ref(false)
const loading = ref(false)
const keyList = ref<string[]>([])
const keyword = ref('')

const displayList = computed(() => {
  const kw = keyword.value.trim().toLowerCase()
  const rows = kw ? keyList.value.filter(k => k.toLowerCase().includes(kw)) : keyList.value
  return rows.map(key => ({ key }))
})

const dialogTitle = computed(() => {
  const label = t('redisValue.allHashKeys')
  if (loading.value) return label
  return `${label} (${keyList.value.length})`
})

async function open(valFmt: BytesFormat | null) {
  const conn = share.conn
  const redisKey = share.redisKey
  if (!conn || !redisKey) return

  visible.value = true
  loading.value = true
  keyword.value = ''
  try {
    keyList.value = await meCommands.hashKeys(conn.id, { key: redisKey, valFmt })
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
  <me-dialog :title="dialogTitle" icon="el-icon-key" v-model="visible" width="700">
    <div v-loading="loading" class="table-hash-keys">
      <el-input v-model="keyword" :placeholder="t('redisValue.tableKeyword')" clearable />
      <div class="table-hash-keys-main">
        <me-table
          v-if="displayList.length"
          layout="sizes, prev, pager, next, jumper"
          :data="displayList"
          export-name="hash-keys"
          height="100%"
          stripe
          border
          :default-sort="{ prop: 'key', order: 'ascending' }">
          <el-table-column type="index" label="#" width="60" align="center" />
          <el-table-column :label="t('redisValue.key')" prop="key" show-overflow-tooltip sortable />
        </me-table>
        <el-empty v-else-if="!loading" :description="t('redisValue.hashKeysEmpty')" />
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
