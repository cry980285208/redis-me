/** 应用内自动更新（Tauri updater） */
import { openUrl } from '@tauri-apps/plugin-opener'
import { type } from '@tauri-apps/plugin-os'
import { relaunch } from '@tauri-apps/plugin-process'
import {
  check,
  type CheckOptions,
  type DownloadEvent,
  type Update,
} from '@tauri-apps/plugin-updater'
import { ElMessage } from 'element-plus'
import type { ElMessageBoxOptions } from 'element-plus'
import { h } from 'vue'

import i18n from '@/locales'
import type { MeAppUpdateState } from '@/types/me-interface'
import { DoNothing, meCommands, meConfirm, meErr, meLog } from '@/utils/util'

const t = i18n.global.t

function errString(e: unknown): string {
  if (e instanceof Error) return e.message
  if (typeof e === 'string') return e
  try {
    return JSON.stringify(e)
  } catch {
    return Object.prototype.toString.call(e)
  }
}

const manualCloseOptions: ElMessageBoxOptions = {
  closeOnClickModal: false,
  closeOnPressEscape: false,
  type: 'info',
}

export async function meCheckUpdate(
  quiet = true,
  checkOptions: CheckOptions = {},
  app: MeAppUpdateState,
): Promise<void> {
  if (window?.meTauri?.isAppStore) {
    meLog('应用商店内部的应用更新，忽略检查接口')
    return
  }

  if (!quiet) {
    ElMessage.primary(t('util.checking'))
  }

  const update = await check(checkOptions).catch(DoNothing)
  if (update) {
    await meDownloadUpdate(quiet, update, app)
  } else if (update === null) {
    if (!quiet) {
      ElMessage.success(t('util.latestVersion'))
    }
  } else {
    if (!quiet) {
      ElMessage.error(t('util.checkUpdateErr'))
    }
  }
}

export async function meDownloadUpdate(
  quiet = true,
  update: Update,
  app: MeAppUpdateState,
): Promise<void> {
  meLog('检查结果:', update)
  const hint = t('util.updateHint', { version: update.version })
  const changelog = t('util.changelog')
  const changelogUrl = t('util.changelogUrl')
  const message = () =>
    h('p', null, [
      h('span', hint),
      h(
        'a',
        {
          style:
            'color: var(--el-color-primary); text-decoration: none; margin-left: 5px; cursor: pointer; ',
          onClick: () => {
            void openUrl(changelogUrl)
          },
        },
        changelog,
      ),
    ])

  meConfirm(
    'MessageInvalid',
    async () => {
      try {
        app.downloading = true
        app.downloadPercentage = 0

        let downloaded = 0
        let contentLength = 0
        const downloadingHandle = (event: DownloadEvent) => {
          switch (event.event) {
            case 'Started':
              contentLength = event.data.contentLength ?? 0
              break
            case 'Progress':
              downloaded += event.data.chunkLength
              app.downloadPercentage = contentLength
                ? Math.round((downloaded / contentLength) * 100)
                : 0
              break
            case 'Finished':
              app.downloadPercentage = 100
              break
          }
        }

        const isWindows = type() === 'windows'
        const isMacOS = type() === 'macos'
        if (isWindows) {
          await update.download(downloadingHandle)
          meConfirm(t('util.downloadDown'), async () => await update.install(), manualCloseOptions)
        } else {
          await update.downloadAndInstall(downloadingHandle)
          // macOS：relaunch 会与 single-instance 竞态，改走 Rust 延迟 open 重启
          meConfirm(
            t('util.updateDone'),
            async () => {
              if (isMacOS) {
                await meCommands.restartAfterUpdate()
              } else {
                await relaunch()
              }
            },
            manualCloseOptions,
          )
        }
      } catch (e) {
        meErr(t('util.updateErr', { message: errString(e) }))
      } finally {
        app.downloading = false
      }
    },
    { ...manualCloseOptions, message },
  )
}
