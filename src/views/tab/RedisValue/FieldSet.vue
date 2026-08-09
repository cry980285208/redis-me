<script setup lang="ts">
import { cloneDeep } from 'lodash'
import { computed, inject, onUnmounted, ref, useTemplateRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import MeSelectUpDownIcon from '@/components/MeSelectUpDownIcon.vue'
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
import { attrsNormalizedEqual, parseAttrsInput, parseVectorInput } from '@/utils/vector'

/** 含 UI 用 type / wireFieldKey，提交时剔除 */
type FieldSetForm = RedisFieldSet_Deserialize & {
  type: string
  wireFieldKey?: string
  streamId?: string
}

type FieldSetOpen = Partial<FieldSetForm> & {
  /** fieldScan 返回的 wire 形态（恒 base64） */
  keyWireFmt?: BytesFormat
  /** Stream 条目 ID */
  streamId?: string
  /** 查看模式：表单只读，隐藏保存 */
  readonly?: boolean
  /** Vector Set：键的 VDIM（用于维度预检） */
  vectorDim?: number | null
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
const emit = defineEmits<{ success: []; closed: []; refreshed: [data: RedisFieldValue] }>()
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
  vector: [],
}
const form = ref<FieldSetForm>(cloneDeep(initForm))

/** fieldScan 原始 base64 wire，切换字段编码时始终以此为源 */
const srcFieldWire = ref('')
/** Vector Set：键的 VDIM（打开时传入；用于维度预检） */
const expectedVectorDim = ref<number | null>(null)
/** Vector Set：attrs 展示文本（打开时 VGETATTR；不进 RedisFieldSet） */
const attrsText = ref('')
const initialAttrsDisplay = ref('')
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
const vectorsetType = computed(() => form.value.type === 'vectorset')
/** Vector Set 向量/attrs 为 JSON 明文，始终可美化；其它类型随 utf8/strjson */
const prettyEnabled = computed(
  () =>
    vectorsetType.value ||
    effectiveFieldViewFmt.value === 'utf8' ||
    effectiveFieldViewFmt.value === 'strjson',
)
/** JavaSerial / Pickle：不支持写回 → 按钮禁用 + tooltip（连接只读则整钮隐藏，见模板） */
const isViewReadonlyFmt = computed(() => isReadonlyView(effectiveFieldViewFmt.value))
const vectorDirty = computed(() => form.value.fieldValue !== initialFieldDisplay.value)
const attrsDirty = computed(
  () => vectorsetType.value && !attrsNormalizedEqual(attrsText.value, initialAttrsDisplay.value),
)
const fieldDirty = computed(() =>
  vectorsetType.value ? vectorDirty.value || attrsDirty.value : vectorDirty.value,
)
const canSaveField = computed(
  () =>
    !readonly.value &&
    !share.readonly &&
    (vectorsetType.value || (!isViewReadonlyFmt.value && !decodeFailed.value)) &&
    fieldDirty.value,
)
/** 保存钮文案：Vector Set 按脏字段标明范围，避免「保存」歧义 */
const saveFieldLabel = computed(() => {
  if (!vectorsetType.value) return t('save')
  if (vectorDirty.value && attrsDirty.value) return t('fieldSet.saveVectorAndAttrs')
  if (vectorDirty.value) return t('fieldSet.saveVector')
  if (attrsDirty.value) return t('fieldSet.saveAttrs')
  return t('save')
})
/** 禁用原因提示；可保存时与按钮文案一致 */
const saveFieldTip = computed(() => {
  if (!vectorsetType.value && isViewReadonlyFmt.value) {
    return effectiveFieldViewFmt.value === 'pickle'
      ? t('util.pickleReadonly')
      : t('util.javaSerialReadonly')
  }
  if (!vectorsetType.value && decodeFailed.value) return t('util.saveDecodeFailed')
  if (!fieldDirty.value) return t('util.saveNoChange')
  return saveFieldLabel.value
})
/** 显示保存钮：连接只读 / 查看模式 → 隐藏 */
const showSaveField = computed(() => !readonly.value && !share.readonly)
/** hash/list/zset/array 支持 field_get 单行刷新（vectorset 19.1 不做） */
const supportsFieldRefresh = computed(() => {
  const type = form.value.type
  return type === 'hash' || type === 'list' || type === 'zset' || type === 'array'
})

/** wire + 生效 view → 编辑区文本 */
async function syncFieldEditor() {
  // Vector Set：向量为 JSON 明文；attrs 另由 loadVectorAttrs 拉取
  if (vectorsetType.value) {
    form.value.fieldValue = meFormatDisplayValue(srcFieldWire.value, fieldPretty.value)
    initialFieldDisplay.value = form.value.fieldValue
    decodeFailed.value = false
    return
  }
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

async function loadVectorAttrs() {
  attrsText.value = ''
  initialAttrsDisplay.value = ''
  if (!vectorsetType.value) return
  const conn = share.conn
  const wire = form.value.wireFieldKey || ''
  const key = form.value.key
  if (!conn || !wire || !key?.key) return
  try {
    const raw = await meCommands.vGetattr(conn.id, {
      key,
      fieldKey: wire,
      attrs: '',
      valFmt: IPC_WIRE_FORMAT,
    })
    const display = meFormatDisplayValue(raw || '', fieldPretty.value)
    attrsText.value = display
    initialAttrsDisplay.value = display
  } catch {
    attrsText.value = ''
    initialAttrsDisplay.value = ''
  }
}

function open(data: FieldSetOpen) {
  visible.value = true
  readonly.value = !!data.readonly
  expectedVectorDim.value = data.vectorDim ?? null
  Object.assign(form.value, cloneDeep(initForm))
  Object.assign(form.value, data)
  srcFieldWire.value = String(data.srcFieldValue ?? '')
  attrsText.value = ''
  initialAttrsDisplay.value = ''
  // Hash / VectorSet 元素名：wireFieldKey 为 base64；fieldKey 仅展示
  const wireKey = String(data.wireFieldKey || data.fieldKey || '')
  if ((form.value.type === 'hash' || form.value.type === 'vectorset') && wireKey) {
    form.value.wireFieldKey = wireKey
    form.value.fieldKey = meFormatViewValue(wireKey, 'utf8')
  }
  fieldViewFmt.value = 'auto'
  fieldPretty.value = props.pretty
  void (async () => {
    await syncFieldEditor()
    await loadVectorAttrs()
  })()
}

function onFieldViewFmtChange() {
  void syncFieldEditor()
  codeRemountKey.value++
}

function togglePretty() {
  if (!prettyEnabled.value) return
  fieldPretty.value = !fieldPretty.value
  // Vector Set：美化当前编辑区，不回源（避免丢掉未保存的向量/属性）
  if (vectorsetType.value) {
    const vec = parseVectorInput(form.value.fieldValue)
    if (vec.ok) {
      form.value.fieldValue = meFormatDisplayValue(JSON.stringify(vec.nums), fieldPretty.value)
    }
    const attrs = parseAttrsInput(attrsText.value)
    if (attrs.ok) {
      attrsText.value = meFormatDisplayValue(attrs.json, fieldPretty.value)
    }
    codeRemountKey.value++
    return
  }
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
    let vector: number[] = []
    let attrsJson = ''
    // Vector Set：按脏字段分别写 VADD / VSETATTR
    if (vectorsetType.value) {
      if (vectorDirty.value) {
        const parsed = parseVectorInput(form.value.fieldValue)
        if (!parsed.ok) {
          meErr(t('fieldAdd.vectorInvalid'))
          return
        }
        vector = parsed.nums
        // 维度预检：已知 VDIM 时拦截不一致，避免 Redis 服务端报错
        if (expectedVectorDim.value != null && vector.length !== expectedVectorDim.value) {
          meErr(
            t('fieldAdd.vectorDimMismatch', {
              dim: vector.length,
              expected: expectedVectorDim.value,
            }),
          )
          return
        }
        fieldValue = ''
      }
      if (attrsDirty.value) {
        const attrsParsed = parseAttrsInput(attrsText.value)
        if (!attrsParsed.ok) {
          meErr(t('fieldAdd.attrsInvalid'))
          return
        }
        attrsJson = attrsParsed.json
      }
    } else {
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
    }

    // srcFieldValue：Set/ZSet 替换成员时定位用，须与 valFmt 同为 base64
    const srcFieldValue =
      form.value.type === 'zset' || form.value.type === 'set'
        ? srcFieldWire.value
        : form.value.srcFieldValue

    const useWireKey =
      (form.value.type === 'hash' || form.value.type === 'vectorset') && !!wireFieldKey

    isSaving.value = true
    try {
      if (!vectorsetType.value || vectorDirty.value) {
        await meCommands.fieldSet(share.conn!.id, {
          ...rest,
          srcFieldValue,
          fieldKey: useWireKey ? wireFieldKey! : form.value.fieldKey,
          fieldValue,
          vector,
          // Vector Set 元素名仍走 wire；向量走 vector[]
          valFmt: IPC_WIRE_FORMAT,
          includeFieldTtl: form.value.type === 'hash' ? props.hashFieldTtlEnabled : null,
        })
      }
      if (vectorsetType.value && attrsDirty.value && wireFieldKey) {
        await meCommands.vSetattr(share.conn!.id, {
          key: form.value.key,
          fieldKey: wireFieldKey,
          attrs: attrsJson,
          valFmt: IPC_WIRE_FORMAT,
        })
      }
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
  if (type !== 'hash' && type !== 'list' && type !== 'zset' && type !== 'array') return null
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
      <el-form-item :label="t('fieldSet.fieldKey')" v-if="form.type === 'hash'">
        <el-input v-model="form.fieldKey" disabled />
      </el-form-item>
      <el-form-item :label="t('fieldSet.streamId')" v-if="form.type === 'stream'">
        <el-input :model-value="form.streamId || ''" disabled />
      </el-form-item>
      <el-form-item :label="t('fieldSet.element')" v-if="vectorsetType">
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
      <el-form-item
        :label="t('fieldSet.index')"
        v-if="form.type === 'list' || form.type === 'array'">
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
      <el-form-item
        :label="vectorsetType ? t('fieldSet.vector') : t('fieldSet.value')"
        class="field-value-item">
        <me-code
          :key="codeRemountKey"
          v-model="form.fieldValue"
          :read-only="
            editorLoading ||
            readonly ||
            (!vectorsetType && isReadonlyView(effectiveFieldViewFmt)) ||
            decodeFailed
          "
          :error="decodeFailed"
          class="field-code-editor" />
      </el-form-item>
      <!-- Vector Set：打开时自动 VGETATTR；标签/间距与元素、向量一致；空内容保存即删除 -->
      <el-form-item v-if="vectorsetType" :label="t('fieldSet.attrs')" class="field-value-item">
        <me-code
          :key="`attrs-${codeRemountKey}`"
          v-model="attrsText"
          mode="json"
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
            :info="vectorsetType ? t('redisValue.copyVector') : t('redisValue.copyValue')"
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
          <!-- Auto 识别结果：下拉右侧；下拉本身保持 Auto；Vector Set 向量非 wire 不展示编码 -->
          <div v-if="!vectorsetType" class="field-set-enc me-flex">
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
                :disabled="(!vectorsetType && (isViewReadonlyFmt || decodeFailed)) || !fieldDirty"
                @click="submit"
                >{{ saveFieldLabel }}</el-button
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

  /* 与上方元素等表单项间距一致；最后一项贴底供编辑区撑满 */
  .field-value-item {
    display: flex;
    flex-direction: column;
    flex: 1;
    margin-bottom: 18px;
    min-width: 0;
    min-height: 0;
    width: 100%;

    &:last-child {
      margin-bottom: 0;
    }

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
