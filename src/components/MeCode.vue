<script setup lang="ts">
// #region 导入
import { LanguageSupport, StreamLanguage, syntaxHighlighting } from '@codemirror/language'
import { properties as propertiesMode } from '@codemirror/legacy-modes/mode/properties'
import { Prec } from '@codemirror/state'
import { EditorView, keymap, lineNumbers } from '@codemirror/view'
import { useDark } from '@vueuse/core'
import { json5 as cmJson5 } from 'codemirror-json5'
import { computed, ref } from 'vue'
import CodeMirror from 'vue-codemirror6'

import {
  meBasicSetup,
  propertiesDarkSyntax,
  propertiesEagerParse,
  zhPhrases,
} from '@/plugins/codemirror'
import { isZh } from '@/utils/util'
// #endregion

// #region 核心状态
// Java .properties 流式解析：适配 Redis INFO/CONFIG（key:value、# 段注释、续行）
const propertiesLang = new LanguageSupport(StreamLanguage.define(propertiesMode))

// 在编辑器聚焦时 F11：对 `.cm-editor` 调用 Fullscreen API（再按 F11 或 Esc 退出）
function toggleCmEditorFullscreen(el: HTMLElement) {
  const doc = el.ownerDocument
  if (doc.fullscreenElement === el) {
    void doc.exitFullscreen().catch(() => {})
    return
  }
  void el.requestFullscreen().catch(() => {})
}

// 自动换行默认开启，Mod+B 切换（Mac ⌘ / Win·Linux Ctrl）
const lineWrap = ref(true)
// 行号默认显示，Mod+N 切换
const showLineNumbers = ref(true)

// 编辑器字号（px），Mod+= / Mod+- 调节，Mod+0 恢复默认
const FONT_SIZE_DEFAULT = 15
const FONT_SIZE_MIN = 10
const FONT_SIZE_MAX = 28
const FONT_SIZE_STEP = 2
const fontSizePx = ref(FONT_SIZE_DEFAULT)

function bumpFontSize(delta: number) {
  fontSizePx.value = Math.min(FONT_SIZE_MAX, Math.max(FONT_SIZE_MIN, fontSizePx.value + delta))
}
// #endregion

// #region 计算属性
const meCodePrecKeymap = Prec.highest(
  keymap.of([
    {
      key: 'F11',
      run: view => {
        toggleCmEditorFullscreen(view.dom)
        return true
      },
    },
    {
      key: 'Mod-b',
      run: () => {
        lineWrap.value = !lineWrap.value
        return true
      },
    },
    {
      key: 'Mod-n',
      run: () => {
        showLineNumbers.value = !showLineNumbers.value
        return true
      },
    },
    {
      key: 'Mod-=',
      run: () => {
        bumpFontSize(FONT_SIZE_STEP)
        return true
      },
    },
    {
      key: 'Mod--',
      run: () => {
        bumpFontSize(-FONT_SIZE_STEP)
        return true
      },
    },
    {
      key: 'Mod-0',
      run: () => {
        fontSizePx.value = FONT_SIZE_DEFAULT
        return true
      },
    },
  ]),
)

const props = withDefaults(
  defineProps<{
    /** `json` / `json5` 均使用 JSON5 语法高亮（合法 JSON 也可）；`properties` 为 Redis INFO/CONFIG */
    mode?: string
    readOnly?: boolean
    /** 解码失败：danger 描边 */
    error?: boolean
  }>(),
  { mode: 'json', readOnly: false, error: false },
)

const rootClass = computed(() => [
  ...(props.readOnly ? ['codemirror-opacity', 'is-disabled'] : []),
  ...(props.error ? ['is-decode-error'] : []),
])

const dark = useDark()
const lang = computed(() => {
  if (props.mode === 'json' || props.mode === 'json5') return cmJson5()
  if (props.mode === 'properties') return propertiesLang
  return undefined
})
const phrases = computed(() => (isZh.value ? zhPhrases : {}))
const extensions = computed(() => {
  const list = [
    meBasicSetup,
    meCodePrecKeymap,
    EditorView.theme({ '&': { fontSize: `${fontSizePx.value}px` } }),
  ]

  if (lineWrap.value) {
    list.push(EditorView.lineWrapping)
  }

  if (showLineNumbers.value) {
    list.push(lineNumbers())
  }

  if (props.mode === 'properties') {
    list.push(syntaxHighlighting(propertiesDarkSyntax), propertiesEagerParse)
  }
  return list
})
// #endregion
</script>

<template>
  <!-- https://github.com/logue/vue-codemirror6  -->
  <code-mirror
    v-bind="$attrs"
    :dark
    :lang
    :phrases
    :extensions
    :readonly="props.readOnly"
    :class="rootClass" />
</template>

<style scoped lang="scss">
.codemirror-opacity {
  opacity: 0.8;
}

.is-decode-error {
  /* border 不被 gutter 挡住；勿用 inset shadow */
  border: 2px solid var(--el-color-danger);
  box-sizing: border-box;
}

.vue-codemirror {
  height: 100%;

  /* 默认高度 */
  :deep(.cm-editor) {
    height: 100%;
  }

  /* F11 全屏时填满视口（普通 DOM 全屏，非整窗 Tauri F11） */
  :deep(.cm-editor:fullscreen) {
    box-sizing: border-box;
    width: 100vw;
    height: 100vh;
    max-height: 100vh;
    background-color: var(--el-bg-color-page, var(--el-bg-color, #fff));
  }

  :deep(.cm-editor:fullscreen .cm-scroller) {
    flex: 1;
    min-height: 0;
  }

  /* 字体设置 */
  :deep(.cm-scroller) {
    font-family: var(--code-font);
  }
}

html.dark .vue-codemirror {
  background-color: #272822;

  :deep(.cm-editor:fullscreen) {
    background-color: #272822;
  }

  :deep(.ͼ3 .cm-gutters) {
    background-color: #272822;
  }

  /* 默认选区 #233 与背景 #272822 过近，略提亮 */
  :deep(.cm-selectionBackground) {
    background-color: #4b6a3f !important;
  }

  /* JSON值在黑色模式下红色看着不舒服，因此改下 */
  /* Json 的 null（默认 #708 过暗） */
  :deep(.ͼb) {
    color: #ae81ff;
  }

  /* Json的字符串值 */
  :deep(.ͼe) {
    color: #e6db74;
  }

  /* Json的布尔值 */
  :deep(.ͼc) {
    color: var(--el-color-primary);
  }

  /* Json的数字值 */
  :deep(.ͼd) {
    color: var(--el-color-success);
  }

  /* Json5的注释 */
  :deep(.ͼm) {
    color: #75715e;
  }
}
</style>
