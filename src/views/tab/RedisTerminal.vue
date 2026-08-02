<script setup lang="ts">
import { computed, inject, nextTick, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import MeIcon from '@/components/MeIcon.vue'
import { MeSelectUpDownIcon } from '@/components/MeSelectUpDownIcon'
import MeShortcut from '@/components/MeShortcut.vue'
import { commandHelp, isReadonlyCommand } from '@/locales/cmd'
import { shareProvideKey } from '@/types/me-interface'
import type { CliOutputMode } from '@/types/tauri-specta'
import { getTerminalShortcuts } from '@/utils/shortcut'
import { meCopy, meCommands, isZh } from '@/utils/util'

import CommandHelp from '../ext/CommandHelp.vue'
import NodeList from '../ext/NodeList.vue'

const { t } = useI18n()
// 共享数据
const share = inject(shareProvideKey)!
const canEdit = computed(() => !share.readonly)
/** 只读列表头：英文 Read-only 较宽，中文只读可窄一些 */
const readonlyColWidth = computed(() => (isZh.value ? 88 : 120))

/** 终端输出格式（对齐 redis-cli --raw / --json / --csv；每次进入默认 TTY） */
const outputModeOptions: { value: CliOutputMode; labelKey: string }[] = [
  { value: 'standard', labelKey: 'redisTerminal.outputStandard' },
  { value: 'raw', labelKey: 'redisTerminal.outputRaw' },
  { value: 'json', labelKey: 'redisTerminal.outputJson' },
  { value: 'csv', labelKey: 'redisTerminal.outputCsv' },
]
const outputMode = ref<CliOutputMode>('standard')

// 待颜色的文本
function colorText(color: string, text: string, bold = false): string {
  return bold
    ? `<span style="color: ${color}; font-weight: bold">${text}</span>`
    : `<span style="color: ${color}">${text}</span>`
}

const autoBroadcast = ref(true)
const node = ref('')
const prefix = computed(() => (node.value ? node.value + '> ' : '$ '))
const welcome = computed(() =>
  t('redisTerminal.welcome', { RedisME: colorText(share.color, 'RedisME', true) }),
)

// 成功输出统一绿色；raw 空结果保留空行占位
function formatCommandResult(data: string): string {
  if (outputMode.value === 'raw' && data === '') {
    return colorText('var(--el-color-success)', '<br/>')
  }
  const html = data.split(/\r?\n/).join('<br/>')
  return colorText('var(--el-color-success)', html)
}

// 定制化执行命令
async function execCommand(command: string): Promise<string> {
  if (!canEdit.value && !isReadonlyCommand(command)) {
    return colorText('var(--el-color-warning)', t('redisTerminal.readonlyWriteHint'))
  }

  try {
    const param = {
      command,
      node: node.value,
      autoBroadcast: autoBroadcast.value,
      outputMode: outputMode.value,
    }
    const data = await meCommands.executeCommand(share.conn!.id, param, false)
    autoCopyIfNeed(data)
    return formatCommandResult(data)
  } catch (e: unknown) {
    autoCopyIfNeed(e)
    return colorText('var(--el-color-error)', `(error) ${String(e)}`)
  }
}

// 自动复制命令结果
const autoCopy = ref(false)
function autoCopyIfNeed(text: unknown) {
  if (autoCopy.value) {
    meCopy(String(text), undefined, false)
  }
}

// 命令提示的中英文实时切换
// 说明: vue-web-terminal的命令只能初始化1次, 后续更新无效。因此考虑销毁重建
const showCode = ref(true)
watch(commandHelp, () => {
  showCode.value = false
  nextTick(() => {
    showCode.value = true
  })
})

// 命令帮助弹窗
const commandHelpRef = ref<InstanceType<typeof CommandHelp>>()
function openCommandDialog() {
  commandHelpRef.value?.open()
}

const keyShortVisible = ref(false)
function openKeyShortDialog() {
  keyShortVisible.value = true
}

const keyShortcuts = computed(() => getTerminalShortcuts(t))
</script>

<template>
  <div class="redis-terminal">
    <!-- 命令输入 -->
    <me-xterm
      v-if="showCode"
      class="terminal"
      :exec-command="execCommand"
      :prefix
      :welcome
      :command-help="commandHelp" />

    <!-- 集群节点 -->
    <div class="node me-flex" v-if="share.conn?.cluster">
      <el-tooltip raw-content :content="t('redisTerminal.broadcastHint')" placement="top">
        <el-checkbox
          v-model="autoBroadcast"
          :label="t('redisTerminal.autoBroadcast')"
          style="margin-left: 10px" />
      </el-tooltip>
      <node-list v-model="node" clearable style="margin-left: 10px" />
    </div>

    <!-- 工具栏 -->
    <div class="tool me-flex">
      <el-select
        v-model="outputMode"
        class="output-mode-select me-select-plain"
        :suffix-icon="MeSelectUpDownIcon">
        <el-option
          v-for="item in outputModeOptions"
          :key="item.value"
          :label="t(item.labelKey)"
          :value="item.value" />
      </el-select>
      <el-tooltip :content="t('redisTerminal.autoCopyHint')" placement="top-end">
        <el-checkbox v-model="autoCopy" style="margin-left: 10px" />
      </el-tooltip>
      <me-icon
        class="icon-btn"
        icon="me-icon-keyshort"
        :info="t('redisTerminal.keyShortHint')"
        placement="top-end"
        @click="openKeyShortDialog"
        style="margin-left: 10px; font-size: 20px" />
      <me-icon
        class="icon-btn"
        icon="el-icon-help"
        :info="t('redisTerminal.commandHint')"
        placement="top-end"
        @click="openCommandDialog"
        style="margin-left: 5px" />
    </div>

    <!-- 快捷键提示 -->
    <el-dialog
      v-model="keyShortVisible"
      width="400"
      align-center
      draggable
      :show-close="false"
      header-class="me-shortcut-dialog__header">
      <div class="terminal-shortcut-title">{{ t('setting.shortcutTerminal') }}</div>
      <MeShortcut :items="keyShortcuts" />
    </el-dialog>

    <!-- 命令帮助 -->
    <CommandHelp ref="commandHelpRef" />
  </div>
</template>

<style scoped lang="scss">
.redis-terminal {
  height: 100%;
  overflow: hidden;
  position: relative;

  .terminal {
    height: 100%;
  }

  :deep(.xterm) {
    padding: 10px;
  }

  .node {
    position: absolute;
    right: 0;
    top: 0;
    z-index: 10;
  }

  .tool {
    position: absolute;
    right: 10px;
    bottom: 0;
    z-index: 10;
    align-items: center;
  }

  .output-mode-select {
    :deep(.el-select__wrapper) {
      min-height: 0;
      height: 30px;
      padding: 4px;
    }
  }
}

.terminal-shortcut-title {
  margin-bottom: 20px;
  font-size: 14px;
  font-weight: bold;
  color: var(--el-text-color-primary);
  text-align: center;
}
</style>
