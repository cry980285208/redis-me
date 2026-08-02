<script setup lang="ts">
import { cloneDeep } from 'lodash'
import { computed, inject, onUnmounted, ref, useTemplateRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import { MeSelectUpDownIcon } from '@/components/MeSelectUpDownIcon'
import { shareProvideKey } from '@/types/me-interface'
import type {
  BytesFormat,
  RedisFieldGet_Deserialize,
  RedisFieldSet_Deserialize,
  RedisFieldValue,
} from '@/types/tauri-specta'
import { detectViewFormat, detectedViewLabel } from '@/utils/detect-view-format'
import {
  IPC_WIRE_FORMAT,
  base64WireToUtf8Display,
  customFormatName,
  fieldViewOptions,
  isCustomView,
  isReadonlyView,
  isViewDecodeError,
  meFormatViewValue,
  meFormatViewValueAsync,
  meViewToWire,
  meViewToWireAsync,
  needsJsonNormalize,
  type ViewBytesFormat,
} from '@/utils/format'
import { meCommands, meCopy, meErr, meFormatDisplayValue, meJsonNormal, meOk } from '@/utils/util'

/** 含 UI 用 type / wireFieldKey，提交时剔除 */
type FieldSetForm = RedisFieldSet_Deserialize & { type: string; wireFieldKey?: string }

type FieldSetOpen = Partial<FieldSetForm> & {
  /** fieldScan 返回的 wire 形态（恒 base64） */
  keyWireFmt?: BytesFormat
  /** Stream 条目 ID */
  streamId?: string
  /** 查看模式：表单只读，隐藏保存 */
  readonly?: boolean
}

const props = withDefaults(
  defineProps<{
    /** 与 RedisValue 值区美化开关一致，open 时同步为初始状态 */
    pretty?: boolean
    /** 与值页 HTTL 开关一致；关则隐藏 TTL 展示/编辑，保存时由后端保留原有过期 */
    hashFieldTtlEnabled?: boolean
  }>(),
  { pretty: true, hashFieldTtlEnabled: false },
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
  includeFieldTtl: false,
  valFmt: IPC_WIRE_FORMAT,
}
const form = ref<FieldSetForm>(cloneDeep(initForm))

/** fieldScan 原始 base64 wire，切换字段编码时始终以此为源 */
const srcFieldWire = ref('')
/** 下拉选中项；默认 Auto，与 STRING 键级一致 */
const fieldViewFmt = ref<ViewBytesFormat>('auto')
const fieldPretty = ref(true)
const editorLoading = ref(false)
const isRefreshing = ref(false)
const decodeFailed = ref(false)
const codeRemountKey = ref(0)
/** syncFieldEditor 完成后的展示快照，用于脏检查 */
const initialFieldDisplay = ref('')

const customNames = computed(() => (window.meTauri.settings.customCodecs ?? []).map(f => f.name))
const fieldViewOptionList = computed(() => fieldViewOptions(customNames.value))
/** Auto 识别结果；非 Auto 时不展示旁侧标签 */
const detectedView = computed(() => detectViewFormat(srcFieldWire.value))
/** Auto 时为识别结果，否则等于下拉选中项；驱动展示 / 保存 / 只读 */
const effectiveFieldViewFmt = computed<ViewBytesFormat>(() =>
  fieldViewFmt.value === 'auto' ? detectedView.value : fieldViewFmt.value,
)
const detectedViewText = computed(() =>
  fieldViewFmt.value === 'auto' ? detectedViewLabel(detectedView.value) : '',
)
const prettyEnabled = computed(
  () => effectiveFieldViewFmt.value === 'utf8' || effectiveFieldViewFmt.value === 'strjson',
)
/** JavaSerial / Pickle：不支持写回 → 按钮禁用 + tooltip（连接只读则整钮隐藏，见模板） */
const isViewReadonlyFmt = computed(() => isReadonlyView(effectiveFieldViewFmt.value))
const fieldDirty = computed(() => form.value.fieldValue !== initialFieldDisplay.value)
const canSaveField = computed(
  () =>
    !readonly.value &&
    !share.readonly &&
    !isViewReadonlyFmt.value &&
    !decodeFailed.value &&
    fieldDirty.value,
)
/** 禁用原因提示；可保存时为普通「保存」 */
const saveFieldTip = computed(() => {
  if (isViewReadonlyFmt.value) {
    return effectiveFieldViewFmt.value === 'pickle'
      ? t('util.pickleReadonly')
      : t('util.javaSerialReadonly')
  }
  if (decodeFailed.value) return t('util.saveDecodeFailed')
  if (!fieldDirty.value) return t('util.saveNoChange')
  return t('save')
})
/** 显示保存钮：连接只读 / 查看模式 → 隐藏 */
const showSaveField = computed(() => !readonly.value && !share.readonly)
/** hash/list/zset 支持 field_get 单行刷新 */
const supportsFieldRefresh = computed(() => {
  const type = form.value.type
  return type === 'hash' || type === 'list' || type === 'zset'
})

/** wire + 生效 view → 编辑区文本 */
async function syncFieldEditor() {
  const wire = srcFieldWire.value
  const fmt = effectiveFieldViewFmt.value
  if (!wire) {
    form.value.fieldValue = ''
    initialFieldDisplay.value = ''
    decodeFailed.value = false
    return
  }
  if (!fieldPretty.value && fmt === 'strjson') {
    form.value.fieldValue = base64WireToUtf8Display(wire)
    initialFieldDisplay.value = form.value.fieldValue
    decodeFailed.value = false
    return
  }
  editorLoading.value = true
  try {
    if (isCustomView(fmt)) {
      form.value.fieldValue = await meFormatViewValueAsync(wire, fmt)
    } else if (fmt === 'utf8') {
      form.value.fieldValue = meFormatDisplayValue(
        meFormatViewValue(wire, 'utf8'),
        fieldPretty.value,
      )
    } else {
      form.value.fieldValue = meFormatViewValue(wire, fmt)
    }
    decodeFailed.value = isViewDecodeError(form.value.fieldValue)
    initialFieldDisplay.value = form.value.fieldValue
  } catch (e) {
    form.value.fieldValue = e instanceof Error ? e.message : String(e)
    initialFieldDisplay.value = form.value.fieldValue
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
  // Hash 字段名：wireFieldKey 为 base64；fieldKey 仅展示
  const wireKey = String(data.wireFieldKey || data.fieldKey || '')
  if (form.value.type === 'hash' && wireKey) {
    form.value.wireFieldKey = wireKey
    form.value.fieldKey = meFormatViewValue(wireKey, 'utf8')
  }
  fieldViewFmt.value = 'auto'
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

/** 自定义编解码被删后，当前字段 view 失效则回退 Auto */
watch(customNames, names => {
  if (!visible.value || !isCustomView(fieldViewFmt.value)) return
  const name = customFormatName(fieldViewFmt.value)
  if (!name || !names.includes(name)) {
    fieldViewFmt.value = 'auto'
    void syncFieldEditor()
  }
})

const rules = computed(() => ({
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
  if (!canSaveField.value) return
  formRef.value.validate(async (valid: boolean) => {
    if (!valid) return

    const { type: _type, wireFieldKey, ...rest } = form.value
    const fmt = effectiveFieldViewFmt.value
    let fieldValue = form.value.fieldValue
    // 与 KeyRename / FieldAdd 一致：提交前先编码检查，失败 meErr 并 return
    try {
      if (needsJsonNormalize(fmt)) {
        fieldValue = fieldValue === '' ? '' : meJsonNormal(fieldValue)
      }
      if (isCustomView(fmt)) {
        fieldValue = await meViewToWireAsync(fieldValue, fmt)
      } else {
        fieldValue = meViewToWire(fieldValue, fmt)
      }
    } catch (e) {
      meErr(e instanceof Error ? e.message : String(e))
      return
    }

    // srcFieldValue：Set/ZSet 替换成员时定位用，须与 valFmt 同为 base64
    const srcFieldValue =
      form.value.type === 'zset' || form.value.type === 'set'
        ? srcFieldWire.value
        : form.value.srcFieldValue

    isSaving.value = true
    try {
      await meCommands.fieldSet(share.conn!.id, {
        ...rest,
        srcFieldValue,
        fieldKey: form.value.type === 'hash' && wireFieldKey ? wireFieldKey : form.value.fieldKey,
        fieldValue,
        valFmt: IPC_WIRE_FORMAT,
        includeFieldTtl: form.value.type === 'hash' ? props.hashFieldTtlEnabled : null,
      })
      visible.value = false
      emit('success')
      meOk(t('editOk'))
    } finally {
      isSaving.value = false
    }
  })
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
    valFmt: IPC_WIRE_FORMAT,
    includeFieldTtl: type === 'hash' ? props.hashFieldTtlEnabled : null,
  }
}

function applyFieldGetToForm(data: RedisFieldValue) {
  const type = form.value.type
  srcFieldWire.value = data.fieldValue
  if (type === 'hash') {
    form.value.wireFieldKey = data.fieldKey
    form.value.fieldKey = meFormatViewValue(data.fieldKey, 'utf8')
    if (props.hashFieldTtlEnabled) {
      form.value.fieldTtl = data.fieldTtl
    }
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
        v-if="form.type === 'hash' && share.capabilities.httlSupported && hashFieldTtlEnabled">
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
      <el-form-item :label="t('fieldSet.value')" class="field-value-item">
        <me-code
          :key="codeRemountKey"
          v-model="form.fieldValue"
          :read-only="
            editorLoading || readonly || isReadonlyView(effectiveFieldViewFmt) || decodeFailed
          "
          :error="decodeFailed"
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
            :info="t('redisValue.copyValue')"
            class="icon-btn"
            style="font-size: 18px; margin-left: 5px"
            icon="el-icon-document-copy"
            @click="meCopy(form.fieldValue)" />
          <me-icon
            v-if="supportsFieldRefresh"
            placement="top-start"
            :info="t('redisValue.refreshFieldRow')"
            class="icon-btn"
            style="font-size: 18px; margin-left: 5px"
            icon="el-icon-refresh-right"
            :style="{ opacity: isRefreshing ? 0.5 : 1, cursor: isRefreshing ? 'wait' : 'pointer' }"
            @click="refreshField" />
          <!-- Auto 识别结果：下拉右侧；下拉本身保持 Auto -->
          <div class="field-set-enc me-flex">
            <el-select
              v-model="fieldViewFmt"
              class="field-set-enc-select me-select-plain"
              :suffix-icon="MeSelectUpDownIcon"
              :disabled="editorLoading"
              @change="onFieldViewFmtChange">
              <el-option
                v-for="item in fieldViewOptionList"
                :key="item.value"
                :label="item.label"
                :value="item.value" />
            </el-select>
            <el-text
              v-if="detectedViewText"
              class="field-set-auto-label"
              :title="t('redisValue.autoDetected')">
              {{ detectedViewText }}
            </el-text>
          </div>
        </div>
        <div>
          <el-button @click="cancel">{{ t('cancel') }}</el-button>
          <!-- 连接只读/查看模式：隐藏；禁用时 tooltip 说明原因 -->
          <el-tooltip v-if="showSaveField" :content="saveFieldTip" placement="top">
            <span style="margin-left: 12px; display: inline-block">
              <el-button
                type="primary"
                :loading="isSaving"
                :disabled="isViewReadonlyFmt || decodeFailed || !fieldDirty"
                @click="submit"
                >{{ t('save') }}</el-button
              >
            </span>
          </el-tooltip>
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

  .field-set-enc {
    align-items: center;
    // margin-left: 12px;
  }

  .field-set-auto-label {
    margin-left: 2px;
    white-space: nowrap;
    color: var(--el-color-primary);
    font-weight: 600;
  }

  .field-set-enc-select {
    font-size: var(--el-font-size-base);

    :deep(.el-select__wrapper) {
      min-height: 0;
      height: 30px;
      padding: 4px;
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
