<script setup lang="ts">
import { computed, inject, nextTick, onMounted, reactive, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import MeShortcut from '@/components/MeShortcut.vue'
import { shareProvideKey, connUiProvideKey } from '@/types/me-interface'
import { getConnIcon } from '@/utils/conn'
import { getConnGlobalShortcuts, getTerminalShortcuts, getValueShortcuts } from '@/utils/shortcut'
import { bus, CONN_REFRESH, meCommands, meOk, openNewWindow } from '@/utils/util'
import About from '@/views/ext/About.vue'
import CommandLog from '@/views/ext/CommandLog.vue'
import Official from '@/views/ext/Official.vue'
import Setting from '@/views/ext/Setting.vue'

const share = inject(shareProvideKey)!
const connUi = inject(connUiProvideKey)!
const { t } = useI18n()

const dialog = reactive({ setting: false, info: false, social: false, commandLog: false })
const keyShortVisible = ref(false)
const globalShortcuts = computed(() => getConnGlobalShortcuts(t))
const codeMirrorShortcuts = computed(() => getValueShortcuts(t))
const terminalShortcuts = computed(() => getTerminalShortcuts(t))

function openSetting(): void {
  dialog.setting = !dialog.setting
}

function openShortcuts(): void {
  keyShortVisible.value = !keyShortVisible.value
}

onMounted(() => {
  connUi.openSetting = openSetting
  connUi.openShortcuts = openShortcuts
})

async function handleCommand(command: string): Promise<void> {
  if (command === 'refreshConn') {
    if (!share.conn) return
    const capabilities = await meCommands.connect(share.conn!.id)
    Object.assign(share.capabilities, capabilities)
    bus.emit(CONN_REFRESH)
  } else if ('closeConn' === command) {
    share.conn = null
  } else if ('commandLog' === command) {
    if (!share.conn) {
      meOk(t('keyHeader.commandLogNeedConn'))
      return
    }
    // 延迟到下一帧显示弹框，确保 dropdown 菜单先完成收起，避免弹框创建干扰 dropdown 状态
    nextTick(() => {
      dialog.commandLog = true
    })
  } else if ('setting' === command) {
    openSetting()
  } else if ('window' === command) {
    await openNewWindow()
  } else if ('info' === command) {
    dialog.info = true
  } else if ('social' === command) {
    dialog.social = true
  } else {
    meOk(`TODO: ${command}`)
  }
}
</script>

<template>
  <div class="key-header">
    <el-select
      v-model="share.conn"
      :placeholder="t('keyHeader.connHint')"
      class="conn"
      clearable
      filterable
      :disabled="share.connList.length === 0"
      value-key="id">
      <el-option v-for="item in share.connList" :label="item.name" :value="item" :key="item.id">
        <div :style="{ color: item?.color }">
          <me-icon :icon="getConnIcon(item)" :name="item.name" />
        </div>
      </el-option>

      <template #label="{ value }">
        <div :style="{ color: share.color }">
          <me-icon :icon="getConnIcon(value)" :name="value.name" />
        </div>
      </template>
    </el-select>

    <el-dropdown placement="bottom-end" @command="handleCommand" style="margin-left: 10px">
      <el-button type="success" icon="el-icon-operation" />
      <template #dropdown>
        <el-dropdown-menu>
          <template v-if="share.conn">
            <el-dropdown-item command="refreshConn">
              <me-icon :name="t('keyHeader.refreshConn')" icon="el-icon-refresh" />
            </el-dropdown-item>
            <el-dropdown-item command="closeConn">
              <me-icon :name="t('keyHeader.closeConn')" icon="el-icon-circle-close" />
            </el-dropdown-item>
            <el-dropdown-item command="commandLog">
              <me-icon :name="t('keyHeader.commandLog')" icon="me-icon-log" />
            </el-dropdown-item>
          </template>

          <el-dropdown-item command="window" :divided="!!share.conn">
            <me-icon :name="t('keyHeader.newWindow')" icon="me-icon-window" />
          </el-dropdown-item>
          <el-dropdown-item command="setting" divided>
            <me-icon :name="t('keyHeader.setting')" icon="el-icon-setting" />
          </el-dropdown-item>
          <el-dropdown-item command="social">
            <me-icon :name="t('keyHeader.social')" icon="me-icon-social" />
          </el-dropdown-item>
          <el-dropdown-item command="info">
            <me-icon :name="t('keyHeader.about')" icon="me-icon-info" />
          </el-dropdown-item>
        </el-dropdown-menu>
      </template>
    </el-dropdown>

    <!--为了方便主题语言等初始化，组件一直存在；为了方便v-model直接绑定弹框是否显示直接传入dialog-->
    <el-dialog v-model="dialog.setting" width="700" align-center draggable>
      <template #header>
        <me-icon icon="el-icon-setting" :name="t('setting.title')"></me-icon>
      </template>
      <Setting />
    </el-dialog>
    <el-dialog
      v-model="keyShortVisible"
      width="900"
      align-center
      draggable
      :show-close="false"
      header-class="me-shortcut-dialog__header">
      <div class="setting-shortcut-cols">
        <div class="setting-shortcut-col">
          <div class="setting-shortcut-col__title">{{ t('setting.shortcutGlobal') }}</div>
          <MeShortcut :items="globalShortcuts" compact />
        </div>
        <div class="setting-shortcut-col">
          <div class="setting-shortcut-col__title">{{ t('setting.shortcutCodeMirror') }}</div>
          <MeShortcut :items="codeMirrorShortcuts" compact />
        </div>
        <div class="setting-shortcut-col">
          <div class="setting-shortcut-col__title">{{ t('setting.shortcutTerminal') }}</div>
          <MeShortcut :items="terminalShortcuts" compact />
        </div>
      </div>
    </el-dialog>
    <el-dialog v-model="dialog.info" width="400" align-center draggable>
      <About />
    </el-dialog>
    <el-dialog
      v-model="dialog.social"
      width="430"
      align-center
      draggable
      :show-close="false"
      style="--el-dialog-bg-color: unset; box-shadow: unset">
      <Official />
    </el-dialog>
    <CommandLog v-model="dialog.commandLog" />
  </div>
</template>

<style scoped lang="scss">
.key-header {
  display: flex;
}

.setting-shortcut-cols {
  display: grid;
  grid-template-columns: 1fr 1fr 1fr;
  gap: 20px;
  align-items: start;
}

.setting-shortcut-col__title {
  margin-bottom: 20px;
  font-size: 14px;
  font-weight: bold;
  color: var(--el-text-color-primary);
}

.setting-shortcut-col {
  display: flex;
  flex-direction: column;
  min-width: 0;

  &:first-child {
    align-items: flex-start;
  }

  &:nth-child(2) {
    align-items: center;
  }

  &:last-child {
    align-items: flex-end;
  }
}
</style>
