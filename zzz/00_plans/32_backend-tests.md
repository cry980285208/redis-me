# 32. 后端测试

状态：未开始。

[31](./31_backend-layout.md) 从 32.2 做完再开始。每步单独提交。

## 要达到的状态

`cargo test --lib` 永不连接 Redis，本机即使已经设置 `REDIS_URL` 也一样。

单机往返放在 `src-tauri/tests/`，和库分开。没配单机就跳过；配了却连不上或断言失败，测试失败。配好单机后，string、hash、list、set、zset、stream 必须通过，并且覆盖下面「往返要断言的内容」。JSON、TimeSeries、VectorSet、Array 看本机 Redis 有没有该模块，没有就跳过。stream 从 Redis 5 起就在服务端里，不算可选模块。

不新增 GitHub Actions。发版 workflow 保持原样。

## 现在为什么不能当回归

`client/mod.rs` 的 `mod tests`（约 450 行）是手写联调：主机写死、`REDIS_PASSWORD` 用 `expect`、没有断言，`config_set` 和大批量写入会改远端数据。没这台机器时 `cargo test` 失败。

库内已有、继续留在原文件的单测：`redis_cli_format`、`redis_cli_tty`、`conn`、`proxy_dialer`、`system_proxy`、`tls_cert`、`command_log`、`error`、`util`，以及 ACL selector、TS.INFO 解析、ZSET 分数边界。

## 配置

同一角色只认一个来源，不把 URL 和文件里的字段拼在一起。密码写在 URL 里，不再另设 `REDIS_PASSWORD`。两个 toml 也不合并：仓库内本地文件存在就用它，否则用家目录那份。

| 角色     | 来源                                                                                                                                                                                                      |
| -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 单机     | `REDIS_URL` 有值就只用它，否则用 toml 的 `[single]`                                                                                                                                                       |
| 集群     | `REDIS_CLUSTER_URL`（逗号分隔的种子）有值就只用它，密码写在某个种子的 userinfo 里，例如 `redis://:pass@127.0.0.1:7001`。否则用 toml 的 `[cluster]`（`password` 只在这条文件路径上）。两个都没有就没有集群 |
| 扩展连接 | 只认 toml 里 32.6 的可选段。某段没有就只跳过这一段                                                                                                                                                        |

toml 路径：先 `src-tauri/redis-test.local.toml`，没有再读 `~/.config/redis-me/test.toml`（Windows 为 `%USERPROFILE%\.config\redis-me\test.toml`）。文件可以只写 `[single]`，缺的段就当没配。

```toml
[single]
url = "redis://127.0.0.1:6379/15"

[cluster]
nodes = ["127.0.0.1:7001"]
password = ""
```

提交进仓库的 `redis-test.example.toml` 只保留上面两段（无密码、15 号库）。SSL、SSH、代理、哨兵写在本机 toml 里，字段见 32.6。没有的段就跳过，不要为了测全去连公网。`.gitignore` 只加 `src-tauri/redis-test.local.toml`。不要忽略所有名叫 `test.toml` 的文件；家目录里的配置本来就不在仓库里。

解析放在集成测试里，用 `toml` 的 **dev-dependency**。不要放进 `[dependencies]`。

## 跳过和失败

```rust
match redis::single_client() {
    Ok(None) => {
        eprintln!("skip: no redis single config");
        return;
    }
    Ok(Some(client)) => { /* 断言 */ }
    Err(err) => panic!("redis configured but not usable: {err}"),
}
```

- 没有单机配置 → `Ok(None)`，测试通过。
- 有配置但 `MeSingle::init` 失败 → `Err`，测试失败。
- 断言失败 → 测试失败。
- 可选模块命令返回 unknown command → 打印后 `return`，测试通过。string、hash、list、set、zset、stream 不在此列。

`single_client()` 里用 `Once` 安装 `rustls::crypto::ring::default_provider()`。不装的话，带 `tls-rustls` 的测试进程会 panic。

## 活测试放在 tests/

集成测试是另一个 crate，只能看见公开 API。`MeSingle::init` / `MeCluster::init` 已经是 `pub`。配置加载不要放进库的 `#[cfg(test)]`，否则 `cargo test --lib` 仍可能在设置了 `REDIS_URL` 时去连服务器。

```text
src-tauri/tests/common/redis.rs  # 读配置、single_client、field_scan_param、rustls Once
src-tauri/tests/redis_env.rs     # 只测解析，不连网络
src-tauri/tests/live_single.rs
src-tauri/tests/live_cluster.rs  # 32.5
src-tauri/tests/live_conn.rs     # 32.6 SSL、SSH、代理、哨兵
```

辅助文件放在 `tests/common/` 下。Cargo 会把 `tests/*.rs` 每个文件编成一个集成测试，放在根上会多出一个空测试包。模块仍叫 `redis`，不叫 `support`。31 会把库里的一部分文件放进 `support/`，两边同名之后很难搜。

库名是 `redis_me_lib`（见 `src-tauri/Cargo.toml` 的 `[lib]`）。集成测试用这个名字，不是包名 `redis-me`。

每个集成测试文件是独立 crate，用下面方式共用：

```rust
#[path = "common/redis.rs"]
mod redis;
```

`FieldScanParam` 字段很多。`redis::field_scan_param(key)` 填默认值，每个往返用例保持几行。

键前缀 `redis-me:test:` 加本次测试的短 id。`cargo test` 会并行，不能用固定键名。结束时只删除这些键。

测试代码禁止 `FLUSHALL`、`FLUSHDB`、`CONFIG SET`。

## 32.1 配置加载

新增 `tests/common/redis.rs`、`tests/redis_env.rs`、`redis-test.example.toml`，以及 gitignore 那一行。`toml` 写入 dev-dependencies。不改 `client/mod.rs`。

`redis_env` 覆盖：临时文件、缺文件、`REDIS_URL` 命中后不再读 `[single]`、URL 解析出 host/port/db/password、只有 `[cluster]` 时单机仍算缺失。改环境变量的用例共用一把 `Mutex`，避免并行测试互相覆盖。测完恢复环境变量。不连 Redis。

做完：`cargo test --test redis_env` 通过。

## 32.2 删掉 client/mod.rs 的联调

整段 `mod tests` 删除，不改名留着，也不迁到别的文件。没有断言的 `println` 不是回归；需要的场景在 32.4 用临时键重写。git 历史里还在。

一并去掉的行为：写死主机、`expect("REDIS_PASSWORD")`、`test_config_set`、向固定键名写入几千个字段、默认执行的 `test_cluster_pipeline_reproduce`（那是上游 issue 复现，留在 git 历史即可）。

做完：`cargo test --lib` 在设置了 `REDIS_URL` 时也不连接 Redis，并且通过。这是 31 的门槛。删的是 `client/mod.rs` 里的 `mod tests`，文件里的 `mod` 声明保留。

## 32.3 纯函数单测

测决策，不给整条命令参数做快照。单测放在函数所在文件底部，31 搬家时跟着走。已有单测的不重写。

| 函数                      | 断言什么                                                          |
| ------------------------- | ----------------------------------------------------------------- |
| `scan_1_cmd`              | `timeseries` → `TSDB-TYPE`，`json` → `ReJSON-RL`；空类型不加 TYPE |
| `resolve_list_scan_range` | 倒序、区间裁剪                                                    |
| `import_restore_ttl`      | 永久、已过期、ignore                                              |
| `parse_client_info`       | 一条 CLIENT LIST，含缺字段                                        |
| `parse_node_list`         | 一条 CLUSTER NODES，主从和 slot 范围                              |
| `build_acl_setuser_cmd`   | 规则顺序里保留 selector（解析单测已有，这里只补拼出来的参数）     |

样例用字面量。做完：上述测试在 `cargo test --lib` 里通过。

## 32.4 单机往返

`tests/live_single.rs`。写入 → `scan` / `field_scan` / `field_get` → 断言 → 删除。数量只比一页略多，不要再写几千条。

本机配好单机后必须通过：string、hash、list、set、zset、stream。没有配置时整组跳过。

JSON 一条 `JSON.SET`；Array、VectorSet、TimeSeries 先试创建命令，unknown command 就跳过。模块在时，同样做中文和非 UTF-8 往返；分页只要求有游标的类型（Hash / List / Set / ZSet / Stream / 键 SCAN）。

### 往返要断言的内容

这些检查收进 `tests/common/redis.rs`，单机和集群共用。只断言 ASCII 不算通过。

| 情况           | 怎么写                                                                         | 断言                                                                                                                                      |
| -------------- | ------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------- |
| 中文           | 键名、String 值、Hash 字段名和值、List 元素都含中文，例如 `键` / `值`          | 读回与写入相同，不能变成乱码或空串                                                                                                        |
| 非 UTF-8       | String 值、Hash 字段、键本身使用非法 UTF-8 字节，例如 `[0xff, 0xfe, 0x00]`     | 用 `BytesFormat::Base64` 读回，解码后与原字节一致。二进制键的 `RedisKey.bytes` 非空，后续删除和再读走 `bytes`，不用 lossy 的 `key` 字符串 |
| 键扫描下一页   | 同一前缀写入 5 个键，`ScanParam.count` 设为 2                                  | 第一次 `cursor.finished == false`。把返回的 `cursor` 原样传回去再扫，多页键名合起来正好是这 5 个，没有重复，最后一页 `finished == true`   |
| 字段扫描下一页 | Hash / List / Set / ZSet / Stream 各写 5 个元素，`FieldScanParam.count` 设为 2 | 同上：第一页未结束，用返回游标取下一页，各页拼起来等于写入内容。List、Stream 还要断言顺序没乱                                             |
| 精确匹配       | 对上面的中文键和二进制键设 `exact: true`                                       | 只命中这一条；不存在的键返回空列表                                                                                                        |

`field_scan` 的 `pattern` 是搜索框明文，不是 Base64。精确查 Hash 字段时 pattern 用字段原文；二进制字段用原始字节比较，不要把 pattern 按 `bytes_format` 再解码一遍。

做完：有单机配置时上表和基础类型都通过；没有配置时跳过并通过。

## 32.5 集群往返

`tests/live_cluster.rs`。没有 `REDIS_CLUSTER_URL` 且文件里没有 `[cluster]` 就跳过。

基础类型的断言收成 `tests/common/redis.rs` 里接受 `&dyn MeClient` 的函数，单机和集群共用，包含上一节的中文、非 UTF-8 和翻页。集群再加两件：

- `key_slot` / `key_node` 对一个已知键有结果。
- RENAME 跨 slot。用 hash tag 把两个键钉在不同 slot，例如 `{a}redis-me:test:…` 和 `{b}redis-me:test:…`，不要靠随机键名碰运气。

## 32.6 扩展连接

`tests/live_conn.rs`。每段独立：没有这段就跳过这一段，其它段照常跑。路径支持 `~`。

每种能连上的方式都做同一件数据断言，不只读 `INFO`：写入一条中文 String，再写入一段非 UTF-8（`[0xff, 0xfe]`）并用 Base64 读回。不要求在 SSH 或代理上再跑一遍 Hash 翻页。

| 段                | 连接                                                                                        | 通过标准                                                                             |
| ----------------- | ------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| `[ssl]`           | `ssl = true`，不填证书。`url` 用 `rediss://`                                                | 往返成功。这是现在的 insecure TLS                                                    |
| `[ssl_mtls]`      | 填写 `cert`、`key`，`ca` 可空                                                               | 客户端证书能连上并完成往返。证书文件读不到则失败，不要当成没配                       |
| `[ssh_pwd]`       | `login_type = "pwd"`，`host` / `port` / `username` / `password`，`url` 仍是隧道对面的 Redis | 经 SSH 完成往返                                                                      |
| `[ssh_key]`       | `login_type = "pkfile"`，`pkfile`，`passphrase` 可空                                        | 私钥登录后完成往返。有口令的密钥把 `passphrase` 写上                                 |
| `[proxy_http]`    | `proxy_mode = "manual"`，`proxy_type = "http"`                                              | 经 HTTP CONNECT 完成往返                                                             |
| `[proxy_https]`   | `proxy_type = "https"`                                                                      | 先对代理做 TLS，再 CONNECT，然后往返。这和 Redis 的 `rediss` 是两层                  |
| `[proxy_socks5]`  | `proxy_type = "socks5"`                                                                     | 本机解析 Redis 主机名后往返                                                          |
| `[proxy_socks5h]` | `proxy_type = "socks5h"`                                                                    | 主机名交给代理解析后往返                                                             |
| `[proxy_auth]`    | 上面任一手动类型再加 `username` / `password`                                                | 代理认证成功后往返。要测认证时单独写这一段，不要和没认证的段混用同一配置             |
| `[proxy_system]`  | `proxy_mode = "system"`                                                                     | 先做系统代理检测。没检测到就跳过。检测到了必须走该代理完成往返，不能悄悄直连还算通过 |
| `[sentinel]`      | `sentinel = true`，`master_name`，主库账号可空                                              | `sentinel masters` 至少一条，并且连上当前主库完成往返                                |
| `[cluster_ssl]`   | 在集群种子上再开 `ssl = true`                                                               | 跑 32.5 的集群断言                                                                   |
| `[cluster_ssh]`   | 集群再加 SSH（密码或私钥，二选一，字段同 `[ssh_pwd]` / `[ssh_key]`）                        | 跑 32.5 的集群断言                                                                   |

`[single]` 的 URL 里如果带了用户名和密码，32.4 的往返必须用这个账号完成，不能只 `PING`。某段写了 `resp3 = true` 时，连上后 `CLIENT INFO` 里的 `resp` 为 3。

下面这些不连真服务器，放在 `conn.rs` 已有单测里，缺了就补上，不要放到 `live_conn.rs`：

- 同时开 SSH 和代理，建连前就失败。
- 手动代理的 host 为空，建连前就失败。
- 哨兵没写 `master_name`，建连前就失败。
- IPv6 主机写成 `::1` 时，URL 带方括号。

做完：本机 toml 里写了的段都通过；没写的段跳过。

## 步骤依赖

```text
32.1 配置加载
  → 32.2 删除联调                ← 31 可以开始
  → 32.3 纯函数（可与 32.4 并行）
  → 32.4 单机往返
       → 32.5 集群
       → 32.6 扩展连接（SSL、SSH、代理、哨兵，以及集群叠 SSL 或 SSH）
```

## 本计划不做

- 用假 Redis 协议替换真服务器。纯函数用字面量，往返用真 Redis。
- 为了测试改 `*0` 的返回值。纯函数单测放在该函数所在文件里，不为此把只在文件内使用的私有函数暴露出去。
- 把只打印的旧用例留在树里。
- 在测试里 `CONFIG SET`、`FLUSHALL`、`FLUSHDB`，或连接固定公网主机。
- 把密码写进仓库。
- 新增 GitHub Actions。本机 `cargo test` 通过即可。
