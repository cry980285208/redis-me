# 收藏键功能 - 轻量版实现方案

> **实现状态**：✅ 已实现（轻量版）  
> **关键代码**：`src/utils/favorite.ts`、`KeyTree.vue`、`KeyMain.vue`、`RedisValue.vue`  
> **实际实现**：右键收藏、`toggleFavoriteMode` 底部入口、收藏树与跳转；Phase 2/3 扩展（动画、跨 db 汇总、导入导出）未做。

> 目标：查询键时右键收藏，后续通过"我的收藏"入口快速查看和跳转

---

## 一、需求概述

### 1.1 核心需求

| 需求     | 说明                                        |
| -------- | ------------------------------------------- |
| 右键收藏 | 键列表中右键点击键，选择"收藏" / "取消收藏" |
| 收藏标识 | 已收藏的键在列表中显示星标，一眼识别        |
| 收藏入口 | 底部状态栏增加"⭐ 我的收藏 (N)"入口         |
| 收藏视图 | 点击入口后键列表切换为收藏列表，快速查看    |
| 快速跳转 | 收藏列表中点击键直接查看值                  |

### 1.2 设计原则

- **轻量**：不改动后端，纯前端实现
- **隔离**：按 `connId + db` 隔离收藏数据
- **复用**：尽量复用现有组件和交互模式

---

## 二、数据结构设计

### 2.1 TypeScript 接口

```typescript
// src/utils/favorite.ts
export interface FavoriteKey {
  connId: string // 连接ID（区分不同连接）
  db: number // 数据库编号
  redisKey: RedisKey_Deserialize // 完整键信息（key + bytes）
  favoritedAt: number // 收藏时间戳
}
```

### 2.2 存储方案

```typescript
// src/utils/favorite.ts
import { useStorage } from '@vueuse/core'

const FAVORITE_KEY = 'redis-me:favorites'

export function useFavorites() {
  return useStorage<FavoriteKey[]>(FAVORITE_KEY, [])
}
```

- 使用 `useStorage` 自动同步 `localStorage`
- 存储示例：

```json
[
  {
    "connId": "conn-xxx",
    "db": 0,
    "redisKey": { "key": "user:1001", "bytes": "dXNlcjoxMDAx" },
    "favoritedAt": 1719200000000
  },
  {
    "connId": "conn-xxx",
    "db": 0,
    "redisKey": { "key": "user:1002", "bytes": "dXNlcjoxMDAy" },
    "favoritedAt": 1719200000001
  }
]
```

---

## 三、交互流程设计

### 3.1 整体流程

```
正常扫描键模式：
┌─────────────────────────────────────────────────────────┐
│  ┌───────────────────────────────────────────────────┐│
│  │ 键列表                                             ││
│  │  ├─ [HASH] user:1001   [删除][⭐]  ← 已收藏        ││
│  │  ├─ [STR]  user:1002      [删除]                   ││
│  │  └─ [ZSET] order:2024  [删除][⭐]  ← 已收藏        ││
│  └───────────────────────────────────────────────────┘│
│  ┌───────────────────────────────────────────────────┐│
│  │              15 / 100   （选中/过滤）               ││
│  │ [db0] [加载▼]        [⭐] [多选] [更多▼]          ││
│  └───────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────┘
                            │
                            ▼ 点击"⭐"收藏图标
┌─────────────────────────────────────────────────────────┐
│  ┌───────────────────────────────────────────────────┐│
│  │ 收藏列表                                           ││
│  │  ├─ [HASH] user:1001   [删除][⭐]                  ││
│  │  └─ [ZSET] order:2024  [删除][⭐]                  ││
│  └───────────────────────────────────────────────────┘│
│  ┌───────────────────────────────────────────────────┐│
│  │               2 / 2    （选中/过滤）                ││
│  │ [← 返回扫描]         [⭐] [多选] [更多▼]          ││
│  └───────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────┘
```

### 3.2 右键菜单流程

```
用户右键点击键
      │
      ▼
┌──────────────────────────┐
│ 刷新键                   │
│ 多选模式                 │
│ 新增键                   │
│ 重新加载                 │
│ 复制键名                 │
│ 重命名键                 │
│ ⭐ 收藏键                 │  ← 未收藏时显示（或"❌ 取消收藏"）
│ 删除键                   │
└──────────────────────────┘
      │
      ▼
点击"收藏" → 写入 localStorage → 键前显示星标 → meOk('已收藏')
点击"取消收藏" → 从 localStorage 移除 → 星标消失 → meOk('已取消收藏')
```

### 3.3 收藏列表交互

```
收藏视图下：
  ├─ 搜索框：placeholder 变为"搜索收藏的键..."，输入后按 key 本地过滤
  ├─ 精确匹配复选框：隐藏（收藏列表是本地过滤，不需要精确匹配）
  ├─ 键类型筛选：隐藏（收藏数据已包含 type）
  ├─ 键列表：展示当前 connId + db 的收藏键
  ├─ 右键菜单：
  │   ├─ 取消收藏（替换原"收藏"选项）
  │   ├─ 复制键名
  │   ├─ 查看值（reloadKey）
  │   └─ 删除键（如果 canEdit）
  ├─ 点击键：直接查看值（与正常模式一致）
  └─ 底部栏：第一行显示计数，第二行"← 返回扫描"按钮
```

---

## 四、UI 设计

### 4.1 键列表星标标识

在 `KeyTree.vue` 的键节点右侧增加星标（与删除按钮同一行）：

```
列表模式：
  [HASH]  user:1001   [删除] [⭐]
  [STR]   user:1002   [删除]
  [ZSET]  order:2024  [删除] [⭐]

树形模式：
  [HASH]  user            [删除] [⭐]
            └── 1001
  [STR]   session         [删除]
            └── abc123
```

**布局说明**：删除按钮在左，星标在右，两者保留适当间距

星标样式：

- 颜色：`#f7ba2a`（Element Plus 的 warning 色）
- 位置：键名右侧，与删除按钮在同一行（删除在左，星标在右）
- 大小：与删除按钮一致
- 行为：常驻显示（已收藏时）

### 4.2 底部状态栏（两行布局）

底部状态栏改为两行显示，需要调整 CSS：

```scss
// KeyMain.vue 样式调整
.key-footer {
  height: 50px; // 原 30px → 50px
  display: flex;
  flex-direction: column;

  .footer-row-1 {
    height: 20px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .footer-row-2 {
    height: 30px;
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
}
```

底部状态栏改为两行显示，总高度约 50-56px：

**第一行：选中/过滤状态**

```
┌────────────────────────────────────────────────────────────┐
│                    15 / 100  （选中/过滤状态）              │
└────────────────────────────────────────────────────────────┘
```

**第二行：操作按钮区**

```
正常模式：
┌────────────────────────────────────────────────────────────┐
│ [db0 (15)]  [加载更多]        [⭐]  [多选]  [更多▼]       │
└────────────────────────────────────────────────────────────┘

收藏模式：
┌────────────────────────────────────────────────────────────┐
│ [← 返回扫描]                   [⭐]  [多选]  [更多▼]       │
└────────────────────────────────────────────────────────────┘
```

**布局说明：**

- **第一行**：居中显示选中/过滤数量（`filterKeyList.length / keyList.length`）
- **第二行左侧**：db选择 + 加载按钮（正常模式）/ 返回按钮（收藏模式）
- **第二行右侧**：收藏图标（⭐）+ 多选按钮 + 更多下拉

**收藏图标样式：**

- 仅显示星标图标，无文字
- 有收藏时：`el-icon-star-on`，颜色 `#f7ba2a`
- 无收藏时：`el-icon-star-off`，颜色默认
- hover 提示："我的收藏 (N)" / "我的收藏"

### 4.3 收藏数量提示

- 收藏数量实时计算：过滤出当前 `connId + db` 的收藏键数量
- 数量为 0 时显示空星标 `el-icon-star-off`
- 数量 > 0 时显示实心星标 `el-icon-star-on`，颜色 `#f7ba2a`

### 4.4 空状态

收藏列表为空时：

- 显示文字："暂无收藏键，右键键即可收藏"
- 字体颜色：`var(--el-text-color-secondary)`
- 居中对齐

---

## 五、状态管理

### 5.1 新增状态

在 `KeyMain.vue` 中增加：

```typescript
// 收藏视图模式
const favoriteMode = ref(false)

// 收藏数据（全局）
const favorites = useFavorites()

// 当前连接+db 的收藏键（计算属性）
const currentFavorites = computed(() => {
  if (!share.conn) return []
  return favorites.value.filter(f => f.connId === share.conn!.id && f.db === share.conn!.db)
})

// 收藏数量
const favoriteCount = computed(() => currentFavorites.value.length)

// 是否为收藏视图
const isFavoriteMode = computed(() => favoriteMode.value)
```

### 5.2 状态流转

```
正常扫描模式 ──点击"⭐ 我的收藏"──▶ 收藏视图模式
      │
      │ 切换 db/连接
      ▼
  自动回到正常扫描模式

收藏视图模式 ──点击"← 返回扫描"──▶ 正常扫描模式
```

### 5.3 边界处理

| 场景     | 处理                                                                   |
| -------- | ---------------------------------------------------------------------- |
| 切换 db  | 自动退出收藏模式，回到正常扫描（因为收藏是 db 隔离的）                 |
| 切换连接 | 自动退出收藏模式                                                       |
| 键重命名 | `redisKey.bytes` 不变，`redisKey.key` 展示名自动更新（下次收藏时覆盖） |

---

## 六、文件修改清单

### 6.1 新建文件

| 文件                    | 说明                                        |
| ----------------------- | ------------------------------------------- |
| `src/utils/favorite.ts` | 收藏数据结构的 CRUD 方法、useFavorites hook |

### 6.2 修改文件

| 文件                        | 修改内容                                                                                                                                       |
| --------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `src/views/key/KeyTree.vue` | 1. 右键菜单增加"收藏"/"取消收藏"选项<br>2. 键节点前增加星标标识<br>3. 传入收藏状态（prop 或 inject）                                           |
| `src/views/KeyMain.vue`     | 1. 底部状态栏增加"⭐ 我的收藏 (N)"入口<br>2. 增加收藏视图模式状态管理<br>3. 收藏视图下搜索框 placeholder 联动<br>4. 收藏视图下键列表数据源切换 |
| `src/locales/lang/zh-cn.ts` | 新增 i18n 文案：收藏、取消收藏、我的收藏、返回扫描、暂无收藏键                                                                                 |
| `src/locales/lang/en.ts`    | 同上                                                                                                                                           |

---

## 七、详细实现设计

### 7.1 favorite.ts 工具函数

```typescript
// src/utils/favorite.ts
import { useStorage } from '@vueuse/core'

export interface FavoriteKey {
  connId: string
  db: number
  redisKey: RedisKey_Deserialize
  type?: string
  favoritedAt: number
}

const FAVORITE_KEY = 'redis-me:favorites'

export function useFavorites() {
  return useStorage<FavoriteKey[]>(FAVORITE_KEY, [])
}

export function isFavorited(
  favorites: FavoriteKey[],
  connId: string,
  db: number,
  bytes: string,
): boolean {
  return favorites.some(f => f.connId === connId && f.db === db && f.redisKey.bytes === bytes)
}

export function addFavorite(
  favorites: FavoriteKey[],
  connId: string,
  db: number,
  redisKey: RedisKey_Deserialize,
  type?: string,
): FavoriteKey[] {
  if (isFavorited(favorites, connId, db, redisKey.bytes)) return favorites
  return [...favorites, { connId, db, redisKey, type, favoritedAt: Date.now() }]
}

export function removeFavorite(
  favorites: FavoriteKey[],
  connId: string,
  db: number,
  bytes: string,
): FavoriteKey[] {
  return favorites.filter(f => !(f.connId === connId && f.db === db && f.redisKey.bytes === bytes))
}

export function getFavoritesByConn(
  favorites: FavoriteKey[],
  connId: string,
  db: number,
): FavoriteKey[] {
  return favorites.filter(f => f.connId === connId && f.db === db)
}
```

### 7.2 KeyTree.vue 右键菜单修改

在右键菜单模板中增加：

```vue
<!-- 键节点的右键菜单 -->
<template v-if="contextMenuNode?.isLeaf">
  <!-- ... 现有菜单项 ... -->
  <el-dropdown-item v-if="!isFavorited" command="favoriteKey" divided>
    <me-icon icon="el-icon-star-on" :name="t('keyTree.favoriteKey')" />
  </el-dropdown-item>
  <el-dropdown-item v-else command="unfavoriteKey" divided>
    <me-icon icon="el-icon-star-off" :name="t('keyTree.unfavoriteKey')" />
  </el-dropdown-item>
  <!-- ... -->
</template>
```

### 7.3 KeyTree.vue 星标展示

````vue
<!-- 键节点模板 -->
<div v-if="node.isLeaf" class="me-flex key-leaf-row">
  <div class="me-flex key-leaf-main">
    <KeyTypeTag :redis-key="node.data.redisKey" />
    <div class="key-leaf-label">
      <span>{{ node.label }}</span>
    </div>
  </div>
  <!-- 删除按钮（当前选中键时显示） -->
  <me-icon
    v-if="canEdit && !showCheckbox && isCurrentKey(node)"
    :info="t('keyTree.deleteKey')"
    icon="el-icon-delete"
    class="key-delete-btn"
    @click.stop="quickDeleteKey(node.data.redisKey)" />
  <!-- 收藏星标（与删除按钮在同一行，删除在左，星标在右） -->
  <me-icon
    v-if="isFavorite(node.data.redisKey.bytes)"
    icon="el-icon-star-on"
    style="color: #f7ba2a"
    class="key-favorite-btn"
  />
</div>

**CSS 间距**：
```scss
.key-delete-btn {
  flex-shrink: 0;
  margin-right: 6px; // 删除按钮和星标之间保留间距
  cursor: pointer;
  color: var(--el-color-info);
  &:hover {
    color: var(--el-color-info-light-3);
  }
}

.key-favorite-btn {
  flex-shrink: 0;
  margin-right: 10px;
}
````

````

### 7.4 KeyMain.vue 底部状态栏

```vue
<!-- 正常模式：左侧数据库 + 收藏入口 -->
<div class="me-flex" v-if="!showCheckbox && share.conn">
  <el-select v-model="share.conn.db" @change="selectDB" ...>
    <!-- ... -->
  </el-select>

  <!-- 收藏入口 -->
  <me-icon
    :name="t('keyMain.myFavorites')"
    :icon="favoriteCount > 0 ? 'el-icon-star-on' : 'el-icon-star-off'"
    class="icon-btn"
    :style="{ color: favoriteCount > 0 ? '#f7ba2a' : '' }"
    @click="enterFavoriteMode"
    hint
    placement="top" />

  <!-- 加载更多按钮 -->
  <div v-if="!cursor?.finished">
    <me-icon icon="me-icon-load-more" ... />
    <me-icon icon="me-icon-load-all" ... />
  </div>
</div>

<!-- 收藏模式：左侧返回按钮 -->
<div class="me-flex" v-if="favoriteMode">
  <me-icon
    :name="t('keyMain.backToScan')"
    icon="el-icon-back"
    class="icon-btn"
    @click="exitFavoriteMode"
    hint
    placement="top" />
</div>
````

### 7.5 KeyMain.vue 收藏视图逻辑

```typescript
// 进入收藏视图
function enterFavoriteMode(): void {
  favoriteMode.value = true
}

// 退出收藏视图
function exitFavoriteMode(): void {
  favoriteMode.value = false
}

// 收藏视图下的键列表数据
const displayKeyList = computed(() => {
  if (!favoriteMode.value) return filterKeyList.value

  // 从收藏数据构建 RedisKey_Deserialize 列表
  const favKeys = getFavoritesByConn(favorites.value, share.conn!.id, share.conn!.db)
  return favKeys.map(f => f.redisKey)
})
```

---

## 八、i18n 文案

### 8.1 zh-cn.ts

```typescript
keyTree: {
  favoriteKey: '收藏键',
  unfavoriteKey: '取消收藏',
},
keyMain: {
  myFavorites: '我的收藏',
  backToScan: '返回扫描',
  favoriteCount: '我的收藏 ({count})',
  noFavorites: '暂无收藏键，右键键即可收藏',
}
```

### 8.2 en.ts

```typescript
keyTree: {
  favoriteKey: 'Favorite',
  unfavoriteKey: 'Unfavorite',
},
keyMain: {
  myFavorites: 'My Favorites',
  backToScan: 'Back to Scan',
  favoriteCount: 'My Favorites ({count})',
  noFavorites: 'No favorites yet. Right-click a key to favorite.',
}
```

---

## 九、边界情况处理

| 场景         | 处理方案                                                                         |
| ------------ | -------------------------------------------------------------------------------- |
| 键重命名     | `redisKey.bytes` 不变，`redisKey.key` 展示名自动更新（下次收藏时覆盖）           |
| 连接断开     | 收藏数据保留在 localStorage，不影响                                              |
| 跨 db 收藏   | 按 `connId + db` 隔离，切换 db 后收藏列表自动更新                                |
| 收藏数据量大 | localStorage 有 5MB 限制，单连接收藏键超过 10 万条时可能溢出，轻量场景下不会发生 |
| 键类型缓存   | 收藏时记录 type，展示时使用缓存的 type                                           |

---

## 十、实施步骤

### Phase 1：核心功能

- [x] 新建 `src/utils/favorite.ts` - 收藏数据结构和 CRUD
- [x] 修改 `src/views/key/KeyTree.vue` - 右键菜单增加收藏/取消收藏、键节点星标
- [x] 修改 `src/views/KeyMain.vue` - 底部收藏入口、收藏视图模式
- [x] 新增 i18n 文案

### Phase 2：完善体验

- [ ] 收藏数量动画（收藏/取消时的过渡效果）
- [x] 收藏列表支持搜索过滤（收藏视图 + 本地 filter）

### Phase 3：可选扩展

- [ ] 收藏键跨 db 查看（显示所有 db 的收藏）
- [ ] 收藏键导入/导出
- [ ] 收藏键排序（按收藏时间、字母顺序）

---

## 十一、与其他功能的兼容性

| 功能       | 收藏视图下的行为                                      |
| ---------- | ----------------------------------------------------- |
| 多选模式   | 收藏列表也支持多选，可以批量删除收藏的键              |
| 搜索过滤   | 收藏列表支持本地搜索过滤                              |
| 键类型筛选 | 收藏视图下隐藏类型筛选下拉，因为收藏数据已包含 type   |
| 精确匹配   | 收藏视图下隐藏精确匹配复选框，收藏列表按 key 本地过滤 |
| 加载更多   | 收藏视图下隐藏"加载更多"按钮，收藏列表一次性展示全部  |
| 刷新键     | 收藏视图下刷新无效果（不走 SCAN）                     |

---

_方案文档创建于 2026-06-24_
