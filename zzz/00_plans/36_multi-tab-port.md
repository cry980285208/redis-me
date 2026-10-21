# 36. 多窗口（多连接 TAB + 多键值 TAB）移植说明

> **类型**：功能设计 + 上游合并指南
> **基线**：`upstream/master` @ `0860ddd1`（v5.4.3 之后）
> **分支**：`feature/multi-tab-conn-key-v2`
> **来源**：`27d4d229`（5.2.0 时期的 fork 版本）→ 重新提取、重新组织后移植
> **日期**：2026-10-21

---

## 一、为什么要重做（历史教训）

原 fork 分支 `feature/multi-tab-conn-key` 是「在 5.2.0 基础上改」+「合并 5.4.3」两步走的结果，
合并提交 `c6151714` 的冲突解决把全局状态 `share` 拆成了**两份**：

| 文件                | 合并后的实际内容                                                              | 后果                                             |
| ------------------- | ----------------------------------------------------------------------------- | ------------------------------------------------ |
| `src/App.vue`       | 上游 5.4.3 版本（`provide` 的 share **不含**多TAB 字段）                       | 所有子组件 inject 到的是这份                    |
| `src/views/AppMain.vue` | fork 版本（自己 `reactive` 了一份**含**多TAB 字段，且 `provide` 被删）        | `AppMain` 操作的是另一份，谁也看不见            |

结果 `share.openConns` / `share.openKeys` 在子组件里是 `undefined`，
`ConnTabStrip` 的 `share.openConns.find()` 和 `RedisValue` 的 `share.openKeys.filter()`
直接抛 `TypeError`，多连接窗口与多键值窗口整体消失。

**本分支的做法**：不再在旧 fork 上打补丁，而是**以干净的上游 5.4.3 为基线重新移植**，
并把「冲突面」压到最小——见下一节。

---

## 二、文件结构

### 新增文件（上游没有，合并上游时**零冲突面**）

| 文件                                       | 职责                                                          |
| ------------------------------------------ | ------------------------------------------------------------- |
| `src/utils/conn-tabs.ts`                    | 多连接 TAB 状态机：连接常驻、打开/关闭、能力缓存、兜底断开     |
| `src/utils/key-tabs.ts`                     | 多键值 TAB 状态机：TAB 增删切换、切连接自动激活、KEY_OPEN_TAB |
| `src/views/ConnTabStrip.vue`                | 右侧值区顶部的连接 TAB 条（纯 UI）                             |
| `src/views/tab/RedisValue/KeyTabStrip.vue`  | 值区顶部的键值 TAB 条（纯 UI）                                 |

### 改动的上游文件（改动量小且集中）

| 文件                                    | 改动                                                                 |
| --------------------------------------- | -------------------------------------------------------------------- |
| `src/App.vue`                           | `share` 增加 4 个多TAB 字段（+11 行）                               |
| `src/types/me-interface.ts`             | `AppMainShare` 加 4 个字段、`ConnUiInject` 加 3 个方法（+12 行）     |
| `src/utils/util.ts`                     | 新增 `KEY_OPEN_TAB` 事件类型与常量（+3 行）                          |
| `src/locales/lang/{zh-cn,en}.ts`       | 6 条文案（+12 行）                                                  |
| `src/views/AppMain.vue`                 | 接入状态机、连接条、模板套一层（+55 / -13 行）                       |
| `src/views/KeyHeader.vue`               | 顶部连接下拉 / 关闭连接 → 走 TAB（+12 / -3 行）                     |
| `src/views/TabConn.vue`                 | 点连接 → 打开 TAB；清空连接 → 关掉所有 TAB（+3 / -1 行）             |
| `src/views/KeyMain.vue`                 | 选键 → 发 `KEY_OPEN_TAB`；搜索历史按连接存储（+40 / -25 行）         |
| `src/views/tab/RedisMemory.vue`         | 选键 → 发 `KEY_OPEN_TAB`（+3 / -3 行）                              |
| `src/views/tab/RedisValue/index.vue`    | 只接入键值 TAB 条 + 重建后恢复值（+26 / -2 行）                     |

> 对比：旧 fork 版本在 `RedisValue/index.vue` 里插了 **307 行**、在 `AppMain.vue` 里改了 **175 行**，
> 这两个文件恰好是上游改动最频繁的地方 → 每次合并必冲突。本分支把它们压到 26 行 / 55 行，
> 且逻辑全在新文件里。

### 不需要改动的部分

- **Rust 端零改动**：上游 `AppState.clients` 本来就是按 `conn.id` 缓存的，天然支持多连接；
  旧 fork 的 5.2.0 版本也没动过 `src-tauri`。
- 旧的「新建窗口」菜单项（`openNewWindow`）**保留**，多TAB 是增量能力，不替代上游功能。

---

## 三、核心数据流

### 3.1 多连接 TAB

```
用户点连接(TabConn) / 顶部下拉(KeyHeader)
        │
        ▼
connUi.openConnTab(conn)            ← connUi 由 AppMain provide
        │
        ▼
useConnTabs.openConnTab()           src/utils/conn-tabs.ts
        ├─ 已在列表 → 只切 activeConnId + share.conn（不重连）
        ├─ 超过 10 → 提示「最多打开 10 个连接」
        └─ 首次打开 → ensureConnected() 先 connect 成功后入列
        │
        ▼
share.conn = conn  ──► AppMain 的 watch(share.conn)
        │                   ├─ connPrepared = false（重建键区/值区）
        │                   ├─ capabilities 从缓存取（不重连）
        │                   └─ connPrepared = true
        ▼
ConnTabStrip 渲染（读 share.openConns / share.activeConnId）
```

关闭：点 TAB 上的 `×` → `connUi.closeConnTab(id)` → 从 `openConns` 移除 + `disconnect` +
清该连接的键类型/内存占用缓存 → 若关的是激活项，切到右侧（无则左侧）邻居。

### 3.2 多键值 TAB

```
用户点键(KeyMain) / 点内存条目(RedisMemory)
        │
        ▼
bus.emit(KEY_OPEN_TAB, { connId, redisKey })
        │
        ▼
useKeyTabs 的监听（挂在常驻的 AppMain 上）  src/utils/key-tabs.ts
        │
        ▼
openKeyTab(share, connId, redisKey)
        ├─ 同名键已开 → 只更新快照 + 激活
        ├─ 超过 10 → 提示「最多打开 10 个键值」（按「当前连接+库」算）
        └─ 新键 → push 到 share.openKeys 并激活
        │
        ▼
activate() → share.activeKeyId / share.redisKey / share.tabName='value'
           → bus.emit(KEY_REFRESH) → RedisValue 加载该键
```

切连接/切库时，`useKeyTabs` 的 `watch([conn.id, conn.db])` 会自动激活新连接/库的第一个 TAB
（所有 TAB 数据都保留在全局 `share` 里，切回来还在）；没有 TAB 则清空值区。

---

## 四、关键设计约定（以后合并上游必须遵守）

1. **`share` 全局只能有一份**，由 `src/App.vue` 创建并 `provide`。
   任何组件都**只能** `inject(shareProvideKey)`，**禁止**再 `reactive()` 一份出来。
   → 这正是上一次合并出事的根因。
2. **多TAB 的 4 个字段加在 `App.vue` 的 `share` 初始化里**：
   `openConns` / `activeConnId` / `openKeys` / `activeKeyId`。
3. **功能逻辑一律放新文件**（`src/utils/conn-tabs.ts`、`src/utils/key-tabs.ts`），
   上游文件只留「注入 + 转发 + 模板挂载」，每处改动控制在 10 行以内。
4. **状态机挂在常驻组件上**：键值 TAB 状态机挂 `AppMain`（`RedisValue` 会因切连接被销毁重建，
   状态机若挂在它身上，切连接时「自动激活 / 接收 KEY_OPEN_TAB」就会丢）。
5. **Rust `connect` 非幂等**（会重建 client），所以：
   - 首次打开才 `connect`；
   - 切换已打开的连接**只从缓存取 capabilities**，不重连；
   - 只有关闭 TAB 时才 `disconnect`。
6. **跨组件通知走 `bus` 事件**，新增事件要在 `src/utils/util.ts` 的 `MeBusEvents` 里登记。

---

## 五、以后合并上游的操作手册

### 5.1 流程

```bash
git checkout feature/multi-tab-conn-key-v2
git fetch upstream
git rebase upstream/master        # 或 git rebase --onto upstream/master <上次同步点>
# 冲突处理见 5.2
npm run typecheck:vue && npm run build
```

### 5.2 冲突高发点与处理原则

| 冲突文件                     | 原因                       | 处理原则                                                                 |
| ---------------------------- | -------------------------- | ------------------------------------------------------------------------ |
| `src/App.vue`                | 上游也动 `share` 初始化   | **保留两边的字段**：上游字段 + 4 个多TAB 字段，`share` 只能有一份        |
| `src/views/AppMain.vue`      | 上游重构了连接切换逻辑     | 保留上游结构，**只把 `meCommands.connect` 换成 `connTabs.ensureConnected`** |
| `src/views/tab/RedisValue/index.vue` | 上游重构了 MeTable | 保留上游实现，多TAB 只保留「模板顶部挂 `<KeyTabStrip>`」+「onMounted 恢复值」 |
| `src/views/KeyMain.vue`      | 上游重构了键区             | 保留上游实现，只保证 `chooseKey()` 发 `KEY_OPEN_TAB`、历史存储仍是按连接 |
| `src/locales/lang/*.ts`      | 文案区整体后移             | 直接取两边并集，键名带 `connTabs*` / `keyTabs*` 前缀避免撞名             |
| `src/types/me-interface.ts`  | 类型定义增删               | 直接取两边并集                                                        |
| `src/utils/util.ts`          | 事件区增删                  | 直接取两边并集                                                        |

**新文件（`src/utils/conn-tabs.ts`、`src/utils/key-tabs.ts`、两个 `*TabStrip.vue`）永远不冲突**，
只有它们引用的上游接口（`share` 字段、`connUi` 方法、bus 事件）需要人工确认。

### 5.3 合并后自检清单

- [ ] `npm run typecheck:vue` 无错误
- [ ] `npm run build` 通过
- [ ] `src/App.vue` 里 `share` 只有一份，且含 4 个多TAB 字段
- [ ] 没有组件再自己 `reactive()` 一份 `share`
- [ ] 打开第 2 个连接后，第 1 个连接仍在（不被 disconnect）
- [ ] 切连接不重连（Network 面板无重复 connect）
- [ ] 切连接后键值 TAB 条保留，且值区不是空白
- [ ] 关掉某个连接 TAB 后自动切到相邻 TAB
- [ ] 键值 TAB 上限 10 个按「连接+库」计算

---

## 六、本次未移植 / 已知差异

| 项                                | 说明                                                                                     |
| --------------------------------- | ---------------------------------------------------------------------------------------- |
| 搜索历史按连接存储                | 已移植（`KeyMain.vue`）。注意旧版是全局数组，新版是 `{ connId: string[] }`，首次启动会重置旧数据 |
| `autoRefreshLabelWidth: 80→100`  | 旧 fork 的纯 UI 微调，**未移植**（与多TAB 无关）                                            |
| 上游 5.4.3 的「键区索引查询」    | 旧 fork 合并时曾把它整块丢掉；本分支基于上游 5.4.3，**该功能完整保留**                  |
| 「新建窗口」菜单                  | 保留上游实现（多TAB 是增量，不替代）                                                        |
| 旧分支 `feature/multi-tab-conn-key` | 保留作历史存档，含「统一 share」的修复提交；**新开发请用本分支**                       |

---

## 七、相关提交

| 提交                        | 说明                                             |
| --------------------------- | ------------------------------------------------ |
| `27d4d229`                 | 5.2.0 时期的 fork 合并点（多窗口功能来源）        |
| `a7ada820`                 | 「多窗口」功能提交（原始实现）                    |
| `88ad5c04`                 | 键搜索历史按连接存储                              |
| `6fce412b` / `1f82d8d5`    | 自动刷新 UI 与键值 TAB 切换逻辑修复                |
| `c6151714`                 | 5.4.3 合并点（**share 拆分事故点，仅存档**）      |
