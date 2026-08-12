<script setup lang="ts">
// #region 导入
import { openUrl } from '@tauri-apps/plugin-opener'
import { useI18n } from 'vue-i18n'
// #endregion

// #region 核心状态
const { t } = useI18n()
// #endregion

// #region 面板操作
function handleLogoClick(): void {
  openUrl('https://www.hepengju.com')
}

function handleGithubClick(): void {
  openUrl('https://github.com/hepengju/redis-me')
}

function handleBugClick(): void {
  openUrl('https://github.com/hepengju/redis-me/issues')
}
// #endregion
</script>

<template>
  <div class="key-empty">
    <div class="logo-wrap" @click="handleLogoClick">
      <div class="logo-glow" aria-hidden="true" />
      <SvgIcon class="logo-icon" name="me-icon-logo-color" />
    </div>
    <div class="tagline">{{ t('keyEmpty.tagline') }}</div>

    <div class="github">
      <me-icon icon="me-icon-github" :name="t('keyEmpty.sourceCode')" @click="handleGithubClick" />
      <me-icon icon="me-icon-bug" :name="t('keyEmpty.bugReport')" @click="handleBugClick" />
    </div>
  </div>
</template>

<style scoped lang="scss">
.key-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  // justify-content: center;

  /* 与文档站 hero 一致的紫/青对角渐变光晕，blur 仅作用于底层，图标保持清晰 */
  .logo-wrap {
    cursor: pointer;
    margin-top: 20vh;
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100px;
    height: 100px;
  }

  .logo-glow {
    position: absolute;
    z-index: 0;
    /* 略大于图标区域，光晕向外晕开 */
    inset: -30px;
    border-radius: 50%;
    background: linear-gradient(-45deg, #bd34fe 48%, #47caff 52%);
    opacity: 0.5;
    filter: blur(44px);
    pointer-events: none;
  }

  .logo-icon {
    position: relative;
    z-index: 1;
    font-size: 100px;
    opacity: 0.6;
    filter: drop-shadow(-2px 4px 8px rgba(0, 0, 0, 0.2));

    &:hover {
      opacity: 0.8;
    }
  }

  .tagline {
    margin-top: 40px;
    font-size: 16px;
    font-weight: bold;
    opacity: 0.5;
    background: -webkit-linear-gradient(120deg, #c7ba4e 30%, #bd34fe);
    -webkit-text-fill-color: transparent;
    background-clip: text;
  }

  .github {
    margin-top: 20vh;
    width: 60%;
    display: flex;
    justify-content: space-between;

    font-size: 16px;
    font-weight: bold;
    color: var(--el-color-info);
    opacity: 0.6;

    div {
      cursor: pointer;

      &:hover {
        color: var(--el-color-success);
      }
    }
  }
}
</style>
