<script setup lang="ts">
// #region 导入
import { computed, onMounted, ref, useTemplateRef } from 'vue'
import type { FailedFunc, Message, SuccessFunc } from 'vue-web-terminal'

import { handleInputTipsSearch } from '@/plugins/ternimal'
import type { MeXtermCommandItem } from '@/types/me-interface'
import { isDark } from '@/utils/util'
// #endregion

// #region 核心状态

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
// #endregion

// #region 面板操作

type TerminalExpose = {
  pushMessage: (message: string | Message) => void
  fullscreen: () => void
  clearLog: () => void
  getCommand: () => string
  setCommand: (command: string) => void
  execute: (command: string) => boolean
}

const terminalRef = useTemplateRef<TerminalExpose | null>('terminal')
// 自动换行默认开启，Mod+B 切换（与 CodeMirror 一致）
const lineWrap = ref(true)

onMounted(() => {
  terminalRef.value?.pushMessage(props.welcome)
})

// execute() 不返回完成时机；多行粘贴时记下 resolve，等 execCmd 结束再发下一条
let pasteResolve: (() => void) | null = null

async function execCmd(
  _commandKey: string,
  command: string,
  success: SuccessFunc,
  _failed: FailedFunc,
  _name: string,
): Promise<void> {
  const resolve = pasteResolve
  pasteResolve = null
  try {
    const data = await props.execCommand(command)
    const content = typeof data === 'string' ? data : String(data)
    success({ type: 'html', content })
  } catch (error) {
    _failed(String(error))
  } finally {
    resolve?.()
  }
}

function runCommand(cmd: string): Promise<void> {
  const term = terminalRef.value
  if (!term) return Promise.resolve()
  return new Promise(resolve => {
    pasteResolve = resolve
    term.execute(cmd)
    // help / clear / open 不进 execCmd，上面的 resolve 还在
    if (pasteResolve) {
      pasteResolve = null
      resolve()
    }
  })
}

let pasteQueue: Promise<void> = Promise.resolve()

function enqueue(task: () => Promise<void> | void): void {
  pasteQueue = pasteQueue.then(task).catch((error: unknown) => console.error(error))
}

// 换行结尾的行立刻执行；最后一行没有换行则留在输入框
async function pasteLines(text: string): Promise<void> {
  const term = terminalRef.value
  if (!term) return
  const lines = text.replace(/\r\n/g, '\n').replace(/\r/g, '\n').split('\n')
  const tail = lines.pop() ?? ''
  const typed = term.getCommand().trim()
  if (typed && lines.length > 0) lines[0] = `${typed} ${lines[0]}`.trim()
  for (const line of lines) {
    const cmd = line.trim()
    if (cmd) await runCommand(cmd)
  }
  term.setCommand(tail.trim())
}

function onPaste(event: ClipboardEvent): void {
  const data = event.clipboardData
  const text = data?.getData('text/plain') || data?.getData('text') || ''
  if (!/[\r\n]/.test(text)) return
  event.preventDefault()
  enqueue(() => pasteLines(text))
}

// 右键按下时选区还在；到 contextmenu 时可能已被清掉，有选区则留给组件复制
let selectedOnPointer = ''

function onMouseDown(): void {
  selectedOnPointer = document.getSelection()?.toString() ?? ''
}

function insertAtCursor(root: HTMLElement, text: string): void {
  const term = terminalRef.value
  if (!term) return
  const input = root.querySelector('textarea')
  const current = term.getCommand()
  const at = input instanceof HTMLTextAreaElement ? input.selectionStart : current.length
  term.setCommand(current.slice(0, at) + text.trim() + current.slice(at))
}

function onContextMenu(event: MouseEvent): void {
  const selected = document.getSelection()?.toString() || selectedOnPointer
  selectedOnPointer = ''
  if (selected) return
  event.preventDefault()
  event.stopPropagation()
  const root = event.currentTarget
  if (!(root instanceof HTMLElement)) return
  void navigator.clipboard
    .readText()
    .then(text => {
      if (!text) return
      if (!/[\r\n]/.test(text)) {
        enqueue(() => insertAtCursor(root, text))
        return
      }
      enqueue(() => pasteLines(text))
    })
    .catch((error: unknown) => console.error(error))
}

const theme = computed(() => (isDark.value ? 'dark' : 'light'))
// #endregion

// #region 键盘事件
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
    case 'B':
      lineWrap.value = !lineWrap.value
      break
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
// #endregion
</script>

<template>
  <div
    class="me-xterm"
    :class="{ 'is-wrap': lineWrap }"
    @paste.capture="onPaste"
    @mousedown.capture="onMouseDown"
    @contextmenu.capture="onContextMenu">
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
  </div>
</template>

<style lang="scss">
/* 提示颜色 */
//.t-prompt {
//  color: var(--el-color-primary);
//}

/* 提示行：左侧命令完整显示，右侧说明过长时省略 */
.t-cmd-tips-item {
  display: flex !important;
  align-items: center;
  justify-content: space-between;
  gap: 20px;
}

.t-cmd-tips-content {
  flex: 0 0 auto;
  white-space: nowrap;
}

.t-cmd-tips-des {
  flex: 1 1 auto;
  min-width: 0;
  margin-left: 0 !important;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  text-align: right;
}

.t-cmd-tips-items {
  overflow-x: auto;
}

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

.t-window {
  /* 库内 z-index:1 会隔离层叠上下文，导致补全(100)盖不过 help(99)；unset 后同层比较 */
  /* z-index: unset !important; */
  padding: 5px 5px 5px 20px !important;
  background-color: #efefef !important;
}

/* 深色主题下的背景色 */
html.dark {
  .t-window {
    background-color: var(--t-main-background-color) !important;
  }
}

/* 字体设置：终端正文 + 右上角说明 + 补全列表 */
code,
.t-window,
.t-ask-input,
.t-window p,
.t-window div,
.t-crude-font,
.t-cmd-help,
.t-cmd-help-des,
.t-cmd-tips-items {
  font-family: var(--code-font) !important;
}

.me-xterm {
  height: 100%;
}

/* 自动换行开启：保留空白并强制长串换行，避免横向滚动条 */
.me-xterm.is-wrap {
  .t-window p,
  .t-window div,
  .t-log-box {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    word-break: break-all;
  }
}

/* 自动换行关闭：单行展示，横向滚动 */
.me-xterm:not(.is-wrap) {
  .t-window {
    overflow-x: auto;
  }

  .t-window p,
  .t-window div,
  .t-log-box {
    white-space: pre;
    overflow-wrap: normal;
    word-break: normal;
  }
}
</style>
