<script setup lang="ts">
import { sortBy } from 'lodash'
import {
  computed,
  inject,
  nextTick,
  onMounted,
  reactive,
  ref,
  useTemplateRef,
  watch,
  watchEffect,
} from 'vue'
import { useI18n } from 'vue-i18n'

import MeWebsite from '@/components/MeWebsite.vue'
import { configTip as tips } from '@/locales/config'
import { redisConfDict, valkeyConfDict } from '@/locales/config/defaults'
import { shareProvideKey } from '@/types/me-interface'
import { meCopy, meCommands, meOk } from '@/utils/util'
import NodeList from '@/views/ext/NodeList.vue'

const { t } = useI18n()
// 共享数据
const share = inject(shareProvideKey)!
const canEdit = computed(() => !share.readonly)
const props = defineProps({
  initNode: { type: String, default: '' },
  initVersion: { type: String, default: '' },
})

const node = ref(props.initNode)
watch(
  () => props.initNode,
  v => {
    node.value = v
  },
)
const keyword = ref('')
const loading = ref(false)
interface ConfigTableRow {
  param: string
  value: string
}
const dataList = ref<ConfigTableRow[]>([])

// 文件格式的配置文件（使用 Vite ?raw 导入，打包时会内联到 JS 中）
const serverType = computed(() => (share.capabilities.isValkey ? 'Valkey' : 'Redis'))

// 动态加载配置文件
const configCache: Record<string, string | null> = {}
async function loadConfigFile(version: string) {
  if (configCache[version]) return configCache[version]

  try {
    const { default: content } = await import(`../../assets/conf/${version}.conf?raw`)
    configCache[version] = content
    return content
  } catch (e: unknown) {
    console.error(`加载配置文件失败: ${version}`, e)
    return null
  }
}

// 配置文件列表（手动维护，与 src/assets/conf/ 目录保持一致）
const allConfigVersions = [
  'Redis4.0',
  'Redis5.0',
  'Redis6.2',
  'Redis7.0',
  'Redis7.2',
  'Redis7.4',
  'Redis8.0',
  'Redis8.2',
  'Redis8.4',
  'Redis8.6',
  'Redis8.8',
  'Valkey7.2',
  'Valkey8.0',
  'Valkey8.1',
  'Valkey9.0',
]

const dirConfigList = ref<string[]>([])
onMounted(() => {
  dirConfigList.value = [...allConfigVersions].sort().reverse()
})

const configVersionList = computed(() =>
  dirConfigList.value.filter(d => d.startsWith(serverType.value)),
)
const configVersion = ref('') // 版本
const configRaw = ref('')

// 读取配置文件的值
watchEffect(async () => {
  try {
    if (!configVersion.value) {
      configRaw.value = t('redisConfig.noConfig')
      return
    }
    const content = await loadConfigFile(configVersion.value)
    configRaw.value = content || t('redisConfig.noConfig')
  } catch (_e: unknown) {
    configRaw.value = t('redisConfig.noConfig')
  }
})

function handleCommand(command: string) {
  configVersion.value = command
  nextTick(() => (dialog.raw = true))
}

// Json格式的默认配置
const confDict = computed(() => (share.capabilities.isValkey ? valkeyConfDict : redisConfDict))

const dictVersionList = Object.keys(confDict.value).reverse()
function getDefaultVersion() {
  for (const version of dictVersionList) {
    if (serverType.value + props.initVersion > version) {
      return version
    }
  }
  return configVersionList.value[0] ?? ''
}
const dictVersion = ref(getDefaultVersion())
const dictRaw = computed(
  () => confDict.value[dictVersion.value] as Record<string, string | undefined>,
)
const tipMap = computed(() => tips.value as Record<string, string | undefined>)
const showTypeOptions = [
  { label: t('redisConfig.all'), value: 'All' },
  { label: t('redisConfig.diff'), value: 'Diff' },
]
const showType = ref('All')

const filterDataList = computed(() => {
  const key = keyword.value.toLowerCase()
  return dataList.value.filter(
    row =>
      (!key ||
        row.param?.toLowerCase().indexOf(key) > -1 ||
        row.value?.toLowerCase().indexOf(key) > -1 ||
        (tipMap.value[row.param]?.toLowerCase() ?? '').indexOf(key) > -1) &&
      (showType.value === 'All' ||
        (showType.value === 'Diff' && row.value !== dictRaw.value[row.param])),
  )
})

// 合计列
// function getSummaries() {
//   return [t('redisConfig.total'), filterDataList.value.length + ' / ' + dataList.value.length, '']
// }

async function apiConfigGet() {
  const data = await meCommands.configGet(share.conn!.id, '*', node.value)
  const tableData: ConfigTableRow[] = []
  Object.entries(data).forEach(([key, value]) => tableData.push({ param: key, value }))
  dataList.value = sortBy(tableData, ['param'])
}

async function refresh() {
  loading.value = true
  try {
    await apiConfigGet()
  } finally {
    loading.value = false
  }
}
refresh()

// 官网默认配置参考
const dialog = reactive({ raw: false })

// 行样式展示
function calcRowStyle({ row }: { row: ConfigTableRow }) {
  return { color: row.value === dictRaw.value[row.param] ? '' : share.color }
}

// 设置参数
const formRef = useTemplateRef('formRef')
const editLoading = ref(false)
const editShow = ref(false)
const form = reactive({ param: '', value: '', autoBroadcast: true })
const command = computed(
  () =>
    `CONFIG SET ${form.param} ${form.value?.includes(' ') ? '"' + form.value + '"' : form.value}`,
)
async function editConfig(row: ConfigTableRow) {
  form.param = row.param
  form.value = row.value
  await nextTick(() => {
    editShow.value = true
  })
}
async function configSet() {
  formRef.value.validate(async (valid: boolean) => {
    if (!valid) return

    editLoading.value = true
    try {
      await meCommands.configSet(
        share.conn!.id,
        form.param,
        form.value,
        form.autoBroadcast ? '*' : node.value,
      )
      meOk(t('saveOk'))
      await refresh()
      editShow.value = false
    } finally {
      editLoading.value = false
    }
  })
}

const rules = computed(() => ({
  value: [{ required: true, message: t('redisConfig.valueRequired') }],
}))
</script>

<template>
  <div class="redis-config">
    <div class="me-flex header">
      <div>
        <div class="me-flex">
          <node-list v-model="node" style="margin-right: 10px" @change="refresh" />
          <el-dropdown @command="handleCommand">
            <el-button plain icon="el-icon-notebook">{{ t('redisConfig.reference') }}</el-button>
            <template #dropdown>
              <el-dropdown-menu>
                <el-dropdown-item :command="item" v-for="item in configVersionList">{{
                  item
                }}</el-dropdown-item>
              </el-dropdown-menu>
            </template>
          </el-dropdown>
          <me-website to="config" />
        </div>
      </div>
      <div class="me-flex">
        <el-segmented v-model="showType" :options="showTypeOptions"></el-segmented>
        <el-select v-model="dictVersion" style="width: 120px; margin: 0 10px">
          <el-option v-for="item in dictVersionList" :key="item" :label="item" :value="item" />
        </el-select>
        <el-input
          v-model="keyword"
          :placeholder="t('redisConfig.keyword')"
          style="width: 250px; margin-right: 10px"
          clearable />
        <el-button icon="el-icon-search" @click="refresh" type="primary" :loading="loading" />
      </div>
    </div>

    <div class="table">
      <me-table
        :data="filterDataList"
        ref="table"
        v-loading="loading"
        :row-style="calcRowStyle"
        export-name="config">
        <el-table-column
          :label="t('redisConfig.param')"
          prop="param"
          sortable
          show-overflow-tooltip />
        <el-table-column :label="t('redisConfig.value')" prop="value" show-overflow-tooltip />
        <el-table-column
          :label="dictVersion + ' ' + t('redisConfig.defaultConfig')"
          prop="value"
          show-overflow-tooltip>
          <template #default="scope">
            {{ dictRaw[scope.row.param] }}
          </template>
        </el-table-column>
        <el-table-column :label="t('redisConfig.tip')" show-overflow-tooltip>
          <template #default="scope">
            <span style="color: var(--el-color-info)">{{ tipMap[scope.row.param] }}</span>
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
              :info="t('redisConfig.configSet')"
              icon="el-icon-edit"
              class="icon-btn"
              @click="editConfig(scope.row)"
              style="justify-content: center" />
          </template>
        </el-table-column>
      </me-table>
    </div>

    <me-dialog
      icon="me-icon-redis"
      :title="`${configVersion} ${t('redisConfig.defaultConfig')}`"
      v-model="dialog.raw"
      width="60vw">
      <me-code :modelValue="configRaw" mode="properties" read-only />
    </me-dialog>

    <el-dialog :title="t('redisConfig.configSet')" v-model="editShow" align-center>
      <el-form ref="formRef" :model="form" :rules="rules" label-width="80px">
        <el-form-item :label="t('redisConfig.param')">
          <el-input v-model="form.param" disabled />
        </el-form-item>
        <el-form-item :label="t('redisConfig.value')" prop="value">
          <div class="me-flex" style="width: 100%">
            <el-input
              v-model="form.value"
              :placeholder="t('redisConfig.value')"
              clearable
              style="flex: 1" />
            <el-tooltip
              :content="t('redisConfig.autoBroadcastTip')"
              placement="top-end"
              :show-after="1000">
              <el-checkbox
                v-model="form.autoBroadcast"
                :label="t('redisConfig.autoBroadcast')"
                style="margin-left: 20px"
                v-if="share.conn?.cluster" />
            </el-tooltip>
          </div>
        </el-form-item>
        <el-form-item :label="t('redisConfig.command')">
          <el-text :style="{ color: share.color }" @click="meCopy(command)">
            {{ command }}
          </el-text>
        </el-form-item>
      </el-form>

      <template #footer>
        <el-button @click="editShow = false">{{ t('cancel') }}</el-button>
        <el-button type="primary" :loading="editLoading" @click="configSet">{{
          t('save')
        }}</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<style scoped lang="scss">
.redis-config {
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
