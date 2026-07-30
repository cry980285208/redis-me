<script setup lang="ts">
import type { TreeNode } from 'element-plus/es/components/tree-v2/src/types'
import { nanoid } from 'nanoid'
import { computed, inject, nextTick, ref, useTemplateRef, watch } from 'vue'
// 共享数据
import { useI18n } from 'vue-i18n'

import { shareProvideKey } from '@/types/me-interface'
import type { RedisKey_Deserialize } from '@/types/tauri-specta'
import { sameRedisKey } from '@/utils/redis-key'
import { meDeleteKey, TREE_KEY_ID_PREFIX } from '@/utils/util'

import KeyTypeTag from './KeyTypeTag.vue'

const { t } = useI18n()
const share = inject(shareProvideKey)!
const canEdit = computed(() => !share.readonly)

defineExpose({ setCurrentKey })
const emit = defineEmits([
  'chooseKey',
  'chooseFolder',
  'contextKey',
  'contextFolder',
  'checkChange',
  'favoriteKey',
  'unfavoriteKey',
])
const props = withDefaults(
  defineProps<{
    color?: string
    redisKey?: RedisKey_Deserialize | null
    filterKeyList?: RedisKey_Deserialize[]
    showCheckbox?: boolean
    keyShowTree?: boolean
    sortByCount?: boolean
    loading?: boolean
    favorites?: RedisKey_Deserialize[]
    favoriteMode?: boolean
  }>(),
  {
    color: 'var(--el-color-primary)',
    redisKey: null,
    filterKeyList: () => [],
    showCheckbox: false,
    keyShowTree: true,
    sortByCount: true,
    favorites: () => [],
    favoriteMode: false,
  },
)

/** 本地构建的树节点（文件夹 / 键叶子） */
interface KeyBuildNode {
  id: string
  label: string
  children: KeyBuildNode[]
  redisKey?: RedisKey_Deserialize
  keyCount?: number
  isRootNode?: boolean
}

// 左键点击
function nodeClick(_data: unknown, node: TreeNode) {
  if (node.isLeaf) {
    emit('chooseKey', node.data.redisKey)
  } else {
    emit('chooseFolder', node.key)
  }
}

// 右键点击
const contextMenuNode = ref<TreeNode | null>(null)
const meContextRef = useTemplateRef('meContextRef')

function nodeContextMenu(e: MouseEvent, _data: unknown, node: TreeNode) {
  // db0根节点不显示上下文
  if (node.data.isRootNode) return
  contextMenuNode.value = node
  meContextRef.value?.showMenu(e)
}

function handleCommand(command: string) {
  const ctx = contextMenuNode.value
  if (!ctx) return
  if (ctx.isLeaf) {
    const redisKey = ctx.data.redisKey as RedisKey_Deserialize
    emit('contextKey', command, redisKey)
  } else {
    const folder = ctx.key
    emit('contextFolder', command, folder)
  }
}

function handleClose() {
  contextMenuNode.value = null
}

// 右键选中的键, 加入样式
function getNodeClass(node: TreeNode) {
  const clazz = []
  if (
    (node.isLeaf && node.data.redisKey?.key === contextMenuNode.value?.data?.redisKey?.key) ||
    (!node.isLeaf && node.key === contextMenuNode.value?.key)
  ) {
    clazz.push('context-key')
  }
  return clazz
}

// 计算树的数据
const emptyText = computed(() =>
  props.filterKeyList.length === 0 && !props.loading ? t('keyTree.noData') : t('keyMain.scanning'),
)
const treeData = computed(() => {
  // 列表展示
  if (!props.keyShowTree) {
    return buildList(props.filterKeyList)
  }

  // 树形展示
  const root = buildTree(props.filterKeyList)
  root.forEach(node => countLeaves(node))

  // 根节点排序及其子节点排序
  root.sort((n1, n2) => nodesSort(n1, n2))
  root.forEach(node => sortNodeChildrenLoop(node))
  return root
})

// 循环方式排序节点的子节点（避免递归栈溢出）
function sortNodeChildrenLoop(rootNode: KeyBuildNode) {
  // 初始化一个栈，将根节点压入栈中
  const stack = [rootNode]
  while (stack.length > 0) {
    // 取出栈顶节点
    const node = stack.pop()
    if (node === undefined) continue
    if (node.children && node.children.length > 0) {
      // 对当前节点的子节点进行排序
      node.children.sort((n1, n2) => nodesSort(n1, n2))
      // 将所有子节点压入栈中，以便后续处理
      node.children.forEach(child => stack.push(child))
    }
  }
}

function nodesSort(n1: KeyBuildNode, n2: KeyBuildNode) {
  let cmp: number
  if (props.sortByCount) {
    // 文件夹在上面，叶子在下面（将叶子节点的数量归零，避免和只有1个键的文件夹混在一起）
    const n1Count: number = n1.children.length > 0 ? (n1.keyCount ?? 0) : 0
    const n2Count: number = n2.children.length > 0 ? (n2.keyCount ?? 0) : 0
    cmp = n2Count - n1Count
  } else {
    // 保存文件夹在上面，叶子在下面（文件夹的数量为都设置为1）
    const n1Count: number = n1.children.length > 0 ? 1 : 0
    const n2Count: number = n2.children.length > 0 ? 1 : 0
    cmp = n2Count - n1Count
  }
  // 键数量或文件夹/叶子分组相同时按 id 排序
  return cmp === 0 ? (n2.id > n1.id ? -1 : 1) : cmp
}

// 显示复选框时补充根节点
const rootId = nanoid() + Date.now()
const treeRef = useTemplateRef('tree')
/** 用户手动展开的节点 id；同步到 defaultExpandedKeys，data 刷新时避免先折叠再恢复 */
const expandedKeys = ref<string[]>([])
const defaultExpandedKeys = computed(() => {
  if (props.showCheckbox) {
    return expandedKeys.value.length > 0 ? [...new Set([rootId, ...expandedKeys.value])] : [rootId]
  }
  return [...expandedKeys.value]
})

function onNodeExpand(_data: unknown, node: TreeNode) {
  const key = String(node.key)
  if (!expandedKeys.value.includes(key)) {
    expandedKeys.value = [...expandedKeys.value, key]
  }
}

/** 判断 key 是否属于 folderKey 文件夹或其子树（含叶子键） */
function isUnderFolder(key: string, folderKey: string): boolean {
  if (key === folderKey) return true
  if (key.startsWith(folderKey + ':')) return true
  if (key.startsWith(TREE_KEY_ID_PREFIX)) {
    const redisKey = key.slice(TREE_KEY_ID_PREFIX.length)
    return redisKey === folderKey || redisKey.startsWith(folderKey + ':')
  }
  return false
}

function onNodeCollapse(_data: unknown, node: TreeNode) {
  const key = String(node.key)
  // 折叠父节点时子节点不会触发 collapse，需一并移除，否则刷新后会因 setExpandedKeys 沿父链展开而“弹回”
  if (key === rootId) {
    expandedKeys.value = []
    return
  }
  expandedKeys.value = expandedKeys.value.filter(k => !isUnderFolder(k, key))
}

watch(
  () => [props.showCheckbox, props.filterKeyList],
  () => {
    treeRef.value?.setCheckedKeys([])
  },
)
const rootTreeData = computed((): KeyBuildNode[] => {
  if (props.showCheckbox) {
    return [
      {
        id: rootId,
        label: 'db' + String(share.conn?.db ?? ''),
        children: treeData.value as KeyBuildNode[],
        keyCount: props.filterKeyList.length || 0,
        isRootNode: true,
      },
    ]
  }
  return treeData.value as KeyBuildNode[]
})

// 构建树：同层文件夹用 Map 查找，避免 find 线性扫描
function buildTree(keyList: RedisKey_Deserialize[]) {
  const root: KeyBuildNode[] = []
  /** 每层 children 数组 → label → 文件夹节点（不含叶子） */
  const folderMaps = new WeakMap<KeyBuildNode[], Map<string, KeyBuildNode>>()

  function folderMapOf(level: KeyBuildNode[]): Map<string, KeyBuildNode> {
    let m = folderMaps.get(level)
    if (!m) {
      m = new Map()
      folderMaps.set(level, m)
    }
    return m
  }

  keyList.forEach(rk => {
    const parts = rk.key.split(/:+/)
    let nowLevel = root
    parts.forEach((part, index) => {
      // 叶子节点：hepengju 这种无分隔符的键直接作为叶子
      if (index === parts.length - 1) {
        const label = part
        let node = { id: TREE_KEY_ID_PREFIX + rk.key, label, children: [], redisKey: rk }
        nowLevel.push(node)
        return
      }

      // hepengju: / hepengju:string
      const folders = folderMapOf(nowLevel)
      let node = folders.get(part)
      if (!node) {
        // 避免叶子节点的 id 与部分非叶子节点一致
        node = { id: parts.slice(0, index + 1).join(':'), label: part, children: [] }
        nowLevel.push(node)
        folders.set(part, node)
      }
      nowLevel = node.children
    })
  })
  return root
}

// 统计叶子节点个数: 循环方式（豆包）  ==> 递归方式在数据量比较大时会栈溢出
function countLeaves(node: KeyBuildNode) {
  // 初始化一个栈，将根节点压入栈中
  const stack = [node]
  // 用于存储每个节点的叶子节点数量
  const keyCounts = new Map()

  while (stack.length > 0) {
    // 取出栈顶节点
    const nowNode = stack[stack.length - 1]

    // 如果当前节点的所有子节点都已经处理过
    if (nowNode.children.every(child => keyCounts.has(child))) {
      // 弹出栈顶节点
      stack.pop()
      if (nowNode.children.length === 0) {
        // 如果是叶子节点，叶子数量为 1
        keyCounts.set(nowNode, 1)
      } else {
        // 计算当前节点的叶子节点数量，等于所有子节点叶子节点数量之和
        let keyCount = 0
        nowNode.children.forEach(child => {
          keyCount += keyCounts.get(child)
        })
        keyCounts.set(nowNode, keyCount)
      }
      // 将计算好的叶子节点数量赋值给节点的 keyCount 属性
      nowNode.keyCount = keyCounts.get(nowNode)
    } else {
      // 如果当前节点的子节点还有未处理的，将未处理的子节点压入栈中
      nowNode.children.forEach(child => {
        if (!keyCounts.has(child)) {
          stack.push(child)
        }
      })
    }
  }
  // 返回根节点的叶子节点数量
  return keyCounts.get(node)
}

// 构建树: 仅仅叶子节点（即List显示）
function buildList(keyList: RedisKey_Deserialize[]) {
  return keyList.map(rk => ({
    id: TREE_KEY_ID_PREFIX + rk.key,
    label: rk.key,
    children: [],
    redisKey: rk,
  }))
}

// 获取选中的节点键
function checkChange() {
  const nodes = (treeRef.value?.getCheckedNodes(true) ?? []) as KeyBuildNode[]
  const redisKeys = nodes.map(n => n.redisKey).filter((k): k is RedisKey_Deserialize => k != null)
  emit('checkChange', redisKeys)
}

// 设置选中节点并滚动到视口中间（新建键 / 键值页定位复用）
function setCurrentKey(redisKey: RedisKey_Deserialize) {
  const nodeId = TREE_KEY_ID_PREFIX + redisKey.key

  // 展开父节点并同步 expandedKeys，等 flatten 更新后再 scroll
  const parts = redisKey.key.split(/:+/)
  const parentIds: string[] = []
  for (let i = 0; i < parts.length - 1; i++) {
    parentIds.push(parts.slice(0, i + 1).join(':'))
  }
  if (parentIds.length > 0) {
    const nextExpanded = [...expandedKeys.value]
    let changed = false
    for (const parentId of parentIds) {
      if (!nextExpanded.includes(parentId)) {
        nextExpanded.push(parentId)
        changed = true
      }
    }
    if (changed) expandedKeys.value = nextExpanded
  }

  nextTick(() => {
    treeRef.value?.scrollToNode(nodeId, 'center')
    treeRef.value?.setCurrentKey(nodeId)
  })
}

// 键高度配置
const keyHeight = computed(() => meTauri.settings.keyHeight ?? 20)

function quickDeleteKey(redisKey: RedisKey_Deserialize): void {
  if (!share.conn) return
  meDeleteKey(share.conn.id, redisKey)
}

const favoritedKeyList = computed(() => props.favorites)

function isFavoritedLocal(redisKey: RedisKey_Deserialize | undefined): boolean {
  if (!redisKey) return false
  return favoritedKeyList.value.some(f => sameRedisKey(f, redisKey))
}

const isContextNodeFavorited = computed(() => {
  if (!contextMenuNode.value?.isLeaf) return false
  return isFavoritedLocal(contextMenuNode.value.data.redisKey)
})
</script>

<template>
  <el-auto-resizer>
    <template #default="{ height }">
      <el-tree-v2
        ref="tree"
        :data="rootTreeData"
        :default-expanded-keys="defaultExpandedKeys"
        @check-change="checkChange"
        @node-click="nodeClick"
        @node-expand="onNodeExpand"
        @node-collapse="onNodeCollapse"
        @node-contextmenu="nodeContextMenu"
        highlight-current
        :style="{
          '--el-text-color-regular': color,
          '--el-tree-node-hover-bg-color': 'var(--el-color-info-light-8)',
        }"
        :empty-text="emptyText"
        :height="height"
        :item-size="keyHeight"
        :show-checkbox="showCheckbox">
        <template #default="{ node }">
          <!-- 长名截断：左侧文字 ellipsis，右侧 [数量]/操作图标固定不挤出 -->
          <div v-if="node.isLeaf" :class="getNodeClass(node)" class="me-flex key-leaf-row">
            <div
              class="me-flex key-leaf-main"
              :class="{ 'list-key': !keyShowTree && !showCheckbox }">
              <KeyTypeTag :redis-key="node.data.redisKey" />
              <div class="key-leaf-label">
                <span v-if="node.label">{{ node.label }}</span>
                <span v-else style="color: var(--el-color-info-light-3)">[EMPTY]</span>
              </div>
            </div>
            <div class="key-leaf-actions">
              <me-icon
                v-if="canEdit && !showCheckbox && !favoriteMode"
                :info="t('keyTree.deleteKey')"
                icon="el-icon-delete"
                class="key-delete-btn"
                @click.stop="quickDeleteKey(node.data.redisKey)" />
              <me-icon
                v-if="isFavoritedLocal(node.data.redisKey)"
                icon="el-icon-star-filled"
                style="color: #f7ba2a"
                class="key-favorite-btn"
                @click.stop="emit('contextKey', 'unfavoriteKey', node.data.redisKey)" />
            </div>
          </div>
          <div v-else :class="getNodeClass(node)" class="me-flex folder-row">
            <me-icon
              class="folder-icon"
              :icon="
                node.data.isRootNode
                  ? 'me-icon-db'
                  : node.expanded
                    ? 'el-icon-folderOpened'
                    : 'el-icon-folder'
              " />
            <div class="folder-label">{{ node.label }}</div>
            <div class="folder-count">[ {{ node.data.keyCount }} ]</div>
          </div>
        </template>
      </el-tree-v2>

      <!-- 右键菜单 -->
      <me-context ref="meContextRef" @handle-command="handleCommand" @handle-close="handleClose">
        <template v-if="contextMenuNode?.isLeaf">
          <!-- 收藏模式下的右键菜单 -->
          <template v-if="favoriteMode">
            <el-dropdown-item command="unfavoriteKey"
              ><me-icon icon="el-icon-star-filled" :name="t('keyTree.unfavoriteKey')"
            /></el-dropdown-item>
            <el-dropdown-item command="copyKey"
              ><me-icon icon="el-icon-document-copy" :name="t('keyTree.copyKey')"
            /></el-dropdown-item>
            <el-dropdown-item v-if="!showCheckbox" command="checkedMode"
              ><me-icon icon="me-icon-checked" :name="t('keyMain.checkedMode')"
            /></el-dropdown-item>
            <el-dropdown-item v-if="showCheckbox" command="exitCheckedMode"
              ><me-icon icon="el-icon-circle-close" :name="t('keyMain.exitCheckedMode')"
            /></el-dropdown-item>
          </template>
          <!-- 正常模式下的右键菜单 -->
          <template v-else>
            <el-dropdown-item command="addKey" v-if="canEdit"
              ><me-icon icon="el-icon-circle-plus" :name="t('keyTree.addKey')"
            /></el-dropdown-item>
            <el-dropdown-item command="copyKey"
              ><me-icon icon="el-icon-document-copy" :name="t('keyTree.copyKey')"
            /></el-dropdown-item>
            <!-- 使用频率低，改在值区扩展菜单提供 -->
            <!--
            <el-dropdown-item v-if="canEdit" command="renameKey"
              ><me-icon icon="el-icon-edit" :name="t('keyList.renameKey')"
            /></el-dropdown-item>
            <el-dropdown-item v-if="canEdit" command="duplicateKey"
              ><me-icon icon="el-icon-copy-document" :name="t('redisValue.duplicateKey')"
            /></el-dropdown-item>
            -->
            <el-dropdown-item v-if="!showCheckbox" command="checkedMode"
              ><me-icon icon="me-icon-checked" :name="t('keyMain.checkedMode')"
            /></el-dropdown-item>
            <el-dropdown-item v-if="showCheckbox" command="exitCheckedMode"
              ><me-icon icon="el-icon-circle-close" :name="t('keyMain.exitCheckedMode')"
            /></el-dropdown-item>
            <el-dropdown-item
              :command="isContextNodeFavorited ? 'unfavoriteKey' : 'favoriteKey'"
              divided>
              <me-icon
                icon="el-icon-star-filled"
                :name="
                  isContextNodeFavorited ? t('keyTree.unfavoriteKey') : t('keyTree.favoriteKey')
                " />
            </el-dropdown-item>
          </template>
        </template>
        <template v-else>
          <template v-if="favoriteMode">
            <el-dropdown-item command="copyFolder"
              ><me-icon icon="el-icon-document-copy" :name="t('keyTree.copyFolder')"
            /></el-dropdown-item>
            <el-dropdown-item v-if="!showCheckbox" command="checkedMode"
              ><me-icon icon="me-icon-checked" :name="t('keyMain.checkedMode')"
            /></el-dropdown-item>
            <el-dropdown-item v-if="showCheckbox" command="exitCheckedMode"
              ><me-icon icon="el-icon-circle-close" :name="t('keyMain.exitCheckedMode')"
            /></el-dropdown-item>
          </template>
          <template v-else>
            <el-dropdown-item command="addKey" v-if="canEdit"
              ><me-icon icon="el-icon-circle-plus" :name="t('keyTree.addKey')"
            /></el-dropdown-item>
            <el-dropdown-item command="copyFolder"
              ><me-icon icon="el-icon-document-copy" :name="t('keyTree.copyFolder')"
            /></el-dropdown-item>
            <el-dropdown-item v-if="!showCheckbox" command="checkedMode"
              ><me-icon icon="me-icon-checked" :name="t('keyMain.checkedMode')"
            /></el-dropdown-item>
            <el-dropdown-item v-if="showCheckbox" command="exitCheckedMode"
              ><me-icon icon="el-icon-circle-close" :name="t('keyMain.exitCheckedMode')"
            /></el-dropdown-item>
            <el-dropdown-item command="loadFolder" divided
              ><me-icon icon="el-icon-search" :name="t('keyTree.loadFolder')"
            /></el-dropdown-item>
            <el-dropdown-item command="memoryUsage"
              ><me-icon icon="me-icon-memory" :name="t('keyTree.memoryUsage')"
            /></el-dropdown-item>
            <el-dropdown-item command="exportFolder" divided :disabled="share.exportImporting"
              ><me-icon icon="el-icon-upload" :name="t('keyTree.exportFolder')"
            /></el-dropdown-item>

            <el-dropdown-item
              command="deleteFolder"
              v-if="canEdit"
              :disabled="share.exportImporting"
              ><me-icon icon="el-icon-delete" :name="t('keyTree.deleteFolder')"
            /></el-dropdown-item>
          </template>
        </template>
      </me-context>
    </template>
  </el-auto-resizer>
</template>

<style scoped lang="scss">
/* 高亮当前行的颜色 */
:deep(.el-tree--highlight-current .el-tree-node.is-current > .el-tree-node__content) {
  background-color: var(--el-color-info-light-8);
}

/* 自定义节点可收缩，长名才能 ellipsis，右侧数量/图标不被挤出 */
:deep(.el-tree-node__content) {
  overflow: hidden;
}

/* 右键选中的键 */
:deep(.context-key) {
  outline: 1px dashed var(--el-color-primary);
  outline-offset: 1px;
}

/* 列表展示时左侧空白处理 */
.list-key {
  margin-left: -20px;
}

/* 占满 content 剩余宽度（勿用 width:100%，会和展开图标叠宽溢出） */
.key-leaf-row,
.folder-row {
  flex: 1;
  min-width: 0;
  align-items: center;
  overflow: hidden;
}

.key-leaf-main {
  flex: 1;
  min-width: 0;
  align-items: center;
  justify-content: flex-start;
  overflow: hidden;
}

.key-leaf-main :deep(.el-tag) {
  flex-shrink: 0;
}

.key-leaf-label,
.folder-label {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.key-leaf-label {
  margin-left: 5px;
}

.folder-icon {
  flex-shrink: 0;
}

.folder-label {
  margin: 0 5px;
}

/* 数量区固定右侧；操作图标右缘大致对齐到 [ n ] 的最后一个数字 */
.folder-count {
  flex-shrink: 0;
  margin-right: 10px;
  color: var(--el-color-info);
}

.key-leaf-actions {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 5px;
  margin-right: 15px;
}

/* 删除图标：hover 行时显示 */
:deep(.el-tree-node__content:hover) .key-delete-btn {
  visibility: visible;
}

.key-delete-btn {
  flex-shrink: 0;
  visibility: hidden;
  cursor: pointer;
  color: var(--el-color-info);

  :deep(.el-icon) {
    color: inherit;
  }

  &:hover {
    color: var(--el-color-info-light-3);
  }
}

/* 收藏星标图标 */
.key-favorite-btn {
  flex-shrink: 0;
  font-size: 14px;
}

/*  键类型TAG设置 */
:deep(.el-tag--small) {
  height: 14px;
  width: 14px;
  padding: 0 4px;
  font-size: 10px;
}
</style>
