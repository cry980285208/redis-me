<script setup lang="ts">
// #region 导入
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import type { RedisCertLabels } from '@/utils/redis-install-gen'
import { genOpensslCertScript } from '@/utils/redis-install-gen'
// #endregion

const { t } = useI18n()

// #region 核心状态
const visible = ref(false)

// 节点 SAN 由父级提供（全部节点 IP + 回环地址）
const props = defineProps<{ sans: string[] }>()

function open(): void {
  visible.value = true
}

defineExpose({ open })
// #endregion

// #region 产物展示
const certLabels = computed<RedisCertLabels>(() => ({
  scriptTitle: t('redisCert.scriptTitle'),
  scriptOutput: t('redisCert.scriptOutput'),
  step1Title: t('redisCert.step1Title'),
  step1Desc: t('redisCert.step1Desc'),
  step2Title: t('redisCert.step2Title'),
  step2Desc: t('redisCert.step2Desc'),
  step2Note: t('redisCert.step2Note'),
  step3Title: t('redisCert.step3Title'),
  step4Title: t('redisCert.step4Title'),
  step5Title: t('redisCert.step5Title'),
  step6Title: t('redisCert.step6Title'),
}))

const opensslScript = computed(() =>
  genOpensslCertScript({
    sans: props.sans,
    certDays: 36500,
    certCn: 'redis',
    labels: certLabels.value,
  }),
)
// #endregion
</script>

<template>
  <me-dialog v-model="visible" :title="t('redisCert.title')" icon="me-icon-cert" width="720">
    <me-code :model-value="opensslScript" mode="shell" copyable style="height: 100%" />
  </me-dialog>
</template>

<style scoped lang="scss">
:deep(.me-dialog-body) {
  height: 60vh;
}
</style>
