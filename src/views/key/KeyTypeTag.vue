<script setup lang="ts">
import { inject, ref, watch } from 'vue'

import { shareProvideKey } from '@/types/me-interface'
import type { RedisKey_Deserialize } from '@/types/tauri-specta'
import { resolveKeyType } from '@/utils/key-type-cache'
import { meKeyShort, meType } from '@/utils/redis-display'

const share = inject(shareProvideKey)!
const props = defineProps<{
  /** 树虚拟列表更新时可能短暂缺失，避免 prop 校验告警 */
  redisKey?: RedisKey_Deserialize | null
}>()

const keyType = ref<string | undefined>()

// scan 无 keyType；resolveKeyType 按 redisKeyId 缓存（UTF-8 有无 bytes 已归一）
watch(
  () => [props.redisKey?.key, props.redisKey?.bytes, share.conn?.id, share.conn?.db] as const,
  async () => {
    const rk = props.redisKey
    const conn = share.conn
    if (!rk || !conn) {
      keyType.value = undefined
      return
    }
    keyType.value = await resolveKeyType(conn.id, conn.db, rk)
  },
  { immediate: true },
)
</script>

<template>
  <el-tag v-if="redisKey" size="small" disable-transitions :type="meType(keyType)" effect="dark">
    {{ meKeyShort(keyType) }}
  </el-tag>
  <el-tag v-else size="small" disable-transitions type="info" effect="dark">?</el-tag>
</template>
