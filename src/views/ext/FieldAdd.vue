<script setup lang="ts">
import type { FormItemRule } from 'element-plus'
import { cloneDeep } from 'lodash'
import { computed, inject, ref, toRaw, useTemplateRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import { shareProvideKey } from '@/types/me-interface'
import type { RedisFieldAdd_Deserialize, RedisKey_Deserialize } from '@/types/tauri-specta'
import { BYTES_FORMAT, IPC_WIRE_FORMAT, meViewToWire, type ViewBytesFormat } from '@/utils/format'
import { redisKeyWireBase64 } from '@/utils/redis-key'
import {
  KEY_TYPE_LIST,
  meCommands,
  meErr,
  meOk,
  meJsonParse,
  meJsonNormal,
  meTtlSeconds,
  meType,
} from '@/utils/util'

const { t } = useI18n()
const emit = defineEmits(['success', 'closed'])
defineExpose({ open })

function open(data: Partial<RedisFieldAdd_Deserialize & { viewValFmt?: ViewBytesFormat }>) {
  visible.value = true
  Object.assign(form.value, cloneDeep(toRaw(initForm.value)))
  const { viewValFmt, ...rest } = data
  Object.assign(form.value, rest)
  if (viewValFmt) {
    form.value.valFmt = viewValFmt
  }
}

// 共享数据
const share = inject(shareProvideKey)!

// 表单数据
const visible = ref(false)
const isSaving = ref(false)
const initForm = computed(() => ({
  mode: 'key', // key-新增键，field-新增字段
  key: { key: '', bytes: '' } satisfies RedisKey_Deserialize,
  type: 'string',
  ttl: -1,
  value: '',

  streamId: '*', // stream 格式的 id, 默认为*，表示由 redis 生成

  listPushMethod: 'rpush',
  listPushOptions: [
    { label: t('fieldAdd.append'), value: 'rpush' },
    { label: t('fieldAdd.prepend'), value: 'lpush' },
  ],
  /** Array：arset 指定索引 / arinsert 顺序追加 */
  arrayWriteMethod: 'arset',
  arrayWriteOptions: [
    { label: t('fieldAdd.arrayWriteArset'), value: 'arset' },
    { label: t('fieldAdd.arrayWriteArinsert'), value: 'arinsert' },
  ],
  fieldValueList: [{ fieldKey: '', fieldValue: '', fieldScore: 0, fieldTtl: -1 }],
  keyFmt: 'utf8' as ViewBytesFormat,
  valFmt: 'utf8' as ViewBytesFormat,
}))
const form = ref(cloneDeep(toRaw(initForm.value)))

const stringOrJsonType = computed(() => form.value.type === 'string' || form.value.type === 'json')
const jsonType = computed(() => form.value.type === 'json')
const arrayArsetMode = computed(
  () => form.value.type === 'array' && form.value.arrayWriteMethod !== 'arinsert',
)

const rules = computed(() => ({
  'key.key': [{ required: true, message: t('fieldAdd.keyRequired') }],
  type: [{ required: true, message: t('fieldAdd.typeRequired') }],
  ttl: [
    { required: true, message: t('fieldAdd.ttlRequired') },
    {
      validator: (
        _rule: FormItemRule,
        value: unknown,
        callback: (error?: string | Error) => void,
      ) => {
        if (!(form.value.ttl === -1 || form.value.ttl > 0)) {
          callback(new Error(t('fieldAdd.ttlValidator')))
          return
        }
        callback()
      },
    },
  ],
  value: [
    {
      validator: (
        _rule: FormItemRule,
        value: unknown,
        callback: (error?: string | Error) => void,
      ) => {
        // string 等类型允许空串；json 类型空串与非法 JSON 均不通过
        if (form.value.type === 'json') {
          if (value === '') {
            callback(new Error(t('fieldAdd.jsonValidator')))
            return
          }
          try {
            meJsonParse(String(value)) // json 输入支持 json5 格式，此处转换为正常 json 字符串
          } catch {
            callback(new Error(t('fieldAdd.jsonValidator')))
            return
          }
        }
        callback()
      },
    },
  ],
  streamId: [
    {
      validator: (
        _rule: FormItemRule,
        value: unknown,
        callback: (error?: string | Error) => void,
      ) => {
        if (form.value.type === 'stream') {
          if (value) return callback()
          return callback(new Error(t('fieldAdd.streamIdRequired')))
        }
        callback()
      },
    },
  ],
}))

function deleteElement(index: number) {
  form.value.fieldValueList.splice(index, 1)
}

function newElement(index: number) {
  const newValue = { fieldKey: '', fieldValue: '', fieldScore: 0, fieldTtl: -1 }
  form.value.fieldValueList.splice(index + 1, 0, newValue)
}

// 提交数据
const ttlUnit = ref('second')
const formRef = useTemplateRef('formRef')
function submit() {
  formRef.value.validate(async (valid: boolean) => {
    if (!valid) return

    const keyViewFmt = form.value.keyFmt as ViewBytesFormat
    const valViewFmt = form.value.valFmt as ViewBytesFormat
    const isJson = form.value.type === 'json'

    let value = isJson ? meJsonNormal(form.value.value) : form.value.value
    let fieldValueList = form.value.fieldValueList
    let key: RedisKey_Deserialize = form.value.key

    // Array ARSET：索引须为十进制明文，不能走值编码 wire
    if (form.value.type === 'array' && form.value.arrayWriteMethod !== 'arinsert') {
      for (const item of form.value.fieldValueList) {
        const idx = String(item.fieldKey ?? '').trim()
        if (!/^\d+$/.test(idx)) {
          meErr(t('fieldAdd.arrayIndexInvalid'))
          return
        }
      }
    }

    // 与 KeyRename 一致：提交前先做编码转换检查，失败 meErr 并 return，不打后端
    try {
      if (form.value.type === 'string') {
        value = meViewToWire(value, valViewFmt)
      }
      const isArrayArset = form.value.type === 'array' && form.value.arrayWriteMethod !== 'arinsert'
      fieldValueList = form.value.fieldValueList.map(item => ({
        ...item,
        fieldKey: isArrayArset
          ? String(item.fieldKey).trim()
          : meViewToWire(item.fieldKey, valViewFmt),
        fieldValue: meViewToWire(item.fieldValue, valViewFmt),
      }))
      fieldValueList.forEach(item => {
        if (item.fieldTtl === null) item.fieldTtl = -1
      })
      // 新建键按 keyFmt；加字段在 SCAN 省略 bytes 时用展示名转 wire
      if (!form.value.key.bytes) {
        key =
          form.value.mode === 'key'
            ? { key: meViewToWire(form.value.key.key, keyViewFmt), bytes: '' }
            : { key: redisKeyWireBase64(form.value.key), bytes: '' }
      }
    } catch (e) {
      meErr(e instanceof Error ? e.message : String(e))
      return
    }

    isSaving.value = true
    try {
      const redisKey = await meCommands.fieldAdd(share.conn!.id, {
        ...form.value,
        key,
        value,
        ttl: meTtlSeconds(form.value.ttl, ttlUnit.value),
        fieldValueList,
        keyFmt: IPC_WIRE_FORMAT,
        valFmt: isJson ? 'utf8' : IPC_WIRE_FORMAT,
      })
      visible.value = false
      emit('success', redisKey)
      meOk(t('addOk'))
    } finally {
      isSaving.value = false
    }
  })
}

const hint = computed(() => {
  if (form.value.type === 'hash')
    return share.capabilities.httlSupported ? t('fieldAdd.hashHintTtl') : t('fieldAdd.hashHint')
  if (form.value.type === 'zset') return t('fieldAdd.zsetHint')
  if (form.value.type === 'stream') return t('fieldAdd.streamHint')
  if (form.value.type === 'array') {
    return arrayArsetMode.value ? t('fieldAdd.arrayHint') : t('fieldAdd.arrayInsertHint')
  }
  return ''
})

// me-code 的值发生变化时进行自动验证
watch(
  () => form.value.value,
  () => {
    formRef?.value?.validate()
  },
)

// json和stream类型不支持编码
function handleKeyTypeChange() {
  if (jsonType.value) {
    form.value.keyFmt = 'utf8'
    form.value.valFmt = 'utf8'
  }
}
</script>

<template>
  <el-dialog
    :title="form.mode === 'key' ? t('fieldAdd.newKey') : t('fieldAdd.newField')"
    v-model="visible"
    :width="666"
    @closed="emit('closed')"
    destroy-on-close
    :close-on-press-escape="false"
    :close-on-click-modal="false"
    draggable>
    <el-form ref="formRef" :model="form" :rules="rules" label-position="top">
      <!-- 键类型与 TTL: 仅新建键时显示 -->
      <el-row :gutter="20" v-if="form.mode === 'key'">
        <el-col :span="12">
          <el-form-item :label="t('fieldAdd.type')" prop="type">
            <el-select v-model="form.type" style="width: 100%" @change="handleKeyTypeChange">
              <el-option
                v-for="item in KEY_TYPE_LIST"
                :label="item.value"
                :value="item.value.toLowerCase()">
                <el-text :type="item.type">{{ item.value }}</el-text>
              </el-option>

              <template #label="{ label, value }">
                <el-text :type="meType(label)">{{ label }}</el-text>
              </template>
            </el-select>
          </el-form-item>
        </el-col>

        <el-col :span="12">
          <el-form-item :label="t('fieldAdd.ttl')" prop="ttl">
            <el-input v-model.number="form.ttl" style="flex: 1">
              <template #append>
                <el-select v-model="ttlUnit" :style="{ width: t('timeUnit.width') + 'px' }">
                  <el-option :label="t('timeUnit.second', form.ttl)" value="second" />
                  <el-option :label="t('timeUnit.minute', form.ttl)" value="minute" />
                  <el-option :label="t('timeUnit.hour', form.ttl)" value="hour" />
                  <el-option :label="t('timeUnit.day', form.ttl)" value="day" />
                </el-select>
              </template>
            </el-input>
          </el-form-item>
        </el-col>
      </el-row>

      <!-- 键：新建键可编辑，新增字段时禁止编辑且前缀补充类型 -->
      <el-form-item :label="t('fieldAdd.key')" prop="key.key">
        <el-input type="text" v-model="form.key.key" :disabled="form.mode === 'field'">
          <template #prepend v-if="form.mode === 'field'">
            <el-text :type="meType(form.type)">{{ form.type.toUpperCase() }}</el-text>
          </template>
        </el-input>
      </el-form-item>

      <!-- 值：新建键且类型为 string 或 json 时显示 -->
      <el-form-item
        :label="t('fieldAdd.value')"
        prop="value"
        v-if="form.mode === 'key' && stringOrJsonType">
        <me-code v-model="form.value" style="height: 150px; width: 100%" />
      </el-form-item>

      <!-- list 类型的添加方式：rpush、lpush -->
      <el-form-item
        :label="t('fieldAdd.type')"
        v-if="form.mode === 'field' && form.type === 'list'">
        <el-segmented v-model="form.listPushMethod" :options="form.listPushOptions" />
      </el-form-item>

      <!-- Array：ARSET 指定索引 / ARINSERT 顺序追加 -->
      <el-form-item :label="t('fieldAdd.type')" v-if="form.type === 'array'">
        <el-segmented v-model="form.arrayWriteMethod" :options="form.arrayWriteOptions" />
      </el-form-item>

      <!-- streamId: 仅 stream 类型显示 -->
      <el-form-item :label="t('fieldAdd.streamId')" prop="streamId" v-if="form.type === 'stream'">
        <el-input v-model="form.streamId" clearable />
      </el-form-item>

      <!-- key, value, score: 非 string 和 json 类型 -->
      <el-form-item :label="t('fieldAdd.element') + ' ' + hint" v-if="!stringOrJsonType">
        <div
          v-for="(item, index) in form.fieldValueList"
          class="me-flex"
          style="margin-bottom: 10px; width: 100%"
          :key="index">
          <el-input
            type="text"
            v-model="item.fieldKey"
            :placeholder="
              form.type === 'array'
                ? t('fieldAdd.arrayIndex')
                : form.type === 'hash'
                  ? t('fieldAdd.hashKey')
                  : t('fieldAdd.field')
            "
            style="margin-right: 10px"
            v-if="
              form.type === 'hash' ||
              form.type === 'stream' ||
              (form.type === 'array' && arrayArsetMode)
            "
            :validate-event="false" />
          <el-input
            type="text"
            v-model="item.fieldValue"
            :placeholder="t('fieldAdd.value')"
            style="margin-right: 10px"
            :validate-event="false" />
          <el-input-number
            :controls="false"
            v-model="item.fieldScore"
            style="margin-right: 10px"
            v-if="form.type === 'zset'"
            :validate-event="false" />
          <el-input-number
            v-if="form.type === 'hash' && share.capabilities.httlSupported"
            v-model="item.fieldTtl"
            :min="-1"
            :controls="false"
            :placeholder="t('fieldAdd.fieldTtl')"
            style="margin-right: 10px; width: 250px"
            :validate-event="false" />
          <el-button
            icon="el-icon-delete"
            circle
            @click="deleteElement(index)"
            v-if="form.fieldValueList.length > 1" />
          <el-button icon="el-icon-plus" circle @click="newElement(index)" />
        </div>
      </el-form-item>
    </el-form>
    <template #footer>
      <div class="me-flex">
        <div>
          <!-- 键编码：仅新建键时显示 -->
          <el-text v-show="form.mode === 'key'" type="info"> {{ t('fieldAdd.keyCodec') }}</el-text>
          <el-select
            v-show="form.mode === 'key'"
            v-model="form.keyFmt"
            style="width: 100px; margin: 0 20px 0 10px"
            :disabled="jsonType">
            <el-option v-for="item in BYTES_FORMAT" :label="item" :value="item.toLowerCase()" />
          </el-select>

          <!-- 值编码：新建键和新增字段时显示 -->
          <el-text type="info">{{ t('fieldAdd.valueCodec') }}</el-text>
          <el-select
            v-model="form.valFmt"
            style="width: 100px; margin: 0 20px 0 10px"
            :disabled="jsonType">
            <el-option v-for="item in BYTES_FORMAT" :label="item" :value="item.toLowerCase()" />
          </el-select>
        </div>

        <!-- 操作按钮 -->
        <div>
          <el-button @click="visible = false">{{ t('cancel') }}</el-button>
          <el-button type="primary" :loading="isSaving" @click="submit()">{{
            t('save')
          }}</el-button>
        </div>
      </div>
    </template>
  </el-dialog>
</template>

<style scoped lang="scss">
:deep(.el-input-group__prepend) {
  padding: 0 16px;
}
</style>
