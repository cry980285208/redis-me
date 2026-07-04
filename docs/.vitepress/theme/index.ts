import { inBrowser, useData } from 'vitepress'
import DefaultTheme from 'vitepress/theme'
import { h, watchEffect } from 'vue'

import AppPreview from './components/AppPreview.vue'
import HeroActions from './components/HeroActions.vue'
import { LANG_STORAGE_KEY } from './lang-redirect'

import './styles.css'

export default {
  extends: DefaultTheme,
  Layout() {
    return h(DefaultTheme.Layout, null, {
      'home-hero-actions-after': () => h(HeroActions),
      'home-hero-after': () => h(AppPreview),
    })
  },
  setup() {
    const { lang } = useData()
    // 导航栏切换语言后记住偏好，下次访问根路径不再按浏览器猜测
    watchEffect(() => {
      if (!inBrowser) return
      localStorage.setItem(LANG_STORAGE_KEY, lang.value.startsWith('zh') ? 'zh' : 'en')
    })
  },
}
