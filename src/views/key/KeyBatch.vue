<script setup lang="ts">
import { useVirtualList } from '@vueuse/core'
import type { FormItemRule } from 'element-plus'
import { cloneDeep } from 'lodash'
import { computed, inject, ref, useTemplateRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import { shareProvideKey } from '@/types/me-interface'
import type { RedisKey_Deserialize } from '@/types/tauri-specta'
import { clearKeyTypeCacheForConn } from '@/utils/key-type-cache'
import { meCommands, meOk } from '@/utils/util'

/** 打开对话框时合并进表单的字段（与 initForm 一致） */
type KeyBatchForm = {
  match: string
  keyList: RedisKey_Deserialize[]
  deleteDirect: boolean
  file: string
  withTtl: boolean
  /** csv：DUMP 格式；cmd：redis-cli 可执行命令文本 */
  exportFormat: 'csv' | 'cmd'
}

type KeyBatchOpenData = Partial<KeyBatchForm>

const { t } = useI18n()
const emit = defineEmits(['success', 'closed'])

defineExpose({ open })
function open(data: KeyBatchOpenData, mode: string = 'export') {
  visible.value = true
  checkedKeys.value = (data.keyList?.length ?? 0) > 0
  showScan.value = !checkedKeys.value
  batchMode.value = mode
  Object.assign(form.value, cloneDeep(initForm))
  Object.assign(form.value, data)
}

// 共享数据
const share = inject(shareProvideKey)!

// 表单数据
const checkedKeys = ref(false)
const batchMode = ref('export')
const visible = ref(false)
const loading = ref(false)
const initForm: KeyBatchForm = {
  match: '',
  keyList: [],
  deleteDirect: false,
  file: '',
  withTtl: true,
  exportFormat: 'csv',
}

const form = ref<KeyBatchForm>(cloneDeep(initForm))

watch(
  () => form.value.match,
  () => {
    if (!checkedKeys.value) {
      showScan.value = true
      form.value.keyList = []
    }
  },
)

const rules = computed((): Record<string, FormItemRule[]> => {
  const rules: Record<string, FormItemRule[]> = {
    match: [{ required: true, message: t('keyBatch.matchRequired') }],
  }
  if (isExport.value) {
    rules.file = [{ required: true, message: t('keyBatch.exportFileRequired') }]
  }
  return rules
})

// 提交数据
const formRef = useTemplateRef('formRef')
function submit() {
  formRef.value.validate(async (valid: boolean) => {
    if (!valid) return

    loading.value = true
    try {
      if (isExport.value) {
        await meCommands.exportCsv(share.conn!.id, form.value)
      } else {
        await meCommands.batchDel(share.conn!.id, form.value)
        clearKeyTypeCacheForConn(share.conn!.id)
      }
      if (!isExport.value) {
        meOk(t('deleteOk'))
      }
      emit('success', batchMode.value)
      visible.value = false
    } finally {
      loading.value = false
    }
  })
}

// 扫描受影响的键
const showScan = ref(true)
async function scanKey() {
  loading.value = true
  try {
    const params = { match: form.value.match, type: '', count: 0, loadAll: true, cursor: null }
    const data = await meCommands.scan(share.conn!.id, params)
    form.value.keyList = data.keyList
    showScan.value = false
  } finally {
    loading.value = false
  }
}

// 虚拟列表
const items = computed(() => form.value.keyList)
const { list, containerProps, wrapperProps } = useVirtualList(items, { itemHeight: 14 })

// 批量删除和导出数据同时支持
const isExport = computed(() => batchMode.value === 'export')
const title = computed(() => (isExport.value ? t('keyBatch.export') : t('keyBatch.delete')))
const directTip = computed(() =>
  isExport.value ? t('keyBatch.exportDirect') : t('keyBatch.deleteDirect'),
)
const confirmBtn = computed(() =>
  isExport.value ? t('keyBatch.confirmExport') : t('keyBatch.confirmDelete'),
)
const confirmSizeBtn = computed(() => {
  const count = form.value.keyList.length
  return isExport.value
    ? t('keyBatch.confirmExportSize', { count }, count)
    : t('keyBatch.confirmDeleteSize', { count }, count)
})
const exportBtnEnabled = computed(() => (isExport.value ? !!form.value.file : true))

// 导出文件名称添加服务器及版本（不同版本的redisdump命令可能不兼容，便于分析问题）
const exportFilePrefix = computed(
  () =>
    'RedisME_export_' +
    (share.capabilities.isValkey ? 'Valkey' : 'Redis') +
    share.capabilities.version,
)
const exportFormatOptions = computed(() => [
  { label: 'CSV', value: 'csv' as const },
  { label: 'CMD', value: 'cmd' as const },
])
const exportFileSuffix = computed(() => (form.value.exportFormat === 'cmd' ? 'redis' : 'csv'))
const exportFormatTip = computed(() =>
  form.value.exportFormat === 'cmd'
    ? t('keyBatch.exportFormatTipCmd')
    : t('keyBatch.exportFormatTipCsv'),
)
</script>

<template>
  <el-dialog :title v-model="visible" :width="600" @closed="emit('closed')" destroy-on-close>
    <el-form ref="formRef" :model="form" :rules="rules" label-position="top">
      <el-form-item :label="t('keyBatch.match')" prop="match" v-if="!checkedKeys">
        <!-- 此处保留可编辑，使用更加方便 -->
        <el-input type="text" v-model="form.match" />
        <el-checkbox v-model="form.deleteDirect" v-if="form.keyList.length === 0">{{
          directTip
        }}</el-checkbox>
      </el-form-item>

      <el-row :span="24" v-if="isExport">
        <el-col :span="12">
          <el-form-item :label="t('keyBatch.ttl')">
            <el-checkbox v-model="form.withTtl">{{ t('keyBatch.expireTip') }}</el-checkbox>
          </el-form-item>
        </el-col>
        <el-col :span="12">
          <el-form-item :label="t('keyBatch.exportFormat')">
            <el-segmented v-model="form.exportFormat" :options="exportFormatOptions" />
          </el-form-item>
        </el-col>
      </el-row>
      <el-text v-if="isExport" type="info" class="export-format-tip">{{ exportFormatTip }}</el-text>

      <el-form-item :label="t('keyBatch.exportFile')" v-if="isExport" prop="file">
        <me-file-input
          v-model="form.file"
          :placeholder="t('keyBatch.exportFileTip')"
          :file-prefix="exportFilePrefix"
          :file-suffix="exportFileSuffix" />
      </el-form-item>

      <el-form-item
        :label="t('keyBatch.impactKeys', { count: form.keyList.length }, form.keyList.length)"
        v-if="!showScan">
        <div v-bind="containerProps" :style="{ height: '150px', width: '100%' }">
          <div v-bind="wrapperProps">
            <div v-for="item in list" :key="item.index" class="key single-line-ellipsis">
              {{ item.data.key }}
            </div>
          </div>
        </div>
      </el-form-item>
    </el-form>

    <template #footer>
      <el-button @click="visible = false">{{ t('cancel') }}</el-button>
      <el-button
        type="primary"
        :loading="loading"
        @click="submit"
        v-if="form.deleteDirect"
        :disabled="!exportBtnEnabled"
        >{{ confirmBtn }}</el-button
      >
      <template v-else>
        <el-button type="primary" :loading="loading" @click="scanKey" v-if="showScan">{{
          t('keyBatch.showImpactKeys')
        }}</el-button>
        <el-button
          type="primary"
          :loading="loading"
          @click="submit"
          v-else
          :disabled="!(form.keyList.length > 0 && exportBtnEnabled)">
          {{ confirmSizeBtn }}</el-button
        >
      </template>
    </template>
  </el-dialog>
</template>

<style scoped lang="scss">
.key {
  font-size: 14px;
  line-height: 14px;
  padding: 3px 4px;
  color: var(--el-color-info);
}

.export-format-tip {
  display: block;
  margin: -8px 0 12px;
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-line;
}
</style>
