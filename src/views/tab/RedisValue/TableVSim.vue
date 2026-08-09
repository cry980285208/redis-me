<script setup lang="ts">
/**
 * Vector Set VSIM 弹框：ELE / VALUES 相似度查询。
 * WITHSCORES 固定；可选 WITHATTRIBS / FILTER / EPSILON / EF。
 */
import { computed, inject, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import { shareProvideKey } from '@/types/me-interface'
import type { BytesFormat, RedisVSimItem } from '@/types/tauri-specta'
import {
  IPC_WIRE_FORMAT,
  meFormatViewValue,
  meViewToWire,
  type ViewBytesFormat,
} from '@/utils/format'
import { meCommands, meErr } from '@/utils/util'
import { parseVectorInput } from '@/utils/vector'

const { t } = useI18n()
const share = inject(shareProvideKey)!

const visible = ref(false)
const loading = ref(false)
/** ele | values */
const mode = ref<'ele' | 'values'>('ele')
const elementText = ref('')
const vectorText = ref('')
const count = ref(10)
const withAttribs = ref(true)
const filterText = ref('')
const epsilonText = ref('')
const efText = ref('')
const itemList = ref<RedisVSimItem[]>([])
const keyword = ref('')

const dialogTitle = computed(() => t('redisValue.vSimTitle'))

const filteredList = computed(() => {
  const kw = keyword.value.trim().toLowerCase()
  if (!kw) return itemList.value
  return itemList.value.filter(
    item =>
      item.key.toLowerCase().includes(kw) ||
      String(item.score).includes(kw) ||
      (item.attrs || '').toLowerCase().includes(kw),
  )
})

let currentViewFmt: ViewBytesFormat = 'utf8'

async function open(viewFmt: ViewBytesFormat = 'utf8', seed?: { elementDisplay?: string }) {
  const conn = share.conn
  const redisKey = share.redisKey
  if (!conn || !redisKey) return

  currentViewFmt = viewFmt
  visible.value = true
  loading.value = false
  mode.value = 'ele'
  elementText.value = seed?.elementDisplay || ''
  vectorText.value = ''
  count.value = 10
  withAttribs.value = true
  filterText.value = ''
  epsilonText.value = ''
  efText.value = ''
  keyword.value = ''
  itemList.value = []

  if (elementText.value) {
    await query()
  }
}

function parseOptionalFloat(text: string): number | null | undefined {
  const s = text.trim()
  if (!s) return null
  const n = Number(s)
  if (!Number.isFinite(n)) return undefined
  return n
}

function parseOptionalU64(text: string): number | null | undefined {
  const s = text.trim()
  if (!s) return null
  const n = Number(s)
  if (!Number.isFinite(n) || n < 0 || !Number.isInteger(n)) return undefined
  return n
}

async function query() {
  const conn = share.conn
  const redisKey = share.redisKey
  if (!conn || !redisKey) return

  let fieldKey = ''
  let vector: number[] = []
  const valFmt: BytesFormat | null = IPC_WIRE_FORMAT

  if (mode.value === 'ele') {
    const name = elementText.value.trim()
    if (!name) {
      meErr(t('redisValue.vSimElementRequired'))
      return
    }
    fieldKey = meViewToWire(name, currentViewFmt)
  } else {
    const parsed = parseVectorInput(vectorText.value)
    // 空向量 parse 仍 ok；VSIM VALUES 必须非空
    if (!parsed.ok || parsed.nums.length === 0) {
      meErr(t('fieldAdd.vectorInvalid'))
      return
    }
    vector = parsed.nums
  }

  const epsilon = parseOptionalFloat(epsilonText.value)
  if (epsilon === undefined) {
    meErr(t('redisValue.vSimEpsilonInvalid'))
    return
  }
  const ef = parseOptionalU64(efText.value)
  if (ef === undefined) {
    meErr(t('redisValue.vSimEfInvalid'))
    return
  }

  loading.value = true
  try {
    const raw = await meCommands.vSim(conn.id, {
      key: redisKey,
      mode: mode.value,
      fieldKey,
      vector,
      count: count.value,
      withAttribs: withAttribs.value,
      filter: filterText.value.trim(),
      epsilon,
      ef,
      valFmt,
    })
    itemList.value = raw.map(item => ({
      ...item,
      key: meFormatViewValue(item.key, currentViewFmt),
      attrs: item.attrs || '',
    }))
  } finally {
    loading.value = false
  }
}

defineExpose({ open })
</script>

<template>
  <me-dialog :title="dialogTitle" icon="me-icon-rank" v-model="visible" width="820">
    <div v-loading="loading" class="table-vsim">
      <!-- 两行同列：EF 与 COUNT 对齐；ELE/VALUES 主输入同宽 -->
      <div class="vsim-form">
        <el-radio-group v-model="mode" class="c-mode" size="default">
          <el-radio-button value="ele">ELE</el-radio-button>
          <el-radio-button value="values">VALUES</el-radio-button>
        </el-radio-group>
        <el-input
          v-if="mode === 'ele'"
          v-model="elementText"
          class="c-query"
          :placeholder="t('redisValue.element')"
          clearable />
        <el-input
          v-else
          v-model="vectorText"
          class="c-query"
          :placeholder="t('fieldAdd.vectorValueHint')"
          clearable />
        <el-input-number v-model="count" class="c-count" :min="1" :max="10000" />
        <el-checkbox v-model="withAttribs" class="c-attr">WITHATTRIBS</el-checkbox>
        <el-button class="c-go" icon="el-icon-search" type="primary" @click="query">
          {{ t('redisValue.vSimQuery') }}
        </el-button>

        <div class="c-filter-group">
          <el-input
            v-model="filterText"
            class="c-filter"
            :placeholder="t('redisValue.vSimFilterHint')"
            clearable />
          <el-input
            v-model="epsilonText"
            class="c-epsilon"
            :placeholder="t('redisValue.vSimEpsilon')"
            clearable />
        </div>
        <el-input v-model="efText" class="c-ef" :placeholder="t('redisValue.vSimEf')" clearable />
        <el-input
          v-model="keyword"
          class="c-kw"
          :placeholder="t('redisValue.vSimFilter')"
          clearable />
      </div>
      <div class="vsim-main">
        <me-table
          v-if="filteredList.length"
          layout="sizes, prev, pager, next, jumper"
          :data="filteredList"
          export-name="vsim"
          height="100%"
          stripe
          border
          :default-sort="{ prop: 'score', order: 'descending' }">
          <el-table-column type="index" label="#" width="55" align="center" />
          <el-table-column
            :label="t('redisValue.element')"
            prop="key"
            min-width="120"
            show-overflow-tooltip
            sortable />
          <el-table-column
            :label="t('redisValue.score')"
            prop="score"
            min-width="120"
            show-overflow-tooltip
            sortable />
          <el-table-column
            v-if="withAttribs"
            :label="t('fieldSet.attrs')"
            prop="attrs"
            min-width="120"
            show-overflow-tooltip
            sortable />
        </me-table>
        <el-empty v-else-if="!loading" :description="t('redisValue.vSimEmpty')" />
      </div>
    </div>
  </me-dialog>
</template>

<style scoped lang="scss">
.table-vsim {
  height: 100%;
  overflow: hidden;
  display: flex;
  flex-direction: column;

  // 列：模式 | 主输入 | COUNT/EF | WITHATTRIBS | 查询
  .vsim-form {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) 110px auto auto;
    gap: 8px;
    align-items: center;
    margin-bottom: 10px;

    .c-query {
      width: 100%;
      min-width: 0;
    }

    .c-count,
    .c-ef {
      width: 110px;
    }

    // FILTER + EPSILON 占前两列，使 EF 对上 COUNT
    .c-filter-group {
      grid-column: 1 / 3;
      display: flex;
      gap: 8px;
      min-width: 0;

      .c-filter {
        flex: 1;
        min-width: 0;
      }

      .c-epsilon {
        width: 120px;
        flex-shrink: 0;
      }
    }

    .c-ef {
      grid-column: 3;
    }

    .c-kw {
      grid-column: 4 / -1;
      width: 100%;
      min-width: 0;
    }
  }

  .vsim-main {
    flex: 1;
    min-height: 0;
  }
}
</style>
