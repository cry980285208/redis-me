<script setup lang="ts">
// #region 导入
// 跳转到官网: Redis中英文/Valkey中英文；可选 command 拼具体命令文档路径
import { isZh, meOpenUrl } from '@/utils/util'
// #endregion

// #region 核心状态
// 与 `<me-website to="…">`、DOC_PATHS 键一致（供外部 props）
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
  defineProps<{
    to: DocTopic
    // 命令名（如 ACL CAT），拼到 to 对应路径后：acl-cat/
    command?: string
    // 有值时用 el-link 展示文案，否则用外链图标
    label?: string
    placement?: string
    marginLeft?: string
  }>(),
  { placement: 'right', marginLeft: '10px' },
)

// 下拉项 command，与模板中 el-dropdown-item 一致
const WEB_ORIGIN = {
  redis: 'https://redis.io',
  valkey: 'https://valkey.io',
  redisZh: 'https://redis.ac.cn',
  valkeyZh: 'https://valkey.cn',
} as const

type SiteCmd = keyof typeof WEB_ORIGIN

type Vendor = keyof (typeof DOC_PATHS)['info']
// #endregion

// #region 面板操作
// 官网路径统一：空格转连字符、小写，如 OBJECT ENCODING → object-encoding
function commandSlug(name: string): string {
  return name.trim().toLowerCase().replace(/\s+/g, '-')
}

function handleCommand(cmd: string): void {
  const site = cmd as SiteCmd
  const vendor = (site.endsWith('Zh') ? site.slice(0, -2) : site) as Vendor
  const base = WEB_ORIGIN[site]
  let path = DOC_PATHS[props.to][vendor]
  if (props.command) path += `${commandSlug(props.command)}/`
  meOpenUrl(base + path)
}
// #endregion
</script>

<template>
  <el-dropdown @command="handleCommand" trigger="hover" :placement :style="{ marginLeft }">
    <el-link v-if="label" type="primary" :underline="false">{{ label }}</el-link>
    <me-icon v-else icon="me-icon-link" style="font-size: 14px; color: var(--el-color-success)" />
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
