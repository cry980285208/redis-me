/** Redis 展示元数据：键类型 Tag/简称，以及集群节点列表 enrich */
import { sortBy } from 'lodash'

import i18n from '@/locales'
import type { EnrichedRedisNode, KeyTypeListItem } from '@/types/me-interface'
import type { RedisNode } from '@/types/tauri-specta'

const t = i18n.global.t

export const KEY_TYPE_LIST: KeyTypeListItem[] = [
  { short: 'S', value: 'STRING', type: 'primary' },
  { short: 'H', value: 'HASH', type: 'primary' },
  { short: 'L', value: 'LIST', type: 'danger' },
  { short: 'E', value: 'SET', type: 'danger' },
  { short: 'Z', value: 'ZSET', type: 'danger' },
  { short: 'X', value: 'STREAM', type: 'warning' },
  { short: 'J', value: 'JSON', type: 'warning' },
  { short: 'A', value: 'ARRAY', type: 'warning' }, // Redis 8.8 Array；色同 JSON/Stream
]

const keyTypeMap = new Map(KEY_TYPE_LIST.map(item => [item.value, item.type]))
const keyShortMap = new Map(KEY_TYPE_LIST.map(item => [item.value, item.short]))

/** 键类型：el-text, el-tag 的 type */
export function meType(keyType: string | undefined | null): string {
  if (!keyType) return 'info'
  return keyTypeMap.get(keyType?.toUpperCase() ?? '') || 'info'
}

/** 键类型短：避免 String、Set 的简称都是 S */
export function meKeyShort(keyType: string | undefined | null, defaultValue = '?'): string {
  if (!keyType) return defaultValue
  return keyShortMap.get(keyType?.toUpperCase() ?? '') || defaultValue
}

/**
 * 将 node_list 接口数据排序并补充与 UI 一致的字段。
 * 展示顺序：M1/M2/… 在上，同编号的 S1/S2/… 在下。
 */
export function enrichNodeList(rawList: RedisNode[] | null | undefined): EnrichedRedisNode[] {
  if (!rawList?.length) return []
  const sorted = sortBy(rawList, 'node') as EnrichedRedisNode[]

  let masterIndex = 0
  const masterMap = new Map<string, { idx: number; slots: string | null }>()
  sorted.forEach(item => {
    item.isMaster = item.flags?.includes('master') ?? false
    item.isSlave = item.flags?.includes('slave') ?? false
    if (item.isMaster) {
      masterIndex++
      item.shortLabel = 'M' + masterIndex
      masterMap.set(item.node, { idx: masterIndex, slots: item.slots })
    }
  })
  sorted.forEach(item => {
    if (item.isSlave && item.slaveOfNode) {
      const master = masterMap.get(item.slaveOfNode)
      if (master) {
        item.shortLabel = 'S' + master.idx
        item.masterSlots = master.slots
      }
    }

    if (item.isMaster && item.slots) {
      item.slotsTooltip = t('nodeList.slotsTooltip', { slots: item.slots })
    } else if (item.isSlave && item.masterSlots) {
      item.slotsTooltip = t('nodeList.slotsReplicaTooltip', { slots: item.masterSlots })
    } else {
      item.slotsTooltip = ''
    }

    if (!item.shortLabel) {
      item.shortLabel = item.flags?.slice(0, 1).toUpperCase() || 'F'
    }
  })

  return sorted.sort((a, b) => {
    const rank = (item: EnrichedRedisNode) => (item.isMaster ? 0 : item.isSlave ? 1 : 2)
    const num = (label: string) => {
      const m = /^[MS](\d+)$/.exec(label)
      return m ? Number(m[1]) : 999
    }
    return (
      rank(a) - rank(b) || num(a.shortLabel) - num(b.shortLabel) || a.node.localeCompare(b.node)
    )
  })
}
