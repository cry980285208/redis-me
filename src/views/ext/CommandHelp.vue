<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import MeWebsite from '@/components/MeWebsite.vue'
import { commandHelp } from '@/locales/cmd'
import { sortVersionsDesc } from '@/utils/redis-version'
import { isZh } from '@/utils/util'

const { t } = useI18n()

const visible = defineModel<boolean>('visible', { default: false })

const props = withDefaults(
  defineProps<{
    /** 初始过滤关键词 */
    initialKeyword?: string
    /** 初始分组过滤 */
    initialGroup?: string
  }>(),
  { initialKeyword: '', initialGroup: '' },
)

const keyword = ref(props.initialKeyword)
const group = ref(props.initialGroup)
const tableKey = ref(0)
/** 列头筛选（MeTable 分页前先过滤全量数据，列上 filter-method 仅保留 UI） */
const activeFilters = ref<Record<string, unknown[]>>({})

const groupList = computed(() => new Set(commandHelp.value.map(row => row.group)))
// 版本筛选项按语义版本降序（与运行配置参考列表共用 redis-version）
const sinceFilters = computed(() =>
  sortVersionsDesc([...new Set(commandHelp.value.map(row => row.since))]).map(value => ({
    text: value,
    value,
  })),
)
const readonlyFilters = computed(() => [
  { text: t('redisTerminal.readonlyYes'), value: true },
  { text: t('redisTerminal.readonlyNo'), value: false },
])

const filterDataList = computed(() => {
  let rows = commandHelp.value
  const key = keyword.value.toLowerCase().trim()
  if (group.value) rows = rows.filter(row => row.group === group.value)
  if (key) {
    rows = rows.filter(
      row => row.title.toLowerCase().includes(key) || row.summary.toLowerCase().includes(key),
    )
  }
  const sinces = activeFilters.value.since as string[] | undefined
  if (sinces?.length) rows = rows.filter(row => sinces.includes(row.since))
  const readonlys = activeFilters.value.readonly as boolean[] | undefined
  if (readonlys?.length) rows = rows.filter(row => readonlys.includes(!!row.readonly))
  return rows
})

function onFilterChange(filters: Record<string, unknown[]>) {
  // EP 每次只回传当前列，需合并保留其它列已选条件
  activeFilters.value = { ...activeFilters.value, ...filters }
}

/** 外部调用打开弹窗 */
function open(options?: { keyword?: string; group?: string }) {
  keyword.value = options?.keyword ?? props.initialKeyword
  group.value = options?.group ?? props.initialGroup
  activeFilters.value = {}
  tableKey.value++
  visible.value = true
}

/** 外部调用关闭弹窗 */
function close() {
  visible.value = false
}

/** 只读列表头：英文 Read-only 较宽，中文只读可窄一些 */
const readonlyColWidth = computed(() => (isZh.value ? 88 : 120))

defineExpose({ open, close })
</script>

<template>
  <me-dialog
    v-model="visible"
    icon="el-icon-document"
    :title="t('redisTerminal.commandTitle')"
    width="80vw">
    <div style="height: 100%; display: flex; flex-direction: column">
      <div class="me-flex">
        <div class="me-flex">
          <el-select
            v-model="group"
            style="width: 200px"
            :placeholder="t('redisTerminal.group')"
            clearable
            filterable>
            <el-option v-for="item in groupList" :key="item" :value="item">{{ item }}</el-option>
          </el-select>
          <me-website to="command" />
        </div>
        <el-input
          v-model="keyword"
          :placeholder="t('redisTerminal.keywordHint')"
          style="width: 300px"
          clearable />
      </div>

      <div class="cmd-table" style="margin-top: 10px; flex-grow: 1; height: 0">
        <me-table
          :key="tableKey"
          ref="table"
          :data="filterDataList"
          border
          stripe
          height="100%"
          export-name="command"
          :default-sort="{ prop: 'key', order: 'ascending' }"
          @filter-change="onFilterChange">
          <el-table-column
            :label="t('redisTerminal.group')"
            prop="group"
            width="120"
            show-overflow-tooltip
            sortable />
          <el-table-column
            class-name="col-cmd"
            :label="t('redisTerminal.command')"
            prop="key"
            width="150"
            show-overflow-tooltip
            sortable>
            <template #default="{ row }">
              <!-- 悬停选站点，打开对应命令文档（路径统一为小写+空格转连字符） -->
              <me-website to="command" :command="row.key" :label="row.key" margin-left="0" />
            </template>
          </el-table-column>
          <el-table-column :label="t('redisTerminal.usage')" prop="usage" show-overflow-tooltip />
          <el-table-column
            :label="t('redisTerminal.summary')"
            prop="summary"
            show-overflow-tooltip />
          <el-table-column
            :label="t('redisTerminal.since')"
            prop="since"
            column-key="since"
            width="108"
            show-overflow-tooltip
            :filters="sinceFilters"
            :filter-method="() => true" />
          <el-table-column
            :label="t('redisTerminal.readonly')"
            prop="readonly"
            column-key="readonly"
            :width="readonlyColWidth"
            align="center"
            :filters="readonlyFilters"
            :filter-method="() => true">
            <template #default="{ row }">
              {{ row.readonly ? t('redisTerminal.readonlyYes') : t('redisTerminal.readonlyNo') }}
            </template>
          </el-table-column>
        </me-table>
      </div>
    </div>
  </me-dialog>
</template>

<style scoped lang="scss">
.cmd-table {
  :deep(.el-table__header .cell:has(.el-table__column-filter-trigger)) {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  :deep(.el-table__header th.is-center .cell:has(.el-table__column-filter-trigger)) {
    justify-content: center;
  }

  :deep(.el-table__column-filter-trigger) {
    flex-shrink: 0;
  }

  /* me-website（el-dropdown）在单元格内易顶对齐，强制垂直居中 */
  :deep(td.col-cmd .cell) {
    display: flex;
    align-items: center;
  }
}
</style>
