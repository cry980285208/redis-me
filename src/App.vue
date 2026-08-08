<script setup lang="ts">
import { type as getOsType } from '@tauri-apps/plugin-os'
import { shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import { isDark } from '@/utils/util'
import AppMain from '@/views/AppMain.vue'
import AppResize from '@/views/ext/AppResize.vue'
import AppTitle from '@/views/ext/AppTitle.vue'

const { locale: i18nLocale } = useI18n()

/** 供 CSS 按平台作用：Linux WebKitGTK 字体补丁等（非 WebView 自动注入） */
const osType = getOsType()
document.documentElement.setAttribute('data-platform', osType)

// 主题切换
watch(
  () => meTauri.settings.theme,
  newValue => {
    const newTheme = newValue === 'system' ? meTauri.systemTheme : newValue
    isDark.value = newTheme === 'dark'
  },
  { immediate: true },
)

const locale = shallowRef<Record<string, unknown> | undefined>(undefined)
watch(
  () => meTauri.settings.language,
  newValue => {
    const language = String(newValue === 'system' ? meTauri.systemLanguage : (newValue ?? 'en'))
    const map = window.ElementPlusLanguageMap as Record<string, Record<string, unknown>> | undefined
    locale.value = map?.[language] as Record<string, unknown> | undefined
    i18nLocale.value = language
  },
  { immediate: true },
)

/** 设置里的字体栈 → CSS；未选择返回空，由 App.css :root 兜底 */
function fontStackToCss(v: string | string[] | undefined): string {
  if (v == null || v === '') return ''
  if (Array.isArray(v)) return v.filter(Boolean).join(', ')
  return v
}

function applyCssVar(name: string, value: string) {
  if (value) document.documentElement.style.setProperty(name, value)
  else document.documentElement.style.removeProperty(name)
}

// 字体切换：有选择才覆盖，否则沿用 App.css --ui-font / --code-font
watch(
  () => fontStackToCss(meTauri.settings.uiFont),
  v => applyCssVar('--ui-font', v),
  { immediate: true },
)
watch(
  () => fontStackToCss(meTauri.settings.codeFont),
  v => applyCssVar('--code-font', v),
  { immediate: true },
)
</script>

<template>
  <el-config-provider :locale>
    <AppTitle />
    <AppResize />
    <AppMain />
  </el-config-provider>
</template>

<style lang="scss">
#app {
  height: 100%;
  overflow: hidden;

  display: flex;
  flex-direction: column;
}
</style>
