<script setup lang="ts">
import { computed, inject } from 'vue'

import { shareProvideKey } from '@/types/me-interface'
import { isConnMinimalMode } from '@/utils/conn'
import RedisChart from '@/views/tab/RedisChart.vue'
import RedisInfo from '@/views/tab/RedisInfo/index.vue'
import RedisMemory from '@/views/tab/RedisMemory.vue'
import RedisMonitor from '@/views/tab/RedisMonitor.vue'
import RedisNotify from '@/views/tab/RedisNotify.vue'
import RedisPubsub from '@/views/tab/RedisPubsub.vue'
import RedisSearch from '@/views/tab/RedisSearch.vue'
import RedisSlow from '@/views/tab/RedisSlow.vue'
import RedisTerminal from '@/views/tab/RedisTerminal.vue'
import RedisValue from '@/views/tab/RedisValue/index.vue'

const share = inject(shareProvideKey)!
const minimalMode = computed(() => isConnMinimalMode(share.conn))
const infoSupported = computed(() => share.capabilities.infoSupported)
const searchSupported = computed(() => share.capabilities.redisSearchSupported)
const memoryUsageSupported = computed(() => share.capabilities.memoryUsageSupported)
</script>

<template>
  <el-tabs v-model="share.tabName" class="redis-tab">
    <!-- 信息界面: 极简模式或无INFO权限则不显示 -->
    <template v-if="!minimalMode">
      <me-tab-pane v-if="infoSupported" name="info" icon="el-icon-calendar">
        <RedisInfo />
      </me-tab-pane>
    </template>

    <!-- 值和终端界面: 极简模式下仅显示这两个 -->
    <me-tab-pane name="value" icon="el-icon-memo">
      <RedisValue />
    </me-tab-pane>
    <me-tab-pane name="terminal" icon="me-icon-terminal" lazy>
      <RedisTerminal />
    </me-tab-pane>

    <!-- 内存、慢查询、监控、发布订阅和图表界面 -->
    <template v-if="!minimalMode">
      <me-tab-pane v-if="searchSupported" name="search" icon="el-icon-search" lazy>
        <RedisSearch />
      </me-tab-pane>
      <me-tab-pane v-if="memoryUsageSupported" name="memory" icon="me-icon-memory" lazy>
        <RedisMemory />
      </me-tab-pane>
      <me-tab-pane name="slow" icon="me-icon-slow" lazy>
        <RedisSlow />
      </me-tab-pane>
      <me-tab-pane name="monitor" icon="el-icon-monitor" lazy>
        <RedisMonitor />
      </me-tab-pane>
      <me-tab-pane name="pubsub" icon="me-icon-pubsub" lazy>
        <RedisPubsub />
      </me-tab-pane>
      <me-tab-pane name="notify" icon="el-icon-bell" lazy>
        <RedisNotify />
      </me-tab-pane>
      <me-tab-pane v-if="infoSupported" name="chart" icon="el-icon-data-line" lazy>
        <RedisChart />
      </me-tab-pane>
    </template>
  </el-tabs>
</template>

<style scoped lang="scss">
.redis-tab {
  // border: 1px solid var(--el-border-color);
  // border-radius: var(--el-border-radius-base);
  padding: 0 5px 0 3px;

  height: 100%;

  :deep(.el-tab-pane) {
    height: 100%;
  }

  :deep(.el-tabs__item) {
    height: 30px;
    // EP 默认就是 align-items: center（内容垂直居中）。
    // 这里若改成 start，图标+文字会贴着顶部，比视觉中心高约 5px，看着不居中
    align-items: center;
    padding: 0 12px;
  }

  :deep(.el-tabs__header) {
    margin: 0 0 10px;
    padding-top: 2px;
  }

  // EP 的 nav-wrap 内置 margin-bottom:-1px，页签栏实际占位 2+30-1=31px，
  // 比左侧连接栏 32px 少 1px，导致右列输入框顶边比左列高 1px；归零后左右完全对齐
  :deep(.el-tabs__nav-wrap) {
    margin-bottom: 0;
  }

  // 宽度不够时 EP 显示的左右滚动箭头默认定位在 40px 高的行内（line-height:44px），
  // 而页签高度是 30px，箭头会与页签文字错位；
  // 这里让箭头与页签同高同起点，图标随文字一起垂直居中
  :deep(.el-tabs__nav-prev),
  :deep(.el-tabs__nav-next) {
    top: 0; // 与页签同一基准线（nav-wrap 顶部）
    height: 30px; // 与页签同高
    line-height: 30px;
    font-size: 14px;
  }
}
</style>
