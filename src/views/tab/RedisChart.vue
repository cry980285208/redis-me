<script setup lang="ts">
// #region 导入
import {
  CategoryScale,
  Chart as ChartJS,
  Legend,
  LinearScale,
  LineController,
  LineElement,
  PointElement,
  TimeScale,
  Tooltip,
} from 'chart.js'
import type { ChartOptions, TooltipItem } from 'chart.js'
import dayjs from 'dayjs'
import { cloneDeep, merge } from 'lodash'
import {
  computed,
  inject,
  nextTick,
  onMounted,
  onUnmounted,
  ref,
  shallowRef,
  unref,
  useTemplateRef,
  watch,
} from 'vue'
import { Line } from 'vue-chartjs'
import 'chartjs-adapter-dayjs-4/dist/chartjs-adapter-dayjs-4.esm'
import { useI18n } from 'vue-i18n'

import { shareProvideKey } from '@/types/me-interface'
import type { RedisChart } from '@/types/tauri-specta'
import { isDark, meHumanNums, meCommands, meLog, PREDEFINE_COLORS } from '@/utils/util'
import NodeList from '@/views/ext/NodeList.vue'
// #endregion

// #region Chart.js 初始化
// 只注册必要的组件即可
// https://chartjs.cn/docs/latest/getting-started/integration.html
ChartJS.register(
  LineController,
  LineElement,
  PointElement,
  TimeScale,
  LinearScale,
  Legend,
  CategoryScale,
  Tooltip,
)
// #endregion

// #region 核心状态
const { t } = useI18n()

const share = inject(shareProvideKey)!
const node = ref('') // 指定节点
const autoRefresh = ref(true) // 自动刷新
const refreshInterval = ref(5) // 刷新间隔（秒）
const keepMinutes = ref(60) // 保留N分钟的数据
const maxPointCount = ref(100) // 最多保存N个数据
const nowPointCount = ref(0) // 当前数据条数
const showMoreChart = ref(false) // 显示更多图表

// 与 chartData 各块 key 一致
type ChartBlockKey =
  | 'command'
  | 'memory'
  | 'network'
  | 'keyTotal'
  | 'connectedClients'
  | 'cacheHitRatio'
  | 'totalConnectionsReceived'
  | 'totalCommandsProcessed'

// 自动刷新及刷新间隔配置
let timer: ReturnType<typeof setInterval> | null = null
onUnmounted(() => {
  if (timer) clearInterval(timer)
})
watch(
  [autoRefresh, refreshInterval],
  ([val, _]: [boolean, number]) => {
    if (timer) clearInterval(timer)
    timer = null
    if (val) {
      timer = setInterval(getData, refreshInterval.value * 1000)
    }
  },
  { immediate: true },
)
// #endregion

// #region 图表操作
// 图表实例，手动刷新
// vue-chartjs Line 经 reforwardRef 暴露的 chart（可能是 Ref，需 unref）
type LineChartExposed = { chart: unknown }
const commandRef = useTemplateRef<LineChartExposed>('command')
const memoryRef = useTemplateRef<LineChartExposed>('memory')
const networkRef = useTemplateRef<LineChartExposed>('network')
const keyTotalRef = useTemplateRef<LineChartExposed>('keyTotal')
const connectedClientsRef = useTemplateRef<LineChartExposed>('connectedClients')
const cacheHitRatioRef = useTemplateRef<LineChartExposed>('cacheHitRatio')
const totalConnectionsReceivedRef = useTemplateRef<LineChartExposed>('totalConnectionsReceived')
const totalCommandsProcessedRef = useTemplateRef<LineChartExposed>('totalCommandsProcessed')
const chartsRoot = useTemplateRef<HTMLElement>('charts')

function updateLineChart(lineRef: LineChartExposed | null | undefined) {
  const chart = unref(lineRef?.chart) as { update?: () => void } | null | undefined
  chart?.update?.()
}

function refreshInstance() {
  updateLineChart(commandRef.value)
  updateLineChart(memoryRef.value)
  updateLineChart(networkRef.value)

  updateLineChart(keyTotalRef.value)
  updateLineChart(connectedClientsRef.value)
  updateLineChart(cacheHitRatioRef.value)
  updateLineChart(totalConnectionsReceivedRef.value)
  updateLineChart(totalCommandsProcessedRef.value)
}

let chartsFullscreenWasOurs = false

async function toggleChartsFullscreen() {
  const el = chartsRoot.value
  if (!el) return
  try {
    if (document.fullscreenElement === el) {
      await document.exitFullscreen()
    } else {
      await el.requestFullscreen()
    }
  } catch (e: unknown) {
    meLog('charts fullscreen error', e)
  }
}

function onChartsFullscreenKey(e: KeyboardEvent) {
  if (e.key !== 'F11') return
  e.preventDefault()
  void toggleChartsFullscreen()
}

function onChartsFullscreenChange() {
  const el = chartsRoot.value
  if (!el) return
  const now = document.fullscreenElement === el
  if (now || chartsFullscreenWasOurs) {
    chartsFullscreenWasOurs = now
    refreshInstance()
  }
}

onMounted(() => {
  window.addEventListener('keydown', onChartsFullscreenKey, true)
  document.addEventListener('fullscreenchange', onChartsFullscreenChange)
  // 等 Line 子组件 onMounted 创建 chart 后再拉数并 refresh（setup 阶段 chart 仍为 null）
  void nextTick(() => {
    if (!share.conn?.cluster) {
      void getData()
    }
  })
})
onUnmounted(() => {
  window.removeEventListener('keydown', onChartsFullscreenKey, true)
  document.removeEventListener('fullscreenchange', onChartsFullscreenChange)
  if (chartsRoot.value && document.fullscreenElement === chartsRoot.value) {
    void document.exitFullscreen()
  }
})

// 从后台获取原始数据
async function getData() {
  try {
    const res = await meCommands.chart(share.conn!.id, node.value)
    const label = Date.now()
    addChartData(label, res, 'command', 'instantaneousOpsPerSec')
    addChartData(label, res, 'memory', 'usedMemory')
    addChartData(label, res, 'network', 'instantaneousInputKbps', 'instantaneousOutputKbps')

    addChartData(label, res, 'keyTotal', 'keyTotal')
    addChartData(label, res, 'connectedClients', 'connectedClients')
    addChartData(label, res, 'cacheHitRatio', 'cacheHitRatio')
    addChartData(label, res, 'totalConnectionsReceived', 'totalConnectionsReceived')
    addChartData(label, res, 'totalCommandsProcessed', 'totalCommandsProcessed')

    // 保存当前数据条数, 超过最大条数，则均分取样
    nowPointCount.value = chartData.value.command.labels.length
    if (nowPointCount.value > maxPointCount.value) {
      const indexes = calcLabelIndexes(chartData.value.command.labels)
      cutChartData(indexes, 'command')
      cutChartData(indexes, 'memory')
      cutChartData(indexes, 'network')
      cutChartData(indexes, 'keyTotal')
      cutChartData(indexes, 'connectedClients')
      cutChartData(indexes, 'cacheHitRatio')
      cutChartData(indexes, 'totalConnectionsReceived')
      cutChartData(indexes, 'totalCommandsProcessed')
    }
    // shallowRef 原地 push 不会触发 vue-chartjs，克隆后 :data 引用变化才能重绘
    chartData.value = cloneDeep(chartData.value)
    nowPointCount.value = chartData.value.command.labels.length
  } catch (e: unknown) {
    meLog('get chart data error', e)
  }
}

// 添加图表数据
function chartNumber(res: RedisChart, key: keyof RedisChart): number {
  const v = res[key]
  return typeof v === 'number' ? v : 0
}

function addChartData(
  label: number,
  res: RedisChart,
  prop0: ChartBlockKey,
  prop1: keyof RedisChart,
  prop2?: keyof RedisChart,
) {
  const propData = chartData.value[prop0]
  ;(propData.labels as number[]).push(label)
  const s0 = propData.datasets[0].data as number[]
  s0.push(chartNumber(res, prop1))
  if (prop2) {
    const s1 = propData.datasets[1].data as number[]
    s1.push(chartNumber(res, prop2))
  }
}

// 裁剪图表数据
function cutChartData(indexes: number[], prop0: ChartBlockKey) {
  const propData = chartData.value[prop0]
  propData.labels = cutArray(propData.labels, indexes)
  propData.datasets[0].data = cutArray(propData.datasets[0].data, indexes)
  if (propData.datasets[1]) {
    propData.datasets[1].data = cutArray(propData.datasets[1].data, indexes)
  }
}

// 裁剪数组
function cutArray<T>(arr: T[], indexes: number[]): T[] {
  const newArr: T[] = []
  for (let i = 0; i < arr.length; i++) {
    if (indexes.includes(i)) {
      newArr.push(arr[i])
    }
  }
  return newArr
}

// 计算需要保留的索引位置，用于裁剪判断
function calcLabelIndexes(labels: number[]) {
  const minLabel = Date.now() - keepMinutes.value * 60 * 1000

  // 最大值减去最小值，除以 maxPointCount 获得刻度间隔
  const interval =
    Math.floor((labels[labels.length - 1] - Math.max(labels[0], minLabel)) / maxPointCount.value) *
    1.5

  const indexes: number[] = []
  let intervalLabel: number | undefined = undefined
  for (let i = 0; i < labels.length; i++) {
    const label = labels[i]

    // 超过保留时间的数据跳过
    if (label < minLabel) {
      continue
    }

    // 第1个保留的标签
    if (intervalLabel === undefined) {
      intervalLabel = label
      continue
    }

    if (label - intervalLabel >= interval) {
      indexes.push(i)
      intervalLabel = label
    }
  }
  return indexes
}
// #endregion

// #region 图表配置
// chart.js配置项（基本配置项）
const baseOptions = {
  maintainAspectRatio: false,
  // x轴为日期格式，显示为秒
  scales: {
    x: {
      type: 'time' as const,
      time: { unit: 'second' as const, displayFormats: { second: 'HH:mm:ss' } },
      // 控制刻度显示数量
      ticks: {
        align: 'center' as const, // 刻度居中
        maxTicksLimit: 10, // 最多显示N个主刻度
        autoSkip: true, // 自动跳过部分刻度
      },
      bounds: 'ticks' as const, // 确保坐标轴从第一个刻度开始，到最后一个刻度结束
      offset: false, // 在两端留出空白
    },
  },
  plugins: {
    tooltip: {
      callbacks: {
        // 格式化工具提示的标题（时间）
        title: function (context: TooltipItem<'line'>[]) {
          const first = context[0]
          if (!first) return ''
          const dt = dayjs(first.parsed.x)
          return dt.format('YYYY-MM-DD HH:mm:ss')
        },
      },
    },
  },
} satisfies ChartOptions<'line'>

// 浅色深色主题
const lightOptions = {
  color: '#333',
  scales: {
    x: {
      ticks: {
        color: '#666', // 默认的X轴标签颜色
      },
      grid: {
        color: 'rgba(0, 0, 0, 0.1)', // 默认的X轴网格线颜色
      },
    },
    y: {
      ticks: {
        color: '#666', // 默认的Y轴标签颜色
      },
      grid: {
        color: 'rgba(0, 0, 0, 0.1)', // 默认的Y轴网格线颜色
      },
    },
  },
}

const darkOptions = {
  color: '#EEE',
  scales: {
    x: {
      ticks: {
        color: '#EEE', // 默认的X轴标签颜色
      },
      grid: {
        color: 'rgba(255, 255, 255, 0.2)', // 默认的X轴网格线颜色
      },
    },
    y: {
      ticks: {
        color: '#EEE', // 默认的Y轴标签颜色
      },
      grid: {
        color: 'rgba(255, 255, 255, 0.2)', // 默认的Y轴网格线颜色
      },
    },
  },
}

const options = computed((): ChartOptions<'line'> => {
  const option = cloneDeep(baseOptions) as ChartOptions<'line'>
  return (
    isDark.value ? merge(option, darkOptions) : merge(option, lightOptions)
  ) as ChartOptions<'line'>
})

const memoryOptions = computed((): ChartOptions<'line'> => {
  return merge(cloneDeep(options.value), {
    scales: {
      y: {
        beginAtZero: true, // 从零开始
        ticks: {
          // 关键：修改 Y 轴刻度的显示单位
          callback: function (this: unknown, value: string | number) {
            const valueInMB = Number(value) / (1024 * 1024) // 字节转 MB
            return valueInMB.toFixed(1) + ' M' // 保留 1 位小数
          },
        },
      },
    },
  }) as ChartOptions<'line'>
})

const ratioOptions = computed((): ChartOptions<'line'> => {
  return merge(cloneDeep(options.value), {
    scales: {
      y: {
        min: 0, // 从零开始
        max: 1,
      },
    },
  }) as ChartOptions<'line'>
})

const totalOptions = computed((): ChartOptions<'line'> => {
  return merge(cloneDeep(options.value), {
    scales: {
      y: {
        ticks: {
          // 关键：修改 Y 轴刻度的显示单位
          callback: function (this: unknown, value: string | number) {
            return meHumanNums(Number(value), '0', 2)
          },
        },
      },
    },
  }) as ChartOptions<'line'>
})

// chart.js数据配置项
const dataset = {
  data: [] as number[],
  tension: 0.4, // 线条张力、平滑度
}

const initData = computed(() => ({
  // 命令执行数/秒
  command: {
    labels: [],
    datasets: [
      { label: t('redisChart.command'), borderColor: PREDEFINE_COLORS[0], ...cloneDeep(dataset) },
    ],
  },
  // 已使用内存
  memory: {
    labels: [],
    datasets: [
      { label: t('redisChart.memory'), borderColor: PREDEFINE_COLORS[1], ...cloneDeep(dataset) },
    ],
  },
  // 网络输入输出
  network: {
    labels: [],
    datasets: [
      { label: t('redisChart.networkIn'), borderColor: PREDEFINE_COLORS[2], ...cloneDeep(dataset) },
      {
        label: t('redisChart.networkOut'),
        borderColor: PREDEFINE_COLORS[4],
        ...cloneDeep(dataset),
      },
    ],
  },

  // 键数量
  keyTotal: {
    labels: [],
    datasets: [
      { label: t('redisChart.keyTotal'), borderColor: PREDEFINE_COLORS[0], ...cloneDeep(dataset) },
    ],
  },
  // 客户端连接数
  connectedClients: {
    labels: [],
    datasets: [
      {
        label: t('redisChart.connectedClients'),
        borderColor: PREDEFINE_COLORS[1],
        ...cloneDeep(dataset),
      },
    ],
  },
  // 缓存命中率
  cacheHitRatio: {
    labels: [],
    datasets: [
      {
        label: t('redisChart.cacheHitRatio'),
        borderColor: PREDEFINE_COLORS[2],
        ...cloneDeep(dataset),
      },
    ],
  },
  // 服务器接受的总连接数
  totalConnectionsReceived: {
    labels: [],
    datasets: [
      {
        label: t('redisChart.totalConnectionsReceived'),
        borderColor: PREDEFINE_COLORS[3],
        ...cloneDeep(dataset),
      },
    ],
  },
  // 服务器处理的总命令数
  totalCommandsProcessed: {
    labels: [],
    datasets: [
      {
        label: t('redisChart.totalCommandsProcessed'),
        borderColor: PREDEFINE_COLORS[4],
        ...cloneDeep(dataset),
      },
    ],
  },
}))

// 避免vue的响应式更新和chartjs的响应式更新冲突（无限循环），使用shallowRef
let chartData = shallowRef(cloneDeep(initData.value))

// 重置数据
function resetData() {
  chartData.value = cloneDeep(initData.value)
  getData()
}
watch(node, resetData)

// 语言切换
watch(
  () => meTauri.settings.language,
  () => {
    chartData.value.command.datasets[0].label = t('redisChart.command')
    chartData.value.memory.datasets[0].label = t('redisChart.memory')
    chartData.value.network.datasets[0].label = t('redisChart.networkIn')
    chartData.value.network.datasets[1].label = t('redisChart.networkOut')

    chartData.value.keyTotal.datasets[0].label = t('redisChart.keyTotal')
    chartData.value.connectedClients.datasets[0].label = t('redisChart.connectedClients')
    chartData.value.cacheHitRatio.datasets[0].label = t('redisChart.cacheHitRatio')
    chartData.value.totalConnectionsReceived.datasets[0].label = t(
      'redisChart.totalConnectionsReceived',
    )
    chartData.value.totalCommandsProcessed.datasets[0].label = t(
      'redisChart.totalCommandsProcessed',
    )
    chartData.value = cloneDeep(chartData.value) // 直接更新时label并没有更新，因此克隆1份，让vue进行重新渲染
  },
  { immediate: true },
)

// 主题切换
watch(
  () => meTauri.settings.theme,
  () => refreshInstance(),
)
// #endregion
</script>

<template>
  <div class="redis-chart">
    <div class="me-flex">
      <div class="left">
        <me-button @click="resetData" icon="el-icon-delete" info="清空数据" placement="top" />
        <el-dropdown placement="bottom-start" :hide-on-click="false" :teleported="false">
          <me-icon
            class="refresh icon-btn"
            :class="autoRefresh ? 'rotating' : ''"
            icon="el-icon-refresh-right"
            @click="getData"
            style="margin-left: 20px" />
          <template #dropdown>
            <el-dropdown-menu>
              <el-form :label-width="t('redisChart.labelWidth')" label-position="right">
                <el-dropdown-item>
                  <el-form-item :label="t('redisChart.autoRefresh')">
                    <el-switch v-model="autoRefresh" style="margin-left: 10px" />
                  </el-form-item>
                </el-dropdown-item>
                <el-dropdown-item>
                  <el-form-item :label="t('redisChart.refreshInterval')">
                    <el-input-number
                      v-model.number="refreshInterval"
                      :min="1"
                      :max="60"
                      :controls="false"
                      style="width: 80px; margin-left: 10px">
                      <template #suffix>{{ t('redisChart.refreshUnit') }}</template>
                    </el-input-number>
                  </el-form-item>
                </el-dropdown-item>
                <el-dropdown-item>
                  <el-form-item :label="t('redisChart.keepMinutes')">
                    <el-input-number
                      v-model.number="keepMinutes"
                      :min="1"
                      :max="600"
                      :controls="false"
                      style="width: 80px; margin-left: 10px">
                      <template #suffix>{{ t('redisChart.keepUnit') }}</template>
                    </el-input-number>
                  </el-form-item>
                </el-dropdown-item>
                <el-dropdown-item>
                  <el-form-item :label="t('redisChart.maxPointCount')">
                    <el-input-number
                      v-model.number="maxPointCount"
                      :min="10"
                      :max="200"
                      :controls="false"
                      style="width: 80px; margin-left: 10px">
                      <template #suffix>{{ t('redisChart.pointUnit') }}</template>
                    </el-input-number>
                  </el-form-item>
                </el-dropdown-item>

                <el-dropdown-item>
                  <el-form-item :label="t('redisChart.moreChart')">
                    <el-switch v-model="showMoreChart" style="margin-left: 10px" />
                  </el-form-item>
                </el-dropdown-item>
              </el-form>
            </el-dropdown-menu>
          </template>
        </el-dropdown>
      </div>
      <div class="right">
        <el-text type="info">[{{ nowPointCount }}]</el-text>
        <node-list v-model="node" style="margin-left: 10px" init-node />
      </div>
    </div>

    <div ref="charts" class="charts">
      <div class="chart"><Line ref="command" :data="chartData.command" :options /></div>
      <div class="chart">
        <Line ref="memory" :data="chartData.memory" :options="memoryOptions" />
      </div>
      <div class="chart"><Line ref="network" :data="chartData.network" :options /></div>

      <template v-if="showMoreChart">
        <div class="chart"><Line ref="keyTotal" :data="chartData.keyTotal" :options /></div>
        <div class="chart">
          <Line ref="connectedClients" :data="chartData.connectedClients" :options />
        </div>
        <div class="chart">
          <Line ref="cacheHitRatio" :data="chartData.cacheHitRatio" :options="ratioOptions" />
        </div>
        <div class="chart">
          <Line
            ref="totalConnectionsReceived"
            :data="chartData.totalConnectionsReceived"
            :options="totalOptions" />
        </div>
        <div class="chart">
          <Line
            ref="totalCommandsProcessed"
            :data="chartData.totalCommandsProcessed"
            :options="totalOptions" />
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped lang="scss">
.redis-chart {
  height: 100%;
  overflow-y: auto;
  display: flex;
  flex-direction: column;

  .left {
    display: flex;
  }

  :deep(.el-input-group__prepend) {
    padding: 0 10px 0 0;
  }

  :deep(.el-input-group__append) {
    width: 42px;
  }

  :deep(.el-form-item) {
    margin-bottom: 0;
  }

  .refresh {
    font-size: 24px;
    color: var(--el-color-success);
  }

  .charts {
    margin-top: -20px;
    flex-grow: 1;

    &:fullscreen {
      margin-top: 0;
      box-sizing: border-box;
      width: 100%;
      height: 100%;
      overflow-y: auto;
      /* 与 Tab 内容区一致：深色主题下未覆写 --el-bg-color-page，会与主背景脱节 */
      background-color: var(--el-bg-color);
    }

    .chart {
      height: min(28vh, 33%);
      padding: 10px;
    }
  }

  @keyframes rotate {
    from {
      transform: rotate(0deg);
    }
    to {
      transform: rotate(360deg);
    }
  }

  .rotating {
    animation: rotate 2s linear infinite;
  }
}
</style>
