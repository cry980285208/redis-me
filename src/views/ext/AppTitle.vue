<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window'
import { type as getOsType, version as getOsVersion } from '@tauri-apps/plugin-os'
import { computed, inject, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import { shareProvideKey } from '@/types/me-interface'
import { bus, meLog, meOk, OPEN_SETTING } from '@/utils/util'

const { t } = useI18n()

// 共享数据：与 AppMain 共用 App.vue provide 的同一份
const share = inject(shareProvideKey)!

// 切换只读/可写（顶部栏锁图标）
function changeReadonly(): void {
  share.readonly = !share.readonly
  meOk(share.readonly ? t('appMain.readonlyTip') : t('appMain.writableTip'))
}

const appWindow = getCurrentWindow()
const isFullScreen = ref(false)
const isMaximized = ref(false)

appWindow.onResized(async () => {
  isMaximized.value = await appWindow.isMaximized()
  isFullScreen.value = await appWindow.isFullscreen()
})

// MacOS 左侧留出红绿灯按钮 + 圆角空间；Tahoe (26+) 引入 Liquid Glass 大圆角(约26pt)需更多间距，此前 Big Sur~Sequoia (11~15) 为 10pt 小圆角
const isMacOS = getOsType() === 'macos'
const osVersionRaw = getOsVersion()
const osVersionMajor = parseInt(osVersionRaw, 10)
const isMacOSTahoePlus = isMacOS && osVersionMajor >= 26
meLog(
  '标题栏边距判断: os=',
  getOsType(),
  'version=',
  osVersionRaw,
  'major=',
  osVersionMajor,
  'tahoePlus=',
  isMacOSTahoePlus,
)
const marginLeft = computed(() => {
  if (isFullScreen.value) return '10px'
  if (!isMacOS) return '10px'
  return isMacOSTahoePlus ? '80px' : '70px'
})

// 点击图标切换主题
const toggleIcon = () => {
  const nowTheme =
    meTauri.settings.theme === 'system' ? meTauri.systemTheme : meTauri.settings.theme
  const newTheme = nowTheme === 'light' ? 'dark' : 'light'
  meTauri.settings.theme = newTheme
  meOk(newTheme)
}

// 点击名称切换语言或未来其他功能的快速测试验证
const toggleName = () => {
  const nowLanguage =
    meTauri.settings.language === 'system' ? meTauri.systemLanguage : meTauri.settings.language
  const newLanguage = nowLanguage === 'en' ? 'zhCN' : 'en'
  meTauri.settings.language = newLanguage
  meOk(newLanguage)
}
</script>

<template>
  <div data-tauri-drag-region class="title-bar me-flex">
    <div class="me-flex" style="align-items: center" :style="{ marginLeft }">
      <me-icon
        icon="me-icon-redis-me"
        class="icon-btn"
        style="font-size: 16px"
        @click="toggleIcon" />
      <div style="margin-left: 5px; font-size: 12px" @click="toggleName">RedisME</div>
    </div>
    <div class="title-right">
      <!-- 只读/可写 -->
      <me-icon
        v-if="share.conn"
        class="action-icon"
        :icon="share.readonly ? 'me-icon-lock' : 'me-icon-unlock'"
        :name="share.readonly ? t('appMain.readonly') : t('appMain.writable')"
        :hint="true"
        placement="bottom"
        @click="changeReadonly" />
      <!-- 设置：弹窗挂在 KeyHeader，兄弟组件间经 bus 通知打开 -->
      <me-icon
        class="action-icon"
        icon="el-icon-setting"
        :name="t('keyHeader.setting')"
        :hint="true"
        placement="bottom"
        :style="{ marginRight: isMacOS ? '15px' : '-5px' }"
        @click="bus.emit(OPEN_SETTING)" />
      <div style="font-size: 12px" v-if="!isMacOS">
        <me-icon
          icon="me-icon-window-minimize"
          class="title-button normal-btn"
          @click="appWindow.minimize()" />
        <me-icon
          icon="me-icon-window-maximize"
          class="title-button normal-btn"
          @click="appWindow.toggleMaximize()"
          v-show="!isMaximized" />
        <me-icon
          icon="me-icon-window-restore"
          class="title-button normal-btn"
          @click="appWindow.toggleMaximize()"
          v-show="isMaximized" />
        <me-icon
          icon="me-icon-window-close"
          class="title-button danger-btn"
          @click="appWindow.close()" />
      </div>
    </div>
  </div>
</template>

<style scoped lang="scss">
.title-bar {
  height: 30px;
  user-select: none;

  .title-right {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  /* 顶部栏功能图标（只读锁、设置） */
  .action-icon {
    font-size: 16px;
    cursor: pointer;
    color: var(--el-color-info);

    &:hover {
      color: var(--el-color-primary);
    }
  }

  .title-button {
    width: 40px;
    height: 30px;
    display: inline-flex;
    justify-content: center;
    align-items: center;
    user-select: none;
    -webkit-user-select: none;
  }

  .normal-btn:hover {
    background: var(--el-color-info-light-7);
  }

  .danger-btn:hover {
    background: var(--el-color-danger);
  }
}
</style>
