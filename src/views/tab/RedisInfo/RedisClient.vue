<script setup lang="ts">
// #region 导入
import { computed, inject, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import { clientTip as tips } from '@/locales/client'
import { shareProvideKey } from '@/types/me-interface'
import type { RedisClientInfo, RedisCommand } from '@/types/tauri-specta'
import type { TableExportMatrix } from '@/utils/export'
import { meConfirm, meHumanSeconds, meCommands, meOk } from '@/utils/util'
import NodeList from '@/views/ext/NodeList.vue'
// `client_list` 实际载荷字段齐全；`Required` 与 specta 在 `#[serde(default)]` 下生成的可选字段对齐
type RedisClientListRow = Required<RedisClientInfo>
// #endregion

// #region 核心状态
const { t } = useI18n()
const share = inject(shareProvideKey)!
const props = defineProps({ initNode: { type: String, default: '' } })
const node = ref(props.initNode)
const clientType = ref('')
const keyword = ref('')
const loading = ref(false)
const dataList = ref<RedisClientListRow[]>([])
const sortProperty = ref('id')
const sortOrder = ref('ascending')
// #endregion

// #region 计算属性
const canEdit = computed(() => !share.readonly)
const tipMap = computed(() => tips.value as Record<string, string | undefined>)
const filterDataList = computed(() => {
  const key = keyword.value.toLowerCase()
  return dataList.value.filter(
    row => !key || row.addr.toLowerCase().includes(key) || row.name.toLowerCase().includes(key),
  )
})

// MeTable 导出：由行数据直接计算展示文本，与表格列定义一致（改列时同步改这里）
function exportRows(data: unknown[]): TableExportMatrix {
  return {
    headers: [
      'ID',
      t('redisClient.addr'),
      t('redisClient.name'),
      t('redisClient.age'),
      t('redisClient.idle'),
      t('redisClient.cmd'),
      ...otherProps,
    ],
    rows: (data as RedisClientListRow[]).map(row => [
      String(row.id),
      row.addr,
      row.name,
      String(meHumanSeconds(row.age)),
      String(meHumanSeconds(row.idle)),
      row.cmd,
      ...otherProps.map(p => String((row as Record<string, unknown>)[p] ?? '')),
    ]),
  }
}
// #endregion

// #region 面板操作
watch(
  () => props.initNode,
  v => {
    node.value = v
  },
)

async function refresh() {
  loading.value = true
  try {
    dataList.value = (await meCommands.clientList(
      share.conn!.id,
      node.value,
      clientType.value,
    )) as RedisClientListRow[]
  } finally {
    loading.value = false
  }
}
refresh()

async function killClient(row: RedisClientListRow) {
  meConfirm(t('redisClient.killClientConfirm', { client: row.addr }), async () => {
    const param: RedisCommand = {
      command: `client kill ${row.addr}`,
      node: node.value,
      autoBroadcast: null,
      outputMode: null,
    }
    await meCommands.executeCommand(share.conn!.id, param)
    meOk(t('redisClient.killClientOk'))
    await refresh()
  })
}

// 客户端属性
const totalProps = [
  'user',
  'db',
  'id',
  'addr',
  'laddr',
  'fd',
  'name',
  'age',
  'idle',
  'flags',
  'sub',
  'psub',
  'ssub',
  'multi',
  'watch',
  'qbuf',
  'qbufFree',
  'argvMem',
  'multiMem',
  'obl',
  'oll',
  'omem',
  'totMem',
  'events',
  'cmd',
  'redir',
  'resp',
  'rbp',
  'rbs',
  'libName',
  'libVer',
  'ioThread',
  'totNetIn',
  'totNetOut',
  'totCmds',
]
const mainProps = ['id', 'addr', 'name', 'age', 'idle', 'cmd']
const otherProps = totalProps.filter(p => !mainProps.includes(p))
function propWidth(item: string) {
  if (item === 'laddr') return 180
  if (item.length == 2) return 70
  if (item.length == 3) return 80
  if (item.length == 4) return 96
  if (item.length == 5) return 100
  return 130
}
// #endregion
</script>

<template>
  <div class="redis-client">
    <div class="me-flex header">
      <div class="me-flex">
        <node-list v-model="node" style="margin-right: 10px" @change="refresh" />
        <el-select
          v-model="clientType"
          style="width: 120px"
          @change="refresh"
          :placeholder="t('redisClient.clientType')"
          clearable>
          <el-option value="NORMAL" />
          <el-option value="MASTER" />
          <el-option value="SLAVE" />
          <el-option value="REPLICA" />
          <el-option value="PUBSUB" />
        </el-select>
        <me-website to="client" />
      </div>
      <div>
        <el-input
          v-model="keyword"
          :placeholder="t('redisClient.keyword')"
          style="width: 280px; margin-right: 10px"
          clearable />
        <el-button icon="el-icon-search" @click="refresh" type="primary" :loading="loading" />
      </div>
    </div>
    <div class="table">
      <me-table
        :data="filterDataList"
        ref="table"
        v-loading="loading"
        :default-sort="{ prop: 'id', order: 'ascending' }"
        height="100%"
        export-name="client"
        :export-rows="exportRows">
        <el-table-column
          label="ID"
          prop="id"
          show-overflow-tooltip
          sortable
          width="100"
          align="right">
          <template #header>
            <el-tooltip :content="tipMap['id'] || 'id'" placement="top">
              <span>ID</span>
            </el-tooltip>
          </template>
        </el-table-column>
        <el-table-column prop="addr" show-overflow-tooltip width="180" sortable>
          <template #header>
            <el-tooltip :content="tipMap['addr'] || 'addr'" placement="top">
              <span>{{ t('redisClient.addr') }}</span>
            </el-tooltip>
          </template>
        </el-table-column>
        <el-table-column prop="name" show-overflow-tooltip width="160" sortable>
          <template #header>
            <el-tooltip :content="tipMap['name'] || 'name'" placement="top">
              <span>{{ t('redisClient.name') }}</span>
            </el-tooltip>
          </template>
        </el-table-column>
        <el-table-column
          prop="age"
          show-overflow-tooltip
          sortable
          width="140"
          align="right"
          :formatter="(row: RedisClientListRow) => meHumanSeconds(row.age)">
          <template #header>
            <el-tooltip :content="tipMap['age'] || 'age'" placement="top">
              <span>{{ t('redisClient.age') }}</span>
            </el-tooltip>
          </template>
        </el-table-column>
        <el-table-column
          prop="idle"
          show-overflow-tooltip
          sortable
          width="120"
          align="right"
          :formatter="(row: RedisClientListRow) => meHumanSeconds(row.idle)">
          <template #header>
            <el-tooltip :content="tipMap['idle'] || 'idle'" placement="top">
              <span>{{ t('redisClient.idle') }}</span>
            </el-tooltip>
          </template>
        </el-table-column>
        <el-table-column prop="cmd" show-overflow-tooltip sortable min-width="200">
          <template #header>
            <el-tooltip :content="tipMap['cmd'] || 'cmd'" placement="top">
              <span>{{ t('redisClient.cmd') }}</span>
            </el-tooltip>
          </template>
        </el-table-column>

        <el-table-column
          v-for="item in otherProps"
          :key="item"
          :prop="item"
          show-overflow-tooltip
          sortable
          :width="propWidth(item)"
          align="right">
          <template #header>
            <el-tooltip :content="tipMap[item] || item" placement="top">
              <span>{{ item }}</span>
            </el-tooltip>
          </template>
        </el-table-column>

        <el-table-column
          :label="t('action')"
          width="80"
          align="center"
          fixed="right"
          v-if="canEdit">
          <template #default="scope">
            <me-icon
              :info="t('redisClient.killClientHint')"
              icon="el-icon-CircleCloseFilled"
              class="icon-btn"
              @click="killClient(scope.row)"
              style="justify-content: center" />
          </template>
        </el-table-column>
      </me-table>
    </div>
  </div>
</template>

<style scoped lang="scss">
.redis-client {
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
    margin-top: 10px;
    flex-grow: 1;
    height: 0;
  }
}
</style>
