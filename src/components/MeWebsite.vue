<script setup lang="ts">
// 跳转到官网: Redis中英文/Valkey中英文
import { openUrl } from '@tauri-apps/plugin-opener'

import { isZh } from '@/utils/util'

/** 与 `<me-website to="…">`、DOC_PATHS 键一致（供外部 props） */
const DOC_PATHS = {
  info: { redis: '/docs/latest/commands/info/', valkey: '/commands/info/' },
  config: {
    redis: '/docs/latest/operate/oss_and_stack/management/config/',
    valkey: '/topics/valkey.conf/',
  },
  client: { redis: '/docs/latest/commands/client-list/', valkey: '/commands/client-list/' },
  command: { redis: '/docs/latest/commands/', valkey: '/commands/' },
  slowlog: { redis: '/docs/latest/commands/slowlog-get/', valkey: '/commands/slowlog-get/' },
  monitor: { redis: '/docs/latest/commands/monitor/', valkey: '/commands/monitor/' },
  pubsub: { redis: '/docs/latest/commands/psubscribe/', valkey: '/commands/psubscribe/' },
  acl: {
    redis: '/docs/latest/operate/oss_and_stack/management/security/acl/',
    valkey: '/topics/acl/',
  },
} as const

type DocTopic = keyof typeof DOC_PATHS

const props = withDefaults(
  defineProps<{ to: DocTopic; placement?: string; marginLeft?: string }>(),
  { placement: 'right', marginLeft: '10px' },
)

/** 下拉项 command，与模板中 el-dropdown-item 一致 */
const WEB_ORIGIN = {
  redis: 'https://redis.io',
  valkey: 'https://valkey.io',
  redisZh: 'https://redis.ac.cn',
  valkeyZh: 'https://valkey.cn',
} as const

type SiteCmd = keyof typeof WEB_ORIGIN

type Vendor = keyof (typeof DOC_PATHS)['info']

function handleCommand(cmd: string): void {
  const site = cmd as SiteCmd
  const vendor = (site.endsWith('Zh') ? site.slice(0, -2) : site) as Vendor
  const base = WEB_ORIGIN[site]
  const paths = DOC_PATHS[props.to]
  void openUrl(base + paths[vendor])
}
</script>

<template>
  <el-dropdown @command="handleCommand" trigger="hover" :placement :style="{ marginLeft }">
    <me-icon icon="me-icon-link" style="font-size: 14px; color: var(--el-color-success)" />
    <template #dropdown>
      <el-dropdown-menu>
        <el-dropdown-item command="redis">
          <me-icon icon="me-icon-redis" name="Redis" />
        </el-dropdown-item>
        <el-dropdown-item v-if="isZh" command="redisZh">
          <me-icon icon="me-icon-redis" name="Redis 中文" />
        </el-dropdown-item>
        <el-dropdown-item command="valkey">
          <me-icon icon="me-icon-valkey" name="Valkey" />
        </el-dropdown-item>
        <el-dropdown-item v-if="isZh" command="valkeyZh">
          <me-icon icon="me-icon-valkey" name="Valkey 中文" />
        </el-dropdown-item>
      </el-dropdown-menu>
    </template>
  </el-dropdown>
</template>
