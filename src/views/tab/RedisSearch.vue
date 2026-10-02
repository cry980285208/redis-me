<script setup lang="ts">
// 搜索页两层：索引表，点「查询」进入该索引的 FT.SEARCH。字段定义和 FT.INFO 原文各一个弹框。
import { computed, inject, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import { connUiProvideKey, shareProvideKey } from '@/types/me-interface'
import type { SearchHit, SearchIndexInfo } from '@/types/tauri-specta'
import type { TableExportMatrix } from '@/utils/export'
import { indexDdl } from '@/utils/search-ddl'
import { defaultSettings } from '@/utils/settings-defaults'
import { KEY_REFRESH, bus, meCommands, meConfirm, meFormatDisplayValue, meOk } from '@/utils/util'

const { t } = useI18n()
const share = inject(shareProvideKey)!
const connUi = inject(connUiProvideKey)!
const canEdit = computed(() => !share.readonly)

const loadingList = ref(false)
const loadingQuery = ref(false)
const pageMode = ref<'list' | 'query'>('list')
const keyword = ref('')
const indexes = ref<SearchIndexInfo[]>([])
const selected = ref<SearchIndexInfo | null>(null)
const queryText = ref('')
const withScores = ref(false)
const hits = ref<SearchHit[]>([])

const detailVisible = ref(false)
const infoVisible = ref(false)
const ddlVisible = ref(false)
const sampleVisible = ref(false)
const sampleKind = ref('')
const loadingSample = ref(false)

// 只按名称、前缀模糊匹配，大小写不敏感。
const filteredIndexes = computed(() => {
  const q = keyword.value.trim().toLowerCase()
  if (!q) return indexes.value
  return indexes.value.filter(row => [row.name, row.prefixes].join('\n').toLowerCase().includes(q))
})

// FT.INFO 原文。走终端的展示格式，避免 JSON.parse 碰到裸 NaN。
const infoText = computed(() => meFormatDisplayValue(selected.value?.raw ?? '', true))
// 由原文还原 FT.CREATE，不另存一份。
const ddlText = computed(() => indexDdl(selected.value?.raw ?? '', selected.value?.name ?? ''))

// 弹框标题：有选中索引时是「标签 索引名」。
function indexTitle(label: string): string {
  return selected.value ? `${label} ${selected.value.name}` : label
}

// 本页命中的字段名，按第一次出现的顺序。列是动态的，不跟 schema 对齐。
const resultColumns = computed(() => {
  const names: string[] = []
  for (const hit of hits.value) {
    for (const field of hit.fields) {
      if (!names.includes(field.field)) names.push(field.field)
    }
  }
  return names
})

// 这一列在当前命中里的值，没有则为空。
function fieldValue(hit: SearchHit, name: string): string {
  return hit.fields.find(field => field.field === name)?.value ?? ''
}

// 结果里的字段名可能是 attribute，也可能是 identifier，两种都交给后端认向量。
function vectorFieldNames(index: SearchIndexInfo): string[] {
  const names: string[] = []
  for (const field of index.fields) {
    if (field.fieldType.toUpperCase() !== 'VECTOR') continue
    for (const name of [field.attribute, field.identifier]) {
      if (name && !names.includes(name)) names.push(name)
    }
  }
  return names
}

// 与索引表列一致（操作列不导出）
function exportIndexes(data: unknown[]): TableExportMatrix {
  const rows = (data as SearchIndexInfo[]).map(row => [
    row.name,
    row.prefixes,
    row.numDocs,
    row.numRecords || '—',
    row.numTerms || '—',
    String(row.fields.length),
  ])
  return {
    headers: [
      t('redisSearch.name'),
      t('redisSearch.prefixes'),
      t('redisSearch.numDocs'),
      t('redisSearch.records'),
      t('redisSearch.terms'),
      t('redisSearch.fields'),
    ],
    rows,
  }
}

// 与结果表列一致。向量格导出完整数组，不是界面上被省略后的样子。
function exportHits(data: unknown[]): TableExportMatrix {
  const headers = [t('redisSearch.docKey')]
  if (withScores.value) headers.push(t('redisSearch.score'))
  headers.push(...resultColumns.value)
  const rows = (data as SearchHit[]).map(hit => {
    const cells = [hit.key]
    if (withScores.value) cells.push(hit.score ?? '')
    for (const name of resultColumns.value) cells.push(fieldValue(hit, name))
    return cells
  })
  return { headers, rows }
}

// 空查询按 *。条数用设置里的字段扫描，只取这一页，不再向服务器要后面的。
async function runSearch(): Promise<void> {
  if (!selected.value) return
  const n = Number(meTauri.settings.fieldScanCount)
  const count = Number.isFinite(n) && n >= 1 ? Math.floor(n) : defaultSettings.fieldScanCount
  loadingQuery.value = true
  try {
    const res = await meCommands.searchQuery(share.conn!.id, {
      index: selected.value.name,
      query: queryText.value.trim() || '*',
      offset: 0,
      count,
      withScores: withScores.value,
      noContent: false,
      vectorFields: vectorFieldNames(selected.value),
    })
    hits.value = res.hits
  } finally {
    loadingQuery.value = false
  }
}

// prefer 用来在导入样例后仍停在该索引。索引没了就退回列表。
async function loadIndexes(prefer?: string): Promise<void> {
  loadingList.value = true
  try {
    const keep = prefer ?? selected.value?.name
    indexes.value = await meCommands.searchIndexList(share.conn!.id)
    const next = indexes.value.find(item => item.name === keep) ?? null
    selected.value = next
    if (pageMode.value === 'query' && next) {
      await runSearch()
    } else if (!next) {
      leaveIndex()
    }
  } finally {
    loadingList.value = false
  }
}

// 字段、原文、DDL 共用 selected，同时只开一个弹框。
function openIndex(row: SearchIndexInfo, which: 'fields' | 'info' | 'ddl'): void {
  selected.value = row
  detailVisible.value = which === 'fields'
  infoVisible.value = which === 'info'
  ddlVisible.value = which === 'ddl'
}

// 更多菜单：DDL 谁都能看，删除只在可写时出现。
function onMore(row: SearchIndexInfo, cmd: string): void {
  if (cmd === 'ddl') openIndex(row, 'ddl')
  else if (cmd === 'drop') dropIndex(row)
}

// 进入查询页时清空条件和分数，马上搜一次。
function openQuery(row: SearchIndexInfo): void {
  selected.value = row
  queryText.value = ''
  withScores.value = false
  hits.value = []
  pageMode.value = 'query'
  void runSearch()
}

// 点结果里的键：打开键值页，不走键树的 chooseKey。
function openDoc(key: string): void {
  const redisKey = { key, bytes: '' }
  share.redisKey = redisKey
  share.tabName = 'value'
  bus.emit(KEY_REFRESH)
  connUi.scrollKeyToTree(redisKey)
}

// 每次打开都重选样例。
function openSample(): void {
  sampleKind.value = ''
  sampleVisible.value = true
}

// 同名索引已在时只提示，不覆盖。成功后刷新列表并停在该索引。
async function loadSample(): Promise<void> {
  if (!sampleKind.value) return
  loadingSample.value = true
  try {
    const res = await meCommands.searchSampleLoad(share.conn!.id, sampleKind.value)
    sampleVisible.value = false
    meOk(
      t(res.created ? 'redisSearch.sampleCreated' : 'redisSearch.sampleExists', {
        name: res.index,
      }),
    )
    await loadIndexes(res.index)
  } finally {
    loadingSample.value = false
  }
}

// 退出查询页，并关掉字段、原文、DDL 三个弹框。
function leaveIndex(): void {
  selected.value = null
  pageMode.value = 'list'
  hits.value = []
  detailVisible.value = false
  infoVisible.value = false
  ddlVisible.value = false
}

// 不带 DD，文档键保留。删的是当前索引就退回列表。
function dropIndex(row: SearchIndexInfo): void {
  const name = row.name
  meConfirm(t('redisSearch.dropConfirm', { name }), async () => {
    await meCommands.searchIndexDrop(share.conn!.id, name)
    meOk(t('redisSearch.dropOk'))
    if (selected.value?.name === name) leaveIndex()
    await loadIndexes()
  })
}

onMounted(() => {
  void loadIndexes()
})

// 换库不重建页面。索引跟当前库走，库号变了要重拉。
watch(
  () => share.conn!.db,
  () => {
    void loadIndexes()
  },
)
</script>

<template>
  <div class="redis-search">
    <!-- 索引列表 -->
    <template v-if="pageMode === 'list'">
      <div class="me-flex header">
        <div>
          <!-- 只读不提供写入样例 -->
          <el-button v-if="canEdit" icon="el-icon-document-add" @click="openSample">
            {{ t('redisSearch.sample') }}
          </el-button>
        </div>
        <div>
          <el-input
            v-model="keyword"
            :placeholder="t('redisSearch.filter')"
            style="width: 300px; margin-right: 10px"
            clearable />
          <el-button
            type="primary"
            icon="el-icon-search"
            :loading="loadingList"
            @click="loadIndexes()" />
        </div>
      </div>

      <div class="table">
        <me-table
          :data="filteredIndexes"
          v-loading="loadingList"
          export-name="search-indexes"
          :export-rows="exportIndexes"
          row-key="name">
          <el-table-column
            prop="name"
            :label="t('redisSearch.name')"
            fixed="left"
            show-overflow-tooltip />
          <el-table-column prop="prefixes" :label="t('redisSearch.prefixes')" show-overflow-tooltip>
            <template #header>
              <me-icon
                icon="el-icon-info-filled"
                :icon-left="false"
                placement="top"
                :name="t('redisSearch.prefixes')"
                :info="t('redisSearch.prefixTip')" />
            </template>
          </el-table-column>

          <el-table-column
            prop="numDocs"
            :label="t('redisSearch.numDocs')"
            width="90"
            align="right">
            <template #header>
              <me-icon
                icon="el-icon-info-filled"
                :icon-left="false"
                placement="top"
                :name="t('redisSearch.numDocs')"
                :info="t('redisSearch.docsTip')" />
            </template>
          </el-table-column>
          <el-table-column :label="t('redisSearch.records')" width="100" align="right">
            <template #header>
              <me-icon
                icon="el-icon-info-filled"
                :icon-left="false"
                placement="top"
                :name="t('redisSearch.records')"
                :info="t('redisSearch.recordsTip')" />
            </template>
            <template #default="{ row }">{{ row.numRecords || '—' }}</template>
          </el-table-column>

          <el-table-column :label="t('redisSearch.terms')" width="90" align="right">
            <template #header>
              <me-icon
                icon="el-icon-info-filled"
                :icon-left="false"
                placement="top"
                :name="t('redisSearch.terms')"
                :info="t('redisSearch.termsTip')" />
            </template>
            <template #default="{ row }">{{ row.numTerms || '—' }}</template>
          </el-table-column>

          <!-- 点数字打开字段定义，不单独占一列操作 -->
          <el-table-column :label="t('redisSearch.fields')" width="90" align="center">
            <template #header>
              <me-icon
                icon="el-icon-info-filled"
                :icon-left="false"
                placement="top"
                :name="t('redisSearch.fields')"
                :info="t('redisSearch.fieldsTip')" />
            </template>
            <template #default="{ row }">
              <el-link type="primary" underline="never" @click="openIndex(row, 'fields')">
                {{ row.fields.length }}
              </el-link>
            </template>
          </el-table-column>

          <!-- 查询、原文、DDL 始终可看；删除只在可写时出现 -->
          <el-table-column :label="t('action')" width="80" fixed="right" align="center">
            <template #default="{ row }">
              <div class="action-icons">
                <me-icon
                  icon="el-icon-search"
                  class="icon-btn"
                  :info="t('redisSearch.query')"
                  @click="openQuery(row)" />
                <me-icon
                  icon="el-icon-info-filled"
                  class="icon-btn"
                  :info="t('redisSearch.info')"
                  @click="openIndex(row, 'info')" />
                <el-dropdown
                  trigger="click"
                  placement="bottom-end"
                  @command="(cmd: string) => onMore(row, cmd)">
                  <me-icon icon="el-icon-more-filled" class="icon-btn" />
                  <template #dropdown>
                    <el-dropdown-menu>
                      <el-dropdown-item command="ddl">
                        <me-icon icon="me-icon-copy-command" :name="t('redisSearch.ddl')" />
                      </el-dropdown-item>
                      <el-dropdown-item v-if="canEdit" command="drop">
                        <me-icon icon="el-icon-delete" :name="t('redisSearch.drop')" />
                      </el-dropdown-item>
                    </el-dropdown-menu>
                  </template>
                </el-dropdown>
              </div>
            </template>
          </el-table-column>
        </me-table>
      </div>
    </template>

    <!-- 单个索引的 FT.SEARCH。返回后仍留在列表，不拆页面 -->
    <template v-else-if="selected">
      <div class="me-flex header">
        <div class="me-flex query-side">
          <el-button icon="el-icon-back" @click="pageMode = 'list'">{{
            t('redisSearch.back')
          }}</el-button>
          <me-website to="search" />
          <span class="index-name">{{ selected.name }}</span>
        </div>
        <div class="query-tools">
          <!-- 勾选变化立刻重查，空输入按 * -->
          <el-checkbox v-model="withScores" @change="runSearch">
            {{ t('redisSearch.withScores') }}
          </el-checkbox>
          <el-input
            v-model="queryText"
            :placeholder="t('redisSearch.queryPlaceholder')"
            style="width: 300px"
            clearable
            @keyup.enter="runSearch" />
          <el-button
            type="primary"
            icon="el-icon-search"
            :loading="loadingQuery"
            @click="runSearch" />
        </div>
      </div>

      <div class="table hits" v-loading="loadingQuery">
        <me-table :data="hits" export-name="search-hits" :export-rows="exportHits">
          <!-- 点键打开键值页，不走键树的 chooseKey -->
          <el-table-column
            :label="t('redisSearch.docKey')"
            width="180"
            fixed="left"
            show-overflow-tooltip>
            <template #default="{ row }">
              <el-link type="primary" underline="never" @click="openDoc(row.key)">
                {{ row.key }}
              </el-link>
            </template>
          </el-table-column>
          <el-table-column v-if="withScores" :label="t('redisSearch.score')" width="200">
            <template #default="{ row }">{{ row.score }}</template>
          </el-table-column>
          <!-- 列跟本次命中走，不跟 schema 对齐；不换行，超出由表格提示 -->
          <el-table-column
            v-for="col in resultColumns"
            :key="col"
            :label="col"
            :min-width="Math.max(140, col.length * 8 + 28)"
            show-overflow-tooltip>
            <template #default="{ row }">{{ fieldValue(row, col) }}</template>
          </el-table-column>
        </me-table>
      </div>
    </template>

    <!-- 字段定义：标识、属性名、类型 -->
    <el-dialog v-model="detailVisible" width="720px" align-center draggable destroy-on-close>
      <template #header>
        <me-icon icon="el-icon-info-filled" :name="indexTitle(t('redisSearch.fields'))" />
      </template>
      <el-table :data="selected?.fields ?? []" border stripe max-height="420" show-overflow-tooltip>
        <el-table-column prop="identifier" :label="t('redisSearch.identifier')" min-width="140" />
        <el-table-column prop="attribute" :label="t('redisSearch.attribute')" min-width="120" />
        <el-table-column prop="fieldType" :label="t('redisSearch.fieldType')" width="110" />
      </el-table>
    </el-dialog>

    <!-- 由 FT.INFO 还原的 FT.CREATE，不是服务器保存的原文 -->
    <me-dialog
      v-model="ddlVisible"
      :title="indexTitle(t('redisSearch.ddl'))"
      icon="me-icon-copy-command"
      width="720px">
      <me-code :model-value="ddlText" mode="redis" read-only copyable />
    </me-dialog>

    <!-- FT.INFO 原文 -->
    <me-dialog
      v-model="infoVisible"
      :title="indexTitle(t('redisSearch.info'))"
      icon="el-icon-info-filled"
      width="720px">
      <me-code :model-value="infoText" read-only />
    </me-dialog>

    <!-- 样例：已有同名索引时不覆盖 -->
    <el-dialog v-model="sampleVisible" width="520px" align-center draggable destroy-on-close>
      <template #header>
        <me-icon icon="el-icon-document-add" :name="t('redisSearch.sampleTitle')" />
      </template>
      <p class="sample-hint">{{ t('redisSearch.sampleHint') }}</p>
      <el-radio-group v-model="sampleKind" class="sample-list">
        <el-radio value="bikes" border>
          <div>{{ t('redisSearch.sampleBikes') }}</div>
          <div class="sample-desc">{{ t('redisSearch.sampleBikesHint') }}</div>
        </el-radio>
        <el-radio value="movies" border>
          <div>{{ t('redisSearch.sampleMovies') }}</div>
          <div class="sample-desc">{{ t('redisSearch.sampleMoviesHint') }}</div>
        </el-radio>
      </el-radio-group>
      <template #footer>
        <el-button @click="sampleVisible = false">{{ t('cancel') }}</el-button>
        <el-button
          type="primary"
          :disabled="!sampleKind"
          :loading="loadingSample"
          @click="loadSample">
          {{ t('ok') }}
        </el-button>
      </template>
    </el-dialog>
  </div>
</template>

<style scoped lang="scss">
.redis-search {
  height: 100%;
  overflow: hidden;
  display: flex;
  flex-direction: column;

  .table {
    margin-top: 10px;
    flex-grow: 1;
    height: 0;
  }

  :deep(th .cell),
  .hits :deep(td .cell) {
    white-space: nowrap;
  }
}

.query-side,
.query-tools {
  align-items: center;
}

.query-tools {
  display: flex;
  gap: 10px;
}

.index-name {
  margin-left: 10px;
  font-weight: 600;
}

:deep(th.is-right .cell > .icon-main) {
  justify-content: flex-end;
}

.sample-hint {
  margin: 0 0 12px;
}

.sample-list {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: 10px;

  :deep(.el-radio) {
    width: 100%;
    height: auto;
    margin-right: 0;
    padding: 10px 12px;
    white-space: normal;
  }

  :deep(.el-radio__label) {
    line-height: 1.4;
  }
}

.sample-desc {
  margin-top: 2px;
  color: var(--el-text-color-secondary);
  font-weight: 400;
}

.action-icons {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;

  .icon-btn {
    font-size: 16px;
  }
}
</style>
