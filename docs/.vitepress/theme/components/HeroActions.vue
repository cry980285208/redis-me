<script setup>
import { useData } from 'vitepress'
import { computed, onMounted, ref } from 'vue'

import { version } from '../../../../package.json'
import Apple from './icon/Apple.vue'
import Linux from './icon/Linux.vue'
import Windows from './icon/Windows.vue'

const { lang } = useData()
const isZh = computed(() => lang.value.startsWith('zh'))
const viewText = computed(() => (isZh.value ? '进入Github' : 'View on GitHub'))
const gitHost = computed(() => (isZh.value ? 'gitee' : 'github'))

/** 客户端检测 OS；SSR 与 Android 等未知平台默认 Windows */
const detectedOs = ref('Windows')

onMounted(() => {
  detectedOs.value = detectOs()
})

function detectOs() {
  const ua = navigator.userAgent
  const platform = (navigator.platform || '').toLowerCase()

  if (/win/i.test(platform) || /windows/i.test(ua)) return 'Windows'
  if (/mac/i.test(platform) || /macintosh|mac os x/i.test(ua)) return 'macOS'
  if (/linux/i.test(platform) || /linux/i.test(ua)) return 'Linux'
  return 'Windows'
}

function downloadLink(fileName) {
  return `https://${gitHost.value}.com/hepengju/redis-me/releases/download/v${version}/${fileName}`
}

function winItem(arch, type, fileName) {
  const isInstaller = type === 'exe'
  const label = isZh.value
    ? isInstaller
      ? '安装版'
      : '绿色版'
    : isInstaller
      ? 'Installer'
      : 'Portable'
  return { text: `${arch} ${label} (.${type})`, link: downloadLink(fileName) }
}

function macItem(chip, fileName) {
  const label =
    chip === 'apple'
      ? isZh.value
        ? 'Apple芯片'
        : 'Apple Silicon'
      : isZh.value
        ? 'Intel芯片'
        : 'Intel Mac'
  return { text: `${label} (.dmg)`, link: downloadLink(fileName) }
}

function linuxItem(arch, format, fileName) {
  const label = isZh.value
    ? { deb: 'Debian', rpm: 'Redhat', appimage: '通用' }[format]
    : { deb: 'Debian', rpm: 'Redhat', appimage: 'Generic' }[format]
  const suffix = format === 'appimage' ? 'AppImage' : format
  return { text: `${arch} ${label} (.${suffix})`, link: downloadLink(fileName) }
}

const downloadMenu = computed(() => {
  return [
    {
      os: 'Windows',
      icon: Windows,
      items: [
        winItem('x64', 'exe', `RedisME_${version}_x64-setup.exe`),
        winItem('x64', 'zip', `RedisME_${version}_portable_x64.zip`),
        winItem('arm64', 'exe', `RedisME_${version}_arm64-setup.exe`),
        winItem('arm64', 'zip', `RedisME_${version}_portable_arm64.zip`),
      ],
    },
    {
      os: 'macOS',
      icon: Apple,
      items: [
        macItem('apple', `RedisME_${version}_aarch64.dmg`),
        macItem('intel', `RedisME_${version}_x64.dmg`),
      ],
    },
    {
      os: 'Linux',
      icon: Linux,
      items: [
        linuxItem('x64', 'deb', `RedisME_${version}_amd64.deb`),
        linuxItem('x64', 'rpm', `RedisME-${version}-1.x86_64.rpm`),
        linuxItem('x64', 'appimage', `RedisME_${version}_amd64.AppImage`),
        linuxItem('arm64', 'deb', `RedisME_${version}_arm64.deb`),
        linuxItem('arm64', 'rpm', `RedisME-${version}-1.aarch64.rpm`),
        linuxItem('arm64', 'appimage', `RedisME_${version}_aarch64.AppImage`),
      ],
    },
  ]
})

const currentOsGroup = computed(
  () => downloadMenu.value.find(group => group.os === detectedOs.value) ?? downloadMenu.value[0],
)

const otherOsGroups = computed(() =>
  downloadMenu.value.filter(group => group.os !== detectedOs.value),
)

const primaryDownload = computed(() => currentOsGroup.value.items[0])

const otherPlatformsLabel = computed(() =>
  isZh.value ? '其他平台下载' : 'Download for other platforms',
)
</script>

<template>
  <div class="actions">
    <div class="action download-action">
      <div class="split-button action-button brand">
        <div class="split-main">
          <a
            class="split-main-button"
            :href="primaryDownload.link"
            rel="noreferrer"
            target="_blank">
            <component :is="currentOsGroup.icon" class="os-icon" />
            <span>{{ currentOsGroup.os }}</span>
          </a>
          <ul class="dropdown-menu">
            <li
              v-for="item in currentOsGroup.items"
              :key="item.link"
              class="group-item"
              style="font-size: 14px">
              <a :href="item.link" rel="noreferrer" target="_blank">{{ item.text }}</a>
            </li>
          </ul>
        </div>
        <span class="split-divider" aria-hidden="true">|</span>
        <div class="split-more">
          <span class="more-button" :aria-label="otherPlatformsLabel">⋯</span>
          <ul class="dropdown-menu other-menu">
            <template v-for="(group, i) in otherOsGroups" :key="group.os">
              <li class="group-title" :class="{ 'group-title-first': i === 0 }">
                <component :is="group.icon" />
                <span>{{ group.os }}</span>
              </li>
              <li
                v-for="item in group.items"
                :key="item.link"
                class="group-item"
                style="font-size: 14px">
                <a :href="item.link" rel="noreferrer" target="_blank">{{ item.text }}</a>
              </li>
            </template>
          </ul>
        </div>
      </div>
    </div>
    <div class="action">
      <a
        class="action-button alt"
        href="https://github.com/hepengju/redis-me"
        rel="noreferrer"
        target="_blank">
        <span class="github-icon" />
        <span>{{ viewText }}</span>
      </a>
    </div>
  </div>
</template>

<style scoped>
.actions {
  position: relative;
  z-index: 30;
  display: flex;
  flex-wrap: wrap;
}

.action {
  flex-shrink: 0;
  padding: 6px;
}

@media (min-width: 640px) {
  .actions {
    padding-top: 32px;
  }
}

@media (min-width: 960px) {
  .actions {
    justify-content: flex-start;
  }
}

.action-button.brand {
  border-color: var(--vp-button-brand-border);
  color: var(--vp-button-brand-text);
  background-color: var(--vp-button-brand-bg);
}

.action-button.alt {
  border-color: var(--vp-button-alt-border);
  color: var(--vp-button-alt-text);
  /*background-color: var(--vp-button-alt-bg);*/
}

.action-button {
  border-radius: 10px;
  padding: 0 20px;
  line-height: 38px;
  font-size: 14px;
  display: inline-block;
  border: 1px solid transparent;
  text-align: center;
  font-weight: 600;
  white-space: nowrap;
  transition:
    color 0.25s,
    border-color 0.25s,
    background-color 0.25s;
  cursor: pointer;
}

.download-action {
  position: relative;
  z-index: 31;
}

.split-button {
  display: inline-flex;
  align-items: stretch;
  padding: 0;
}

.split-main,
.split-more {
  position: relative;
}

.split-main:hover .dropdown-menu,
.split-main .dropdown-menu:hover,
.split-more:hover .dropdown-menu,
.split-more .dropdown-menu:hover {
  display: block;
}

.split-main-button {
  display: flex;
  align-items: center;
  padding: 0 20px;
  line-height: 38px;
  font-size: 14px;
  font-weight: 600;
  color: inherit;
  white-space: nowrap;
  border-radius: 10px 0 0 10px;
}

.split-divider {
  display: flex;
  align-items: center;
  opacity: 0.45;
  font-weight: 400;
  line-height: 1;
  user-select: none;
}

.more-button {
  display: flex;
  align-items: center;
  padding: 0 14px;
  line-height: 38px;
  font-size: 14px;
  font-weight: 600;
  color: inherit;
  border-radius: 0 10px 10px 0;
  cursor: pointer;
  user-select: none;
}

.os-icon {
  flex-shrink: 0;
  margin-right: 8px;
  margin-left: -4px;
}

.dropdown-menu {
  position: absolute;
  top: calc(100% + 5px);
  left: 0;
  list-style-type: none;
  margin: 0;
  padding: 5px;
  border-radius: 10px;
  border: 1px solid var(--vp-c-divider);
  background-color: var(--vp-c-bg-soft);
  box-shadow: var(--vp-shadow-2);
  z-index: 100;
  display: none;
}

/* 透明桥接区，避免按钮与菜单之间的空隙导致 hover 丢失 */
.dropdown-menu::before {
  content: '';
  position: absolute;
  top: -10px;
  left: 0;
  right: 0;
  height: 10px;
}

.dropdown-menu li {
  padding: 8px 8px;
  font-size: 14px;
  cursor: pointer;
  display: flex;
  flex-direction: row;
  gap: 5px;
  align-items: center;
  color: var(--vp-c-text-1);
}

.dropdown-menu li:hover {
  background-color: var(--vp-c-default-soft);
  border-radius: 10px;
}

.dropdown-menu .group-title {
  padding: 8px 8px 4px;
  display: flex;
  flex-direction: row;
  gap: 5px;
  align-items: center;
  font-size: 13px;
  font-weight: 700;
  color: var(--vp-c-text-2);
  cursor: default;
  margin-top: 6px;
  border-top: 1px solid var(--vp-c-divider);
}

.dropdown-menu .group-title-first {
  margin-top: 0;
  border-top: none;
}

.dropdown-menu .group-title:hover {
  background-color: transparent;
}

.split-main .dropdown-menu .group-item {
  padding: 6px 8px;
}

.dropdown-menu .group-item {
  padding: 6px 8px 6px 30px;
}

/* 新增a标签图标 */
.action a {
  display: flex;
  align-items: center;
}

.github-icon {
  display: inline-block;
  width: 16px;
  height: 16px;
  background-color: currentColor;
  -webkit-mask-image: var(--svg);
  mask-image: var(--svg);
  -webkit-mask-repeat: no-repeat;
  mask-repeat: no-repeat;
  -webkit-mask-size: 100% 100%;
  mask-size: 100% 100%;
  --svg: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' width='24' height='24'%3E%3Cpath fill='black' d='M12 .297c-6.63 0-12 5.373-12 12c0 5.303 3.438 9.8 8.205 11.385c.6.113.82-.258.82-.577c0-.285-.01-1.04-.015-2.04c-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729c1.205.084 1.838 1.236 1.838 1.236c1.07 1.835 2.809 1.305 3.495.998c.108-.776.417-1.305.76-1.605c-2.665-.3-5.466-1.332-5.466-5.93c0-1.31.465-2.38 1.235-3.22c-.135-.303-.54-1.523.105-3.176c0 0 1.005-.322 3.3 1.23c.96-.267 1.98-.399 3-.405c1.02.006 2.04.138 3 .405c2.28-1.552 3.285-1.23 3.285-1.23c.645 1.653.24 2.873.12 3.176c.765.84 1.23 1.91 1.23 3.22c0 4.61-2.805 5.625-5.475 5.92c.42.36.81 1.096.81 2.22c0 1.606-.015 2.896-.015 3.286c0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12'/%3E%3C/svg%3E");

  margin-right: 8px;
}

.other-menu {
  right: 0;
  left: auto;
  min-width: 220px;
}
</style>
