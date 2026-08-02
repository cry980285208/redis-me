<script setup lang="ts">
import { computed, onMounted, useTemplateRef } from 'vue'
import type { FailedFunc, Message, SuccessFunc } from 'vue-web-terminal'

import { handleInputTipsSearch } from '@/plugins/ternimal'
import type { MeXtermCommandItem } from '@/types/me-interface'
import { isDark } from '@/utils/util'

type ExecCommandFn = (command: string) => string | Promise<string>

const props = withDefaults(
  defineProps<{
    welcome?: string
    prefix?: string
    execCommand?: ExecCommandFn
    commandHelp?: MeXtermCommandItem[]
  }>(),
  {
    welcome: '欢迎使用 Terminal',
    prefix: '$ ',
    execCommand: async (command: string) => `TODO 后台运行命令: ${command}`,
    commandHelp: () => [],
  },
)

type TerminalExpose = {
  pushMessage: (message: string | Message) => void
  fullscreen: () => void
  clearLog: () => void
  setCommand: (command: string) => void
}

const terminalRef = useTemplateRef<TerminalExpose | null>('terminal')

onMounted(() => {
  terminalRef.value?.pushMessage(props.welcome)
})

async function execCmd(
  _commandKey: string,
  command: string,
  success: SuccessFunc,
  _failed: FailedFunc,
  _name: string,
): Promise<void> {
  const data = await props.execCommand(command)
  const content = typeof data === 'string' ? data : String(data)
  success({ type: 'html', content })
}

const theme = computed(() => (isDark.value ? 'dark' : 'light'))

function onKeydown(e: KeyboardEvent): void {
  const term = terminalRef.value
  if (!term) return

  const key = e.key.toUpperCase()

  if (e.key === 'F11') {
    term.fullscreen()
    return
  }

  // 与快捷键文档 mod 一致：Mac ⌘ / Win·Linux Ctrl
  if (!(e.ctrlKey || e.metaKey)) return

  switch (key) {
    case 'L':
      term.clearLog()
      term.pushMessage(props.welcome)
      term.setCommand('')
      break
    case 'C':
      term.setCommand('')
      break
  }
}
</script>

<template>
  <terminal
    name="terminal"
    ref="terminal"
    @exec-cmd="execCmd"
    @on-keydown="onKeydown"
    :theme
    :show-header="false"
    :line-space="2"
    cursor-style="bar"
    context=""
    :command-store="commandHelp"
    :input-tips-search-handler="handleInputTipsSearch"
    :context-suffix="prefix">
  </terminal>
</template>

<style lang="scss">
/* 提示颜色 */
//.t-prompt {
//  color: var(--el-color-primary);
//}

/* help命令表格默认15px改为5px */
.t-table,
.t-table tr,
.t-table td,
.t-table tbody,
.t-table thead {
  padding: 5px !important;
}

/* 帮助手册 */
.t-cmd-help {
  top: 5px !important;
  right: 5px !important;
}

/* padding 和 背景色修改 */
.t-window {
  padding: 5px 5px 5px 20px !important;
  background-color: #efefef !important;
}

/* 深色主题下的背景色 */
html.dark {
  .t-window {
    background-color: var(--t-main-background-color) !important;
  }
}

/* 字体设置 */
code,
.t-window,
.t-ask-input,
.t-window p,
.t-window div,
.t-crude-font {
  font-family: var(--code-font) !important;
}

/* 保留空白并强制长串换行，避免横向滚动条 */
.t-window p,
.t-window div,
.t-log-box {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  word-break: break-all;
}
</style>
