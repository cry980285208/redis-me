# 键值标签页：复制为命令 — 实现方案

> **实现状态**：✅ 已实现  
> **关键代码**：`src-tauri/src/utils/redis_cli_format.rs`、`get_key_as_command` API、`src/views/tab/RedisValue.vue`  
> **实际实现**：键值页单键扩展菜单「复制为命令」；格式逻辑与 [09](./09_command-export-format.md) 批量导出共用。

## 一、需求

在键值详情页，将当前键转为 **redis-cli 可识别、可粘贴执行** 的命令文本并复制到剪贴板。

**示例（STRING，含换行）：**

```redis
SET "MultiLine" "Line01\nLine02"
```

**示例（HASH，单条命令）：**

```redis
HMSET user:1 name "张三" age "28" email "a@b.com"
```

**示例（二进制 STRING）：**

```redis
SET binkey "\x00\x01\xffhello"
```

粘贴后应满足：

| 场景                | 要求                                               |
| ------------------- | -------------------------------------------------- |
| **redis-cli**       | 交互粘贴或 `redis-cli < file.txt` 可执行           |
| 应用内 **终端**     | 与 `split_redis_args` 解析一致（已对齐 redis-cli） |
| 应用内 **导入命令** | 逐行 `import_cmd` 可执行                           |

---

## 二、设计原则

### 2.1 尽量单条命令

同一键能在 **一条 Redis 命令** 内表达完的，不拆成多行：

| 类型   | 命令                            | 说明                                        |
| ------ | ------------------------------- | ------------------------------------------- |
| STRING | `SET key "value"`               | 单条                                        |
| HASH   | `HMSET key f1 "v1" f2 "v2" ...` | 单条，所有 field-value 成对追加             |
| LIST   | `RPUSH key "a" "b" "c"`         | 单条，`RPUSH` 支持多 element                |
| SET    | `SADD key "m1" "m2" "m3"`       | 单条，`SADD` 支持多 member                  |
| ZSET   | `ZADD key 1 "m1" 2.5 "m2"`      | 单条，score-member 成对                     |
| STREAM | 每条 entry 一行 `XADD`          | **无法合并**为一条（每条 id + fields 独立） |
| JSON   | `JSON.SET key $ '{...}'`        | 单条                                        |

> Redis 4.0+ 的 `HSET key f1 v1 f2 v2` 与 `HMSET` 等价；本方案用 **HMSET**，与 redis-cli 历史习惯一致，旧版本也兼容。

### 2.2 不考虑过期时间

复制结果 **不包含** `EXPIRE` / `PEXPIRE`。

### 2.3 二进制：与 redis-cli 一致

**不使用** 自定义 `BASE64:` 前缀。二进制键名、字段名、值一律按 **redis-cli 双引号 + C 风格转义** 输出，与 `GET` 回显、`SET "\x00..."` 输入习惯一致。

项目内 `split_redis_args`（注释：与 redis-cli `sdssplitargs` 一致）已支持 `\xHH`、`\n`、`\r`、`\t`、`\"`、`\\` 等，生成侧与之对称即可。

### 2.4 转义规则（生成侧）

对需要加引号的参数（键名、值、field、member 等），用双引号包裹，逐字节：

| 字节                                  | 输出                     |
| ------------------------------------- | ------------------------ |
| `"`                                   | `\"`                     |
| `\`                                   | `\\`                     |
| `\n` (0x0A)                           | `\n`                     |
| `\r` (0x0D)                           | `\r`                     |
| `\t` (0x09)                           | `\t`                     |
| 可打印 ASCII（0x20–0x7E，除 `"` `\`） | 原字符                   |
| 其余                                  | `\xHH`（小写 hex，两位） |

**键名 / 字段名：** 仅含 `[A-Za-z0-9_:.-]` 等「安全字符」时可不加引号；含空格、引号、二进制等则加引号并同上转义。

**JSON.SET 的 JSON 参数：** 第三参数为 JSON 文本；若含空格或特殊字符，用单引号包裹（redis-cli 支持 `'...'`），内部单引号按 redis-cli 规则转义。

---

## 三、入口位置

`RedisValue.vue` 顶部 **扩展功能下拉**（`value-header-actions` → `el-dropdown`），放在 **「创建副本」**（`duplicateKey`）**下面** 新增一项：

```
刷新
删除
复制键名
重命名
创建副本
复制为命令    ← 新增
```

- `command="copyAsCommand"`
- 只读连接可用
- 与「创建副本」一样，不区分 `canEdit`（复制不写库）

不在底部 footer 增加按钮。

---

## 四、实现方案

### 4.1 推荐：后端全量序列化

新增 Tauri API，Rust 连接 Redis **全量读取** 后格式化为命令字符串：

```rust
get_key_as_command(key: RedisKey) -> String
```

| 优点         | 说明                            |
| ------------ | ------------------------------- |
| 数据完整     | 不受键值页 `fieldScan` 分页影响 |
| 转义一处维护 | 与 redis-cli 规则对齐           |
| 真实字节     | 不受前端 utf8/hex 视图编码影响  |

**不推荐** 纯前端：HASH/LIST 等分页加载会导致复制不完整。

### 4.2 各类型读取与输出

| 类型   | Redis 读取               | 输出模板                                  |
| ------ | ------------------------ | ----------------------------------------- |
| string | `GET`                    | `SET {key} {value}`                       |
| hash   | `HGETALL`                | `HMSET {key} {f1} {v1} {f2} {v2} ...`     |
| list   | `LRANGE 0 -1`            | `RPUSH {key} {e1} {e2} ...`               |
| set    | `SMEMBERS`               | `SADD {key} {m1} {m2} ...`                |
| zset   | `ZRANGE 0 -1 WITHSCORES` | `ZADD {key} {s1} {m1} {s2} {m2} ...`      |
| stream | `XRANGE - +`             | 每 entry：`XADD {key} {id} {f1} {v1} ...` |
| json   | `JSON.GET`               | `JSON.SET {key} $ {json}`                 |

- 空 HASH / 空 LIST 等：返回空字符串，前端提示「空键，无可复制命令」
- 空 SET：`SADD key` 无 member 不合法 → 同上提示或省略（产品取「提示空键」）

### 4.3 大键与命令行长度

**本期不做特殊处理**：无论键多大，均按 §2.1 输出单条命令（STREAM 仍为每 entry 一行 `XADD`），不弹确认、不自动拆分、不设长度阈值。

### 4.4 Hash 单字段模式

首期：**始终复制整键**（与全量读取一致）。`withHashKey` 单字段查看时不特殊处理。

---

## 五、架构

```
RedisValue.vue  dropdown → copyAsCommand
        │
        ▼
meCommands.getKeyAsCommand(connId, redisKey)
        │
        ▼
Rust: format_key_as_command(conn, key) -> String
        ├─ TYPE → 分支读取
        ├─ redis_cli_escape_arg(&[u8]) -> String   // 键/值/field 共用
        └─ 按类型拼一条（或 STREAM 多条）命令
        │
        ▼
meCopy(text)
```

核心函数建议放在 `src-tauri/src/utils/`（如 `redis_cli_format.rs`），便于单测；`client_trait.rs` 负责按类型拉数并调用。

---

## 六、与导入 / 终端的兼容性

现有 `import_cmd` → `parse_command` → `split_redis_args`，已支持：

- 双引号与 `\xHH`、`\n`、`\r`、`\t`
- 单引号参数（JSON 场景）

复制输出 **无需** 额外导入适配；**不要** 引入 `BASE64:` 等非 redis-cli 语法。

**STREAM 多行：** 导入命令按行执行，多行 `XADD` 自然支持。

**redis-cli 直接执行：**

```bash
redis-cli SET binkey "\x00\x01\xffhello"
redis-cli HMSET h f1 "v1" f2 "v2"
```

与应用内终端行为一致。

---

## 七、实现步骤

### Phase 1 — Rust

1. [x] `redis_cli_format.rs` + 单元测试
2. [x] `format_key_as_command` / `key_as_command_lines`
3. [x] 注册 `get_key_as_command` API（Specta）

### Phase 2 — 前端

1. [x] `RedisValue.vue` 下拉 `copyAsCommand` + `onKeyMoreCommand`
2. [x] i18n：`redisValue.copyAsCommand` 等

---

## 八、修改文件清单

| 文件                                                      | 变更                        |
| --------------------------------------------------------- | --------------------------- |
| `src-tauri/src/utils/redis_cli_format.rs`（新建）         | 转义 + 格式化 + 测试        |
| `src-tauri/src/client/client_trait.rs`                    | 按类型读 Redis + 调用格式化 |
| `src-tauri/src/api.rs`                                    | `get_key_as_command`        |
| `src-tauri/src/client/impl_single.rs` / `impl_cluster.rs` | trait 转发                  |
| `src/views/tab/RedisValue.vue`                            | 下拉菜单项 + handler        |
| `src/locales/lang/zh-cn.ts` / `en.ts`                     | 文案                        |

---

## 九、测试用例

### Rust 单元测试

| 输入 bytes       | 期望片段                                     |
| ---------------- | -------------------------------------------- |
| `Line01\nLine02` | `"Line01\\nLine02"`                          |
| `\x00\x01\xff`   | `"\x00\x01\xff"`                             |
| `hello`          | `hello` 或 `"hello"`（无特殊字符可不加引号） |
| `"quoted`        | 含 `\"`                                      |

### 集成

1. 各类型写入测试键 → 复制 → `import_cmd` 到新 DB → 数据一致
2. 复制结果写入文件 → `redis-cli < file` → 验证
3. STRING 二进制 ↔ `\xHH` 往返

---

## 十、命令示例汇总

```redis
SET "MultiLine" "Line01\nLine02"

SET binkey "\xda\xb8\xe6"

HMSET user:1 name "张三" age "28"

RPUSH mylist "a" "b" "c"

SADD myset "m1" "m2"

ZADD rank 1.5 "playerA" 2 "playerB"

XADD mystream 1700000000000-0 field1 "v1" field2 "v2"
XADD mystream 1700000000001-0 field1 "v3"

JSON.SET doc $ '{"name":"test"}'
```

---

## 十一、决策摘要

| 问题              | 决策                                                                   |
| ----------------- | ---------------------------------------------------------------------- |
| 单条还是多条？    | HASH/LIST/SET/ZSET/STRING/JSON **单条**；STREAM **每 entry 一行 XADD** |
| HASH 用啥？       | **HMSET**                                                              |
| TTL？             | **不考虑**                                                             |
| 入口？            | 顶部下拉，**创建副本下方**                                             |
| 二进制？          | **redis-cli `\xHH` 转义**，不用 BASE64 前缀                            |
| 数据来源？        | **后端全量读**                                                         |
| 大键 / 命令过长？ | **不特殊处理**，原样输出单条                                           |
