<script setup>
import { useData } from 'vitepress'
import { computed } from 'vue'

const { isDark, lang } = useData()

const imgSrc = computed(() => {
  const suffix = lang.value.startsWith('zh') ? '-zh' : ''
  // TODO: 部署在服务器上后，切换为dark主题，浏览器刷新后主题是dark但图片是light(可能是vitepress的bug)，先记录一下
  const prefix = isDark.value ? 'dark' : 'light'
  const file = `${prefix}${suffix}.png`
  // 英文首页在站点根、中文在 /zh/，用相对 URL 兼容根路径与子路径部署
  return lang.value.startsWith('zh') ? `../images/${file}` : `./images/${file}`
})
</script>

<template>
  <div class="preview-wrapper">
    <div class="preview-inner">
      <img :src="imgSrc" alt="preview" />
    </div>
  </div>
</template>

<style scoped>
.preview-wrapper {
  margin-top: -30px;
  padding: 0 24px 32px;
  text-align: center;
  display: flex;
  flex-direction: row;
  justify-content: center;
}

.preview-inner {
  object-fit: cover;
  max-width: 100%;
}

@media (min-width: 640px) {
  .preview-wrapper {
    padding: 0 48px 32px;
  }
}

@media (min-width: 960px) {
  .preview-wrapper {
    padding: 0 64px 32px;
  }

  .preview-inner {
    max-width: 1080px;
  }
}
</style>
