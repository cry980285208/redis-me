<script setup lang="ts">
import { cloneDeep } from 'lodash'
import { computed, inject, onUnmounted, ref, useTemplateRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import { shareProvideKey } from '@/types/me-interface'
import type {
  BytesFormat,
  RedisFieldAsCommand_Deserialize,
  RedisFieldGet_Deserialize,
  RedisFieldSet_Deserialize,
  RedisFieldValue,
} from '@/types/tauri-specta'
import {
  customFormatName,
  defaultFieldViewFmt,
  fieldViewOptions,
  isCustomView,
  isViewDecodeError,
  meFormatViewValue,
  meFormatViewValueAsync,
  meViewToWire,
  meViewToWireAsync,
  needsJsonNormalize,
  toWireFormat,
  viewFmtForField,
  type ViewBytesFormat,
} from '@/utils/format'
import {
  meCommands,
  meCopy,
  meErr,
  meFormatDisplayValue,
  meJsonNormal,
  meOk,
  meWarn,
} from '@/utils/util'

/** 含 UI 用 type / wireFieldKey，提交时剔除 */
type FieldSetForm = RedisFieldSet_Deserialize & { type: string; wireFieldKey?: string }

type FieldSetOpen = Partial<FieldSetForm> & {
  /** fieldScan 返回的 wire 形态 */
  keyWireFmt?: BytesFormat
  /** 键级数据编码，用于默认字段 view */
  keyViewFmt?: ViewBytesFormat
  /** Stream 条目 ID（复制为命令等） */
  streamId?: string
  /** 查看模式：表单只读，隐藏保存 */
  readonly?: boolean
}

const props = withDefaults(
  defineProps<{
    /** 与 RedisValue 值区美化开关一致，open 时同步为初始状态 */
    pretty?: boolean
  }>(),
  { pretty: true },
)

const { t } = useI18n()
const emit = defineEmits(['success', 'closed', 'refreshed'])
defineExpose({ open, close })

const share = inject(shareProvideKey)!

const visible = ref(false)
const readonly = ref(false)
const isSaving = ref(false)
const initForm: FieldSetForm = {
  key: { key: '', bytes: '' },
  type: 'string',
  srcFieldValue: '',
  fieldIndex: 0,
  fieldKey: '',
  fieldValue: '',
  fieldScore: 0,
  fieldTtl: -1,
  valFmt: 'utf8',
}
const form = ref<FieldSetForm>(cloneDeep(initForm))

/** fieldScan 原始 wire，切换字段编码时始终以此为源 */
const srcFieldWire = ref('')
const fieldStreamId = ref('')
const keyWireFmt = ref<BytesFormat>('utf8')
/** 键级 view 编码，field_get 与值页表格刷新一致 */
const keyViewFmt = ref<ViewBytesFormat>('utf8')
const fieldViewFmt = ref<ViewBytesFormat>('utf8')
const fieldPretty = ref(true)
const editorLoading = ref(false)
const isRefreshing = ref(false)
const decodeFailed = ref(false)
const codeRemountKey = ref(0)

const customNames = computed(() => (window.meTauri.settings.customCodecs ?? []).map(f => f.name))
const fieldViewOptionList = computed(() => fieldViewOptions(keyWireFmt.value, customNames.value))
const prettyEnabled = computed(
  () => fieldViewFmt.value === 'utf8' || fieldViewFmt.value === 'strjson',
)
/** hash/list/zset 支持 field_get 单行刷新 */
const supportsFieldRefresh = computed(() => {
  const type = form.value.type
  return type === 'hash' || type === 'list' || type === 'zset'
})

/** wire + 字段 view → 编辑区文本 */
async function syncFieldEditor() {
  const wire = srcFieldWire.value
  const fmt = fieldViewFmt.value
  if (!wire) {
    form.value.fieldValue = ''
    decodeFailed.value = false
    return
  }
  if (!fieldPretty.value && fmt === 'strjson') {
    form.value.fieldValue = wire
    decodeFailed.value = false
    return
  }
  editorLoading.value = true
  try {
    if (isCustomView(fmt)) {
      form.value.fieldValue = await meFormatViewValueAsync(wire, fmt)
    } else if (fmt === 'utf8') {
      form.value.fieldValue = meFormatDisplayValue(wire, fieldPretty.value)
    } else {
      form.value.fieldValue = meFormatViewValue(wire, fmt)
    }
    decodeFailed.value = isViewDecodeError(form.value.fieldValue)
  } catch (e) {
    form.value.fieldValue = e instanceof Error ? e.message : String(e)
    decodeFailed.value = true
  } finally {
    editorLoading.value = false
  }
}

function open(data: FieldSetOpen) {
  visible.value = true
  readonly.value = !!data.readonly
  Object.assign(form.value, cloneDeep(initForm))
  Object.assign(form.value, data)
  srcFieldWire.value = String(data.srcFieldValue ?? '')
  fieldStreamId.value = data.streamId ?? ''
  keyWireFmt.value = data.keyWireFmt ?? 'utf8'
  keyViewFmt.value = data.keyViewFmt ?? 'utf8'
  fieldViewFmt.value = defaultFieldViewFmt(data.keyViewFmt ?? 'utf8', keyWireFmt.value)
  fieldPretty.value = props.pretty
  void syncFieldEditor()
}

function onFieldViewFmtChange() {
  void syncFieldEditor()
  codeRemountKey.value++
}

function togglePretty() {
  if (!prettyEnabled.value) return
  fieldPretty.value = !fieldPretty.value
  void syncFieldEditor()
  codeRemountKey.value++
}

function close() {
  visible.value = false
}

/** 自定义编解码被删后，当前字段 view 失效则回退 */
watch(customNames, names => {
  if (!visible.value || !isCustomView(fieldViewFmt.value)) return
  const name = customFormatName(fieldViewFmt.value)
  if (!name || !names.includes(name)) {
    fieldViewFmt.value = defaultFieldViewFmt('utf8', keyWireFmt.value)
    void syncFieldEditor()
  }
})

const rules = computed(() => ({
  fieldValue: [{ required: true, message: t('fieldSet.fieldValueRequired') }],
  fieldScore: [{ required: true, message: t('fieldSet.fieldScoreRequired') }],
}))

function cancel() {
  visible.value = false
  emit('closed')
}

function onEscapeKey(e: KeyboardEvent) {
  if (!visible.value || e.key !== 'Escape') return
  e.stopPropagation()
  cancel()
}

watch(visible, val => {
  if (val) window.addEventListener('keydown', onEscapeKey, true)
  else window.removeEventListener('keydown', onEscapeKey, true)
})
onUnmounted(() => window.removeEventListener('keydown', onEscapeKey, true))

const formRef = useTemplateRef('formRef')
function submit() {
  formRef.value.validate(async (valid: boolean) => {
    if (!valid) return

    isSaving.value = true
    try {
      const { type: _type, wireFieldKey, ...rest } = form.value
      const fmt = fieldViewFmt.value
      let fieldValue = form.value.fieldValue
      if (needsJsonNormalize(fmt)) {
        fieldValue = meJsonNormal(fieldValue)
      }
      if (isCustomView(fmt)) {
        fieldValue = await meViewToWireAsync(fieldValue, fmt)
      } else {
        fieldValue = meViewToWire(fieldValue, fmt)
      }
      await meCommands.fieldSet(share.conn!.id, {
        ...rest,
        fieldKey: form.value.type === 'hash' && wireFieldKey ? wireFieldKey : form.value.fieldKey,
        fieldValue,
        valFmt: toWireFormat(fmt),
      })
      visible.value = false
      emit('success')
      meOk(t('editOk'))
    } catch (e) {
      meErr(e instanceof Error ? e.message : String(e))
    } finally {
      isSaving.value = false
    }
  })
}

function buildFieldAsCommandParam(): RedisFieldAsCommand_Deserialize | null {
  if (!share.conn || !form.value.key?.key) return null
  const type = form.value.type
  const fmt = fieldViewFmt.value
  const param: RedisFieldAsCommand_Deserialize = {
    key: form.value.key,
    fieldKey:
      type === 'hash' && form.value.wireFieldKey ? form.value.wireFieldKey : form.value.fieldKey,
    fieldIndex: form.value.fieldIndex,
    fieldValue: type === 'zset' || type === 'set' ? srcFieldWire.value : '',
    streamId: fieldStreamId.value,
    valFmt: toWireFormat(fmt),
  }
  if (type === 'stream') param.fieldValue = ''
  return param
}

async function copyFieldAsCommand() {
  const conn = share.conn
  const param = buildFieldAsCommandParam()
  if (!conn || !param) return
  const text = await meCommands.getFieldAsCommand(conn.id, param)
  if (!text.trim()) {
    meWarn(t('redisValue.copyCommandEmpty'))
    return
  }
  meCopy(text, t('redisValue.copyCommandOk'))
}

function buildFieldGetParam(): RedisFieldGet_Deserialize | null {
  if (!form.value.key?.key) return null
  const type = form.value.type
  if (type !== 'hash' && type !== 'list' && type !== 'zset') return null
  return {
    key: form.value.key,
    fieldIndex: form.value.fieldIndex,
    fieldKey:
      type === 'hash' && form.value.wireFieldKey ? form.value.wireFieldKey : form.value.fieldKey,
    fieldValue: type === 'zset' ? srcFieldWire.value : '',
    valFmt: toWireFormat(viewFmtForField(keyViewFmt.value)),
  }
}

function applyFieldGetToForm(data: RedisFieldValue) {
  const type = form.value.type
  srcFieldWire.value = data.fieldValue
  if (type === 'hash') {
    form.value.fieldKey = data.fieldKey
    form.value.fieldTtl = data.fieldTtl
  } else if (type === 'zset' && data.fieldScore != null) {
    form.value.fieldScore = data.fieldScore
  }
}

async function refreshField() {
  const conn = share.conn
  const param = buildFieldGetParam()
  if (!conn || !param || isRefreshing.value) return
  isRefreshing.value = true
  try {
    const data = await meCommands.fieldGet(conn.id, param, false)
    applyFieldGetToForm(data)
    await syncFieldEditor()
    codeRemountKey.value++
    emit('refreshed', data)
    meOk(t('redisValue.refreshFieldRowOk'))
  } catch (e) {
    meErr(e instanceof Error ? e.message : String(e))
  } finally {
    isRefreshing.value = false
  }
}
</script>

<template>
  <el-card
    :header="readonly ? t('fieldSet.viewField') : t('fieldSet.editField')"
    v-show="visible"
    class="field-set">
    <el-form ref="formRef" class="field-set-form" :model="form" :rules="rules" label-position="top">
      <el-form-item :label="t('fieldSet.hashKey')" v-if="form.type === 'hash'">
        <el-input v-model="form.fieldKey" disabled />
      </el-form-item>
      <el-form-item
        :label="t('fieldSet.fieldTtl')"
        v-if="form.type === 'hash' && share.capabilities.httlSupported">
        <el-input-number
          v-model="form.fieldTtl"
          :min="-1"
          :controls="false"
          :disabled="readonly"
          style="width: 100%"
          align="left" />
      </el-form-item>
      <el-form-item :label="t('fieldSet.index')" v-if="form.type === 'list'">
        <el-input v-model="form.fieldIndex" disabled />
      </el-form-item>
      <el-form-item :label="t('fieldSet.score')" prop="fieldScore" v-if="form.type === 'zset'">
        <el-input-number
          :controls="false"
          v-model="form.fieldScore"
          :disabled="readonly"
          align="left"
          style="width: 100%" />
      </el-form-item>
      <el-form-item :label="t('fieldSet.value')" prop="fieldValue" class="field-value-item">
        <me-code
          :key="codeRemountKey"
          v-model="form.fieldValue"
          :read-only="editorLoading || readonly"
          class="field-code-editor" />
      </el-form-item>
    </el-form>
    <template #footer>
      <div class="field-set-footer me-flex">
        <div class="field-set-footer-left">
          <me-icon
            placement="top-start"
            :info="t('fieldSet.prettyHint')"
            class="icon-btn"
            :style="{
              opacity: prettyEnabled && fieldPretty ? 1 : 0.2,
              cursor: prettyEnabled ? 'pointer' : 'default',
            }"
            icon="el-icon-magic-stick"
            @click="togglePretty" />
          <me-icon
            placement="top-start"
            :info="t('redisValue.copyFieldValue')"
            class="icon-btn"
            style="font-size: 18px; margin-left: 5px"
            icon="el-icon-document-copy"
            @click="meCopy(form.fieldValue)" />
          <me-icon
            placement="top-start"
            :info="t('redisValue.copyAsCommand')"
            class="icon-btn"
            style="font-size: 18px; margin-left: 5px"
            icon="me-icon-copy-command"
            @click="copyFieldAsCommand" />
          <me-icon
            v-if="supportsFieldRefresh"
            placement="top-start"
            :info="t('redisValue.refreshFieldRow')"
            class="icon-btn"
            style="font-size: 18px; margin-left: 5px"
            icon="el-icon-refresh-right"
            :style="{ opacity: isRefreshing ? 0.5 : 1, cursor: isRefreshing ? 'wait' : 'pointer' }"
            @click="refreshField" />
          <el-select
            v-model="fieldViewFmt"
            size="small"
            class="field-set-enc-select"
            :disabled="editorLoading"
            @change="onFieldViewFmtChange">
            <el-option
              v-for="item in fieldViewOptionList"
              :key="item.value"
              :label="item.label"
              :value="item.value" />
          </el-select>
        </div>
        <div>
          <el-button @click="cancel">{{ t('cancel') }}</el-button>
          <el-button
            v-if="!readonly"
            type="primary"
            :loading="isSaving"
            :disabled="decodeFailed"
            @click="submit"
            >{{ t('save') }}</el-button
          >
        </div>
      </div>
    </template>
  </el-card>
</template>

<style scoped lang="scss">
.field-set {
  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
  overflow: hidden;

  :deep(.el-card__body) {
    padding: 20px 20px 0 20px;
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  :deep(.el-card__footer) {
    border-top: none;
    flex-shrink: 0;
  }

  .field-set-form {
    display: flex;
    flex-direction: column;
    flex: 1;
    height: 100%;
    min-height: 0;
  }

  .field-set-footer {
    align-items: center;
  }

  .field-set-footer-left {
    display: flex;
    align-items: center;
    font-size: 20px;
  }

  .field-set-enc-select {
    width: 100px;
    margin-left: 12px;
    font-size: var(--el-font-size-base);

    :deep(.el-select__wrapper) {
      min-height: 28px;
    }
  }

  .field-value-item {
    display: flex;
    flex-direction: column;
    flex: 1;
    margin-bottom: 0;
    min-width: 0;
    min-height: 0;
    width: 100%;

    :deep(.el-form-item__content) {
      display: flex;
      flex-direction: column;
      flex: 1;
      min-height: 0;
      min-width: 0;
      width: 100%;
      overflow: hidden;
    }
  }

  .field-code-editor {
    flex: 1;
    width: 100%;
    max-width: 100%;
    height: 100%;
    min-width: 0;
    min-height: 0;
    overflow: hidden;

    :deep(.cm-editor) {
      width: 100%;
      max-width: 100%;
      height: 100%;
    }

    :deep(.cm-scroller) {
      overflow: auto;
    }
  }
}
</style>
