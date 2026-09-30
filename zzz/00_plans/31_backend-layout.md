# 31. 后端目录整理

状态：未开始。

先做完 [32](./32_backend-tests.md) 的 32.1 和 32.2，保证 `cargo test --lib` 即使设了 `REDIS_URL` 也不连 Redis，再动这里。每一步只搬代码，命令行为不变，做完就提交。

## 现在的问题

后端大约 1.3 万行。难读的是 `client/client_trait.rs`（约 4100 行）：`MeClient`、全部共享命令（`*0`）、字段扫描、ACL、导入导出堆在一个文件里。

`impl_single.rs`（约 730 行）和 `impl_cluster.rs`（约 1070 行）里，很多方法是一行 `xxx0(self.get_conn()?, …)`。这行就是目录，打开就能看见调用了哪个函数。保持手写。

`utils/` 里的拨号、redis-cli、命令日志已经按文件分开，但都堆在同一个目录名下，找文件时仍要进「工具」里翻。目录改名从 31.5 起做：先拆 `client_trait.rs`，避免大范围改 import 和拆函数搅在同一次 diff 里。

另外两处分层是反的：

- `ConnConfig::test` / `masters` 会建连，写在 `utils/model.rs` 里，于是 model 和 `utils/conn.rs` 互相引用。
- `MeBase` 带 `AppHandle`、原子标记和命令日志，是连接运行时状态，却和 IPC 结构体放在一起。

## 整理完的样子

仍是一个 crate。`MeClient` 方法名、Tauri command、specta 类型名都不改。`impl_single.rs` / `impl_cluster.rs` 不改名。

```text
src-tauri/src/
  client.rs            # 模块入口，代替 client/mod.rs
  client/
    me_client.rs       # 原 client_trait.rs：只留 trait，以及下面「短命令」
    state.rs
    base.rs            # MeBase（从 model.rs 挪来）
    impl_single.rs
    impl_cluster.rs
    ops.rs             # 模块入口，代替 ops/mod.rs
    ops/
      scan.rs          # 键 SCAN
      field_scan.rs    # 字段扫描整条链路，含各类型分页
      key.rs           # 对已知键的读写
      as_cmd.rs        # 键/字段 → redis-cli 命令文本
      import_export.rs # CSV 与命令文件的导入导出
      acl.rs
      pubsub.rs
```

`utils/model.rs` 在 31.4 之前仍是一个文件，只留 IPC 数据和纯查询方法（`command_map`、`protocol_version`、`is_minimal_mode`）。31.7 把它挪到 crate 根的 `model.rs`，不拆成多个文件。

31.5 起 `utils/` 拆成三块，然后删除 `utils/`：

```text
src-tauri/src/
  model.rs                 # 原 utils/model.rs
  net.rs                   # 模块入口，代替 net/mod.rs
  net/
    conn.rs                # 建连，原 utils/conn.rs
    proxy.rs               # 原 proxy_dialer.rs
    ssh.rs                 # 原 ssh_dialer.rs
    system_proxy.rs
    tls.rs                 # 原 tls_cert.rs
  support.rs               # 模块入口，代替 support/mod.rs
  support/
    format.rs              # redis-cli 命令文本，原 redis_cli_format.rs
    tty.rs                 # redis-cli 回复排版，原 redis_cli_tty.rs
    error.rs
    util.rs
    command_log.rs
    capabilities.rs
    macros.rs
    setup.rs
    app_store.rs
```

`support/` 收错误、工具、命令日志、redis-cli 文本和启动。建连在 `net/`，IPC 在根上的 `model.rs`。新业务不要再丢进 `support/`。

## 按调用关系切，不要按类型切

打开一个键的扫描是一条流水线：`field_scan0` 分发到 `field_scan_list_page`、`field_scan_vectorset_page`、`field_scan_timeseries_page`。这些函数和 `FieldScanParam` 绑在一起，留在 `field_scan.rs`。不要拆 `vector.rs`、`timeseries.rs`、`array.rs`、`stream.rs`。

| 文件                   | 带走的函数                                                                                                                             |
| ---------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `ops/scan.rs`          | `scan_0_batch_count`、`scan_0_exact`、`scan_1_cmd`、`batch_key0`（按 pattern 循环 SCAN，批量删除和导出都用它）                         |
| `ops/field_scan.rs`    | `field_scan0` 及其分页、`field_scan_*`、`zset_rank0`、`zset_range0`。ZSET 分数单测跟着 `parse_zset_score_bound`                        |
| `ops/key.rs`           | `ttl0` `set0` `del0` `copy0` `field_add0` `field_set0` `field_ttl0` `field_get0` `hash_keys0` `hash_values0` `field_pop0` `field_del0` |
| `ops/as_cmd.rs`        | `key_as_command_lines`、`get_key_as_command0`、`get_field_as_command0`                                                                 |
| `ops/import_export.rs` | `export_*`、`import_*`（含写文件的 `export_key_as_command`，它调用 `as_cmd::key_as_command_lines`）                                    |
| `ops/acl.rs`           | 从 `acl_rule_to_string` 到 `acl_dryrun0` 的整段：解析、`build_acl_setuser_cmd`、以及 `acl_users0` 等 `acl_*0`。ACL 单测跟着走          |
| `ops/pubsub.rs`        | `publish0` `subscribe0` `subscribe_stop0` `monitor0` `monitor_stop0`                                                                   |

`as_cmd` 对齐已有的 `get_key_as_command`。`export_cmd` 会和文件导出混在一起，`to_cmd` 看不出来源。`import_export` 和函数前缀 `import_` / `export_` 一致。`transfer`、`exp_imp` 都要再猜一次。

`key.rs` 是对一个已经确定的键做读写，和 `scan` / `field_scan` 的翻页分开。`mutate` 盖不住 `field_get`、`hash_keys`、`hash_values`。

这几个函数被多处调用。依赖只保留一个方向：`key` / `as_cmd` / `me_client` 可以调用 `field_scan`，`import_export` 可以调用 `as_cmd` 和 `scan`，反过来不要。

- `handle_other_value_type`，以及 `vadd_values`、`vemb_json_or_dash`、`vgetattr_opt`、`vsetattr_json_or_clear`，留在 `field_scan.rs` 并标 `pub`。`key`、`as_cmd`、`v_getattr0` / `v_setattr0` 调用它们。
- `batch_key0` 的参数是 `&impl MeClient`，放在 `scan.rs`。`import_export` 和两个 impl 调用它，不要放进 `import_export.rs`。
- `as_cmd` 不调用 `import_export`。文件导出调用 `key_as_command_lines`，所以先有 `as_cmd`，再剪 `import_export`。

`implement_pipeline_commands!` 仍放在 `me_client.rs`，保持 `#[macro_export]`。只有单机 `impl` 使用它。集群的 `mock_data` 是手写的 `ClusterPipeline` 副本，这次不要并成一个宏。

短命令留在 `me_client.rs`，不要再为它们单开文件：`object_info0`、`key_type0`、`xinfo_groups0`、`xinfo_consumers0`、`ar_info0`、`ar_last_items0`、`v_info0`、`v_getattr0`、`v_setattr0`、`v_sim0` 及 `vsim_*`、`ts_info0` 及 TS.INFO 解析、`flush_db0`、`flush_all0`。TS.INFO 单测跟着 `parse_ts_info_items`。`v_getattr0` / `v_setattr0` 内部调用 `field_scan` 里的向量辅助函数。

## 每步都要满足

- 同一次提交里改掉调用方 import。这个 crate 没有外部用户，不留 `pub use ...::*` 垫片。
- 跨文件调用的函数用 `pub`。RedisME 是独立程序，不用 `pub(crate)`。
- `ops.rs` 只声明子模块，不把函数再导出一遍。调用写成 `ops::field_scan::field_scan0`。
- `cargo check --manifest-path src-tauri/Cargo.toml` 通过。
- `cargo test --lib --manifest-path src-tauri/Cargo.toml` 通过。
- 单机和集群继续手写一行转发，不要用宏生成。

`trait` 是关键字，模块文件用 `me_client.rs`，不能用 `trait.rs`。

## 31.1 键扫描和字段扫描

新建 `client/ops/`。把上表 `scan.rs` 和 `field_scan.rs` 的函数从 `client_trait.rs` 剪过去，改 `impl_single.rs` / `impl_cluster.rs` 的 import。

`batch_key0` 仍引用 `client_trait::MeClient`，31.3 再改这条路径。还留在 `client_trait.rs` 里的 `field_add0` 等，改为调用已经搬走的 `vadd_values`、`handle_other_value_type`。函数体先不改。

## 31.2 key、as_cmd、import_export、acl、pubsub

每个文件一次提交。顺序：`key`（依赖 31.1 的向量辅助函数）→ `as_cmd` → `import_export` → `acl` 与 `pubsub`（这两份互不依赖，谁先都可以）。剪完后 `client_trait.rs` 里应只剩 trait、短命令和 `implement_pipeline_commands!`。

## 31.3 trait 改名为 me_client.rs

确认 `client_trait.rs` 没有大块实现后，改名为 `client/me_client.rs`，更新 `mod` 和所有 `client::client_trait` 引用（`state.rs`、两个 impl、`ops/scan.rs`）。

`api_model!`、`api_commands!`、`implement_pipeline_commands!` 都是 `#[macro_export]`。文件挪到 `support/macros.rs` 或留在 `me_client.rs` 之后，调用处仍写 crate 根上的宏名，不用改。

## 31.4 拆开 model 和建连

`ConnConfig::test`、`ConnConfig::masters` 挪到 `utils/conn.rs`，改成

```rust
pub fn test_conn(conf: &ConnConfig, connect_timeout: Duration) -> AnyResult<()>
pub fn sentinel_masters(conf: &ConnConfig, connect_timeout: Duration, command_timeout: Duration) -> AnyResult<Vec<HashMap<String, String>>>
```

`api.rs` 改为调用这两个函数。`command_map`、`protocol_version`、`is_resp3`、`is_minimal_mode` 留在 `ConnConfig` 上。

`MeBase` 及其 `impl` 挪到 `client/base.rs`。`utils/model.rs` 不再引用 `get_client_single` / `get_client_cluster`。

做完跑 `export_tauri_specta_typescript_bindings`，`src/types/tauri-specta.ts` 除格式外没有类型变化。`MeBase` 本来就不参与导出。

## 31.5 建连归到 net/

整文件移动，同一次提交改完 import。不留 `utils::conn` 这种垫片。

| 现在                    | 之后                  |
| ----------------------- | --------------------- |
| `utils/conn.rs`         | `net/conn.rs`         |
| `utils/proxy_dialer.rs` | `net/proxy.rs`        |
| `utils/ssh_dialer.rs`   | `net/ssh.rs`          |
| `utils/system_proxy.rs` | `net/system_proxy.rs` |
| `utils/tls_cert.rs`     | `net/tls.rs`          |

单测跟着文件走。本步只搬文件，不改建连逻辑。

## 31.6 redis-cli 归到 support/

只有 `format.rs` 和 `tty.rs` 两个文件，不单开 `cli/`。`utils/redis_cli_format.rs` → `support/format.rs`，`utils/redis_cli_tty.rs` → `support/tty.rs`。同样当次改完 import。

## 31.7 其余归到 support/，model 提到根上

| 现在                    | 之后                      |
| ----------------------- | ------------------------- |
| `utils/model.rs`        | `model.rs`                |
| `utils/error.rs`        | `support/error.rs`        |
| `utils/util.rs`         | `support/util.rs`         |
| `utils/command_log.rs`  | `support/command_log.rs`  |
| `utils/capabilities.rs` | `support/capabilities.rs` |
| `utils/macros.rs`       | `support/macros.rs`       |
| `utils/setup.rs`        | `support/setup.rs`        |
| `utils/app_store.rs`    | `support/app_store.rs`    |

`lib.rs` 去掉 `mod utils`。搬完删除空的 `utils/`。

`util.rs` 保持原样跟着走，不在这一步把 `parse_client_info`、`split_redis_args` 等再拆进 `ops/`。那是另一次行为不变的搬家，和改目录混在一起会很难看 diff。

做完再跑一次 specta 导出和 `cargo test --lib`。

## 本计划不做

- 用宏生成单机/集群的一行转发。
- 把 `model.rs` 拆成 scan/key/acl/events 多个文件。
- 改 `MeClient` 方法列表，或把单机和集群收成一个泛型。
- 重写 `*0` 的控制流。
- 一次提交里既搬家又改扫描或 ACL 的行为。
- 拆 `api.rs`。它大约 200 行，保持薄转发。
