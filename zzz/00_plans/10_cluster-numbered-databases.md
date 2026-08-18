# 集群模式多数据库（Valkey 9 / redis-rs 1.3.0）支持方案

> **实现状态**：✅ Phase 1 已实现（运行时切库未做）  
> **关键代码**：`conn.rs`（`database_id`）、`capabilities.rs`（`clusterDbSupported`）、`ConnSave.vue`、`KeyMain.vue`  
> **实际实现**：连接时 `ClusterClientBuilder::database_id(conf.db)`；`clusterDbSupported` = 集群 + Valkey + major≥9；主界面只读 `dbX` 标签；`select_db` 返回 `ClusterDbSwitchNotSupported`；Phase 2 运行时切库暂不实现。

## 一、背景

### 1.1 上游能力

- **redis-rs 1.3.0** 新增集群客户端编号数据库支持（[PR #2146](https://github.com/redis-rs/redis-rs/pull/2146)）
  - `ClusterClientBuilder::database_id(i64)`：建连握手时对各节点执行 SELECT
  - 重连后自动恢复同一 db，不依赖后续手动 SELECT
  - 也可通过 URL 指定（如 `redis://host:6379/4`），但与 `.database_id()` 不可冲突
- **Valkey 9.0** 在集群模式下支持 numbered databases（[官方博客](https://valkey.io/blog/numbered-databases/)）
  - 键路由仍由 key 决定 slot，db 编号不影响 slot 计算
  - DBSIZE / SCAN / FLUSHDB 等命令在集群下为**当前节点、当前库**的局部视图
- **Redis OSS 集群** 仍不支持非 0 库，握手时会返回 `SELECT is not allowed in cluster mode`
- **redis-rs 后续方向**（[PR #2150](https://github.com/redis-rs/redis-rs/pull/2150)，未合并）：拟禁止集群客户端手动发 SELECT，db 视为连接级不可变属性

### 1.2 实际实现

| 环节         | 单机                                                      | 集群（当前）                                            |
| ------------ | --------------------------------------------------------- | ------------------------------------------------------- |
| 连接时 db    | `new_raw_conn` 发 SELECT；哨兵用 `set_client_to_redis_db` | `get_client_cluster` 传 `.database_id(conf.db)` ✅      |
| 运行时切换   | `SELECT` + 更新本地 `db`                                  | **拒绝**（`ClusterDbSwitchNotSupported`）               |
| `db_list`    | `CONFIG GET databases`                                    | 仍返回 `[]`（Phase 2）                                  |
| 前端 db 展示 | `el-select` 可切换                                        | Valkey 9+ 集群：`showClusterDbLabel` 只读标签；否则隐藏 |
| 连接编辑     | db 输入框 + tooltip                                       | 同左；非 0 db 在不支持的服务器上 testConn/connect 失败  |
| 能力探测     | —                                                         | `clusterDbSupported` = 集群 + Valkey + major≥9          |

依赖：`src-tauri/Cargo.toml` 已使用 `redis = "1"`（当前解析为 1.3.0）。

---

## 二、redis-rs 能力结论（问题 1）

### 2.1 连接时设置 db

**支持。**

```rust
ClusterClient::builder(vec![url])
    .database_id(conf.db as i64)
    .build()?
```

- db 是 **ClusterClient 级配置**，非单次 Connection 临时状态
- 新建连接、拓扑变更重连时，redis-rs 会对各节点重新 SELECT 到配置的 db
- 非 0 db 需要服务端支持（Valkey 9+ 且已启用多库）；否则握手失败

### 2.2 运行中切换 db

**不应使用 `SELECT` 命令；应重建 ClusterClient。**

| 方式            | redis-rs 1.3.0                                                   | 设计方向        |
| --------------- | ---------------------------------------------------------------- | --------------- |
| 手动 `SELECT n` | 可能只路由到一个节点，重连后丢失                                 | PR #2150 拟禁止 |
| 改 db           | **重建 ClusterClient**（新 `database_id`）+ 新 ClusterConnection | 官方推荐        |

对本项目的含义：

- `MeCluster::select_db` **不要**发 `SELECT`
- 切换 db → 用新 `database_id` 重建 `ClusterClient` 和 `ClusterConnection`（类似重连，但 client 也要换）

### 2.3 兼容性矩阵

| 服务端                       | db=0 | db≠0                       |
| ---------------------------- | ---- | -------------------------- |
| Redis OSS 集群               | 正常 | 握手失败                   |
| Valkey 9+ 集群（多库已启用） | 正常 | 正常                       |
| Valkey 9+ 集群（多库未启用） | 正常 | `DB index is out of range` |

---

## 三、产品方案（问题 2）

### 3.1 方案对比

| 方案                         | 描述                                                               | 优点                         | 缺点             |
| ---------------------------- | ------------------------------------------------------------------ | ---------------------------- | ---------------- |
| **A：仅连接界面**            | 连接编辑设初始 db，主界面隐藏下拉，改 db 需重连                    | 改动小，与 redis-rs 模型一致 | 体验与单机不一致 |
| **B：与单机相同 UI（推荐）** | `clusterDbSupported` 为 true 时显示 db 下拉，切换时后端重建 client | 交互统一                     | 实现量较大       |
| **C：混合**                  | 不支持多库时走 A，支持时走 B                                       | 渐进                         | 前端分支略多     |

**推荐：方案 B（分两阶段交付，见第六节）。**

### 3.2 连接编辑（ConnSave）

- db 输入框**始终保留**（初始库，与单机一致）
- 集群 + 不支持多库：依赖 `testConn` / `connect` 对非 0 db 的失败提示；可选 UI 上 disable 或 tooltip「仅 Valkey 9+ 集群」
- 集群 + 支持多库：行为与单机相同

### 3.3 主界面 db 切换（KeyMain）

**现状：**

```vue
v-if="!share.conn.cluster"
```

**目标（Phase 1 已实现）：**

- Valkey 9+ 集群：`showClusterDbLabel` 只读 `dbX` 标签（**非** db 下拉切换）
- 连接时 db 在 `ConnSave.vue` 配置；运行时 `select_db` 拒绝

**Phase 2（未做）：** `clusterDbSupported` 时显示 db 下拉，切换时后端重建 client。

### 3.4 集群 `db_list`（Phase 2，未实现）

当前 `impl_cluster.rs` **`db_list` 仍返回 `[]`**。Phase 2 计划：

1. 对某一 master 节点执行 `CONFIG GET databases`（与单机类似）
2. 失败时退回 `[{ db: current_db, size: 0 }]`

`dbSize`、SCAN 进度：Valkey 文档已说明集群下 DBSIZE/SCAN 为节点局部视图；现有「单 master 键数 × master 数」为近似值，首版可维持并加注释。

---

## 四、服务能力探测（问题 3）

### 4.1 新增字段

在 `ServerCapabilities`（`src-tauri/src/utils/capabilities.rs`）增加：

```rust
cluster_db_supported: bool  // 前端：clusterDbSupported
```

`connect` 时检测并返回，前端写入 `share.capabilities`。

### 4.2 检测策略（分层）

| 层级                | 方法                                          | 说明                       |
| ------------------- | --------------------------------------------- | -------------------------- |
| L1 快速排除         | `cluster && !is_valkey`                       | Redis OSS 集群 → **false** |
| L2 版本启发         | `is_valkey && major >= 9`                     | Valkey 9+ → **true**       |
| L3 配置细化（可选） | `CONFIG GET cluster-databases` 或 `databases` | 得到 db 数量；未启用时为 1 |
| L4 隐式确认         | 用户 `conf.db ≠ 0` 且连接成功                 | 说明该 db 可用             |

**首版建议：L1 + L2**；`db_list` 用 L3 细化数量。  
**不建议**为探测单独建 `database_id=1` 的测试连接（有副作用、慢）。

### 4.3 与现有能力位关系

- 仅在 **cluster 连接** 且 **connect 成功后** 有意义
- 单机连接时恒为 false（或忽略）
- 与 `infoSupported`、`httlSupported` 等并列，供前端条件渲染

---

## 五、后端实现要点

### 5.1 Phase 1：连接时 db

**文件：`src-tauri/src/utils/conn.rs`**

```rust
let mut builder = ClusterClient::builder(vec![url.to_string()]);
// ... username / password / tls ...
builder = builder.database_id(conf.db as i64);
let client = builder.build()?;
```

- `test_conn` 与正式 `connect` 共用 `get_client_cluster`，非 0 db 在不支持的服务器上应直接失败并展示清晰错误

**文件：`src-tauri/src/utils/capabilities.rs`**

- `detect_capabilities` 增加 `cluster_db_supported`（L1+L2，需传入是否 cluster；或在 cluster connect 后单独设置）

### 5.2 Phase 2：运行时切换 db

**核心：`MeCluster::select_db` 重建 client，不发 SELECT**

```
1. 若 !capabilities.cluster_db_supported → 返回错误
2. 若 db 未变 → return Ok(())
3. 停止 subscribe / monitor（若运行中）
4. conf.db = new_db，调用 get_client_cluster(&conf)
5. 替换 ClusterClient + ClusterConnection
6. 更新 atomic db、LoggingClusterConnection.set_db_index、MeBase.conf.db
7. （可选）刷新 node_list
```

**结构改动：**

- `MeCluster.client` 目前为普通字段，`select_db(&self)` 无法原地替换
- 方案 1：`client` 放入 `Mutex<ClusterClient>`（与 `conn` 对称）
- 方案 2：在 `AppState` 层对集群 `select_db` 走 disconnect → 改 conf.db → connect（较重但边界清晰）

**`reconnect()`** 须沿用 `self.conf.db`（init 时已写入 `MeBase.conf`）。

**禁止：**

```rust
// 不要在集群模式这样做
redis::cmd("select").arg(db).query(&mut conn)?;
```

### 5.3 Phase 2：`db_list` 实现

**文件：`src-tauri/src/client/impl_cluster.rs`**

```rust
fn db_list(&self) -> AnyResult<Vec<RedisDB>> {
    if !self.capabilities.cluster_db_supported {
        return Ok(vec![]);  // 或 Ok(vec![RedisDB { db: current, size: 0 }])
    }
    // CONFIG GET databases，逻辑参考 impl_single.rs
}
```

### 5.4 类型导出

- `ServerCapabilities` 经 specta 导出 → 更新 `src/types/tauri-specta.ts`
- 前端 `AppMainShare.capabilities` 增加 `clusterDbSupported`

---

## 六、实施阶段

### Phase 1（已完成）

1. ✅ `get_client_cluster` 增加 `.database_id(conf.db as i64)`
2. ✅ `ServerCapabilities` 增加 `cluster_db_supported`（L1+L2）
3. ✅ `testConn` / `connect` 在 Redis 集群 + db≠0 时正确失败
4. ✅ 前端：`ConnSave.vue` db 输入 + tooltip；`KeyMain.vue` 只读 `dbX` 标签（`showClusterDbLabel`）
5. ✅ `select_db` 明确拒绝（`ClusterDbSwitchNotSupported`）

**未做**：主界面 db 下拉切换、运行时重建 client（见 Phase 2）。

### Phase 2（体验对齐单机）

1. `MeCluster::db_list` 实现
2. `MeCluster::select_db` 通过重建 client 实现
3. 前端按 `clusterDbSupported` 显示 db 下拉（KeyMain 等 `!share.conn.cluster` 条件）
4. subscribe / monitor 切换 db 时的停止与重建
5. 切换 db 写回 `connList` 持久化

---

## 七、前端改动清单

| 文件                                     | 改动                                                              |
| ---------------------------------------- | ----------------------------------------------------------------- |
| `src/types/me-interface.ts`              | `ServerCapabilities.clusterDbSupported`                           |
| `src/views/AppMain.vue`                  | connect 后 capabilities 已 assign，无需额外逻辑                   |
| `src/views/KeyMain.vue`                  | Valkey 9+ 集群：`showClusterDbLabel` 只读 `dbX`；单机仍用 db 下拉 |
| `src/views/conn/ConnSave.vue`            | 可选：集群且不支持时 disable db 或 tooltip                        |
| `src/plugins/tauri.ts` / specta 生成类型 | 同步新字段                                                        |

---

## 八、测试计划

### 8.1 手动

- [ ] Redis OSS 集群：db=0 连接成功；db=1 testConn 失败
- [ ] Valkey 9 集群（多库启用）：db=0/非0 连接成功
- [ ] Valkey 9 集群：主界面 db 下拉可见（Phase 2）
- [ ] 切换 db 后 SCAN / GET / 收藏 / 命令日志 db 索引正确
- [ ] 切换 db 后 subscribe/monitor 行为正常（Phase 2）
- [ ] 重连后仍保持当前 db

### 8.2 回归

- [ ] 单机、哨兵 db 切换不受影响
- [ ] 旧 Valkey / Redis 集群 db=0 行为与现网一致

---

## 九、待确认项

| #   | 问题                          | 结论（2026-07-01）                      |
| --- | ----------------------------- | --------------------------------------- |
| 1   | 是否做 Phase 2（运行时切换）  | **暂不做**；只读 db 标签 + 连接配置指定 |
| 2   | 能力位命名                    | `clusterDbSupported` ✅                 |
| 3   | 切换 db 是否持久化到 connList | 单机：是；集群：仅连接配置中的 db       |
| 4   | Redis OSS 集群下 db 输入框    | 保留，靠 testConn 失败提示 ✅           |

---

## 十、参考

- [redis-rs PR #2146 — Add numbered databases support for the cluster client](https://github.com/redis-rs/redis-rs/pull/2146)
- [redis-rs PR #2150 — Forbid SELECT command for the cluster client](https://github.com/redis-rs/redis-rs/pull/2150)（未合并）
- [Valkey — Numbered Databases in Valkey 9.0](https://valkey.io/blog/numbered-databases/)
- [ClusterClientBuilder::database_id — docs.rs](https://docs.rs/redis/latest/redis/cluster/struct.ClusterClientBuilder.html#method.database_id)
- 项目内相关代码：
  - `src-tauri/src/utils/conn.rs` — `get_client_cluster`
  - `src-tauri/src/client/impl_cluster.rs` — `select_db` / `db_list`
  - `src-tauri/src/utils/capabilities.rs` — 能力检测
  - `src/views/KeyMain.vue` — db 只读标签
  - `src/views/conn/ConnSave.vue` — 连接编辑 db 输入
- 已实现方案索引：[README.md](./README.md)
