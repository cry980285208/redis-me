# RedisME 命令格式导出功能 - 实现方案

> **实现状态**：✅ 已实现  
> **关键代码**：`src/views/key/KeyBatch.vue`、`client_trait.rs`（`export_keys_as_command` / `import_cmds`）、`redis_cli_format.rs`  
> **实际实现**：导出格式 **csv**（DUMP/RESTORE）与 **cmd**（redis-cli 命令，`.redis` 后缀）；默认 csv；`import_cmd` 逐行解析执行；二进制用 `\xHH` 转义。

## 一、需求分析

### 1.1 当前问题

现有导出功能使用 Redis 的 `DUMP` + `RESTORE` 命令:

```
导出: DUMP key → Base64编码 → CSV文件 (key,value,ttl)
导入: 读取CSV → Base64解码 → RESTORE key ttl serialized-value
```

**存在的问题:**

- `DUMP` 命令生成的是 RDB 格式的二进制数据
- 不同 Redis 版本的 RDB 格式可能**不兼容** (RDB version 差异)
- 导出的文件是**二进制编码**,不可读、不可编辑
- 无法直接在 redis-cli 或其他工具中使用

### 1.2 用户需求

1. **导出为命令格式**: 导出为 `SET key "value"` 这样的原始 Redis 命令
2. **保留旧方式**: DUMP 格式完全保留,不影响现有功能
3. **复制为命令**: 选中键后,可一键复制为命令,方便粘贴使用
4. **版本兼容性好**: Redis 命令在各版本间兼容性好
5. **可读可编辑**: 导出的文件可用文本编辑器查看和修改
6. **可直接执行**: 可用 `redis-cli < file.redis` 直接导入

---

## 二、实际导出格式（与代码对齐）

### 2.1 CMD 格式（redis-cli 风格）

**无** 文件头、`--` 注释、`BASE64:` 前缀。每行一条可执行命令；键名/参数用双引号 + C 风格转义（`redis_cli_format.rs` 的 `format_quoted`）。

```redis
SET "mykey" "myvalue"
EXPIRE "mykey" 3600
HMSET "myhash" "field1" "value1" "field2" "value2"
RPUSH "mylist" "item1" "item2" "item3"
SADD "myset" "member1" "member2"
ZADD "myzset" 1 "member1" 2 "member2"
XADD "mystream" "1700000000000-0" "field1" "value1" "field2" "value2"
JSON.SET "myjson" $ "{\"a\":1}"
SET "mybinary" "\x00\x01\xff"
```

**格式特点：**

- 纯命令行，可用 `redis-cli < file.redis` 导入
- HASH/LIST/SET/ZSET 各 **一条** 聚合命令（HMSET/RPUSH/SADD/ZADD），非多条 HSET/RPUSH
- 二进制一律 `\xHH` 转义，与 redis-cli 一致
- STREAM 使用实际 entry id（XRANGE 读出），非 `*`
- 勾选 TTL 时在键命令后追加 `EXPIRE key seconds`
- 文件后缀 `.redis`（`KeyBatch.vue`）

### 2.2 CSV 格式（DUMP/RESTORE，保留）

```
<base64-key>,<base64-dump>,<ttl-seconds>
```

默认导出格式为 **csv**；cmd 为可选（`KeyBatch.vue` `exportFormat: 'csv'`）。

### 2.3 数据类型映射

| Redis 类型 | 导出命令                   | 说明                     |
| ---------- | -------------------------- | ------------------------ |
| STRING     | `SET key "value"`          | 二进制用 `\xHH`          |
| HASH       | `HMSET key f1 v1 f2 v2 …`  | 单条多 field             |
| LIST       | `RPUSH key v1 v2 …`        | 单条，顺序与 LRANGE 一致 |
| SET        | `SADD key m1 m2 …`         | 单条                     |
| ZSET       | `ZADD key score1 m1 …`     | 单条                     |
| STREAM     | 每条 `XADD key id f1 v1 …` | 多条，id 来自 XRANGE     |
| JSON       | `JSON.SET key $ "…"`       | RedisJSON                |
| 其他       | 跳过并记 err               | Bloom 等                 |

### 2.4 导入

- **CSV**：`RESTORE`（`import_keys`），支持冲突/TTL 策略
- **CMD**：`import_cmds` 逐行 `parse_command` + `Cmd::exec`（`client_trait.rs`），**不**解析 `BASE64:` 前缀

---

## 三、技术实现

> 下文部分伪代码仍含早期设计（文件头注释、`BASE64:` 等），**以实现代码为准**，见 §2 与 `redis_cli_format.rs` / `export_key_as_command`。

### 3.1 后端: Rust 实现

#### 3.1.1 修改数据模型

**文件: `src-tauri/src/utils/model.rs`**

```rust
// 导出
api_model!(RedisExportCsv {
    #[serde(rename = "match")]
    pattern: String,
    key_list: Vec<RedisKey>,
    file: String,
    with_ttl: bool,
    export_format: String,  // 新增: "command" 或 "dump", 默认 "command"
});
```

#### 3.1.2 核心导出函数

**文件: `src-tauri/src/client/client_trait.rs`**

```rust
use std::io::{BufWriter, Write};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

/// 批量导出为命令格式
pub fn export_keys_as_command(
    mut conn: impl Commands,
    key_list: Vec<RedisKey>,
    file: &str,
    with_ttl: bool,
    running: Arc<AtomicBool>,
    app_handle: AppHandle,
    id: String,
) -> AnyResult<()> {
    info!("export keys as command format, file: {}", file);
    let mut writer = BufWriter::new(File::create(file)?);

    // 写入文件头
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    writeln!(writer, "-- RedisME Command Export")?;
    writeln!(writer, "-- Exported at: {}", now)?;
    writeln!(writer, "-- Keys: {}", key_list.len())?;
    writeln!(writer)?;

    let mut ok_count = 0;
    let mut err_count = 0;
    let total_count = key_list.len() as u64;

    for key in key_list {
        if running.load(Relaxed) {
            let result = export_key_as_command(&mut conn, &mut writer, key, with_ttl);
            match result {
                Ok(_) => ok_count += 1,
                Err(e) => {
                    warn!("export key as command err: {}", e);
                    err_count += 1;
                }
            }

            // 通知导出进度
            let event = ExportImportEvent {
                id: id.clone(),
                ok_count,
                err_count,
                total_count,
                ignore_count: 0,
                finished: false,
            };
            let _ = app_handle.emit(EVENT_EXPORT, event);
        }
    }

    // 完成通知
    let event = ExportImportEvent {
        id: id.clone(),
        ok_count,
        err_count,
        total_count,
        ignore_count: 0,
        finished: true,
    };
    let _ = app_handle.emit(EVENT_EXPORT, event);
    writer.flush()?;
    Ok(())
}

/// 导出单个键为命令格式
fn export_key_as_command(
    conn: &mut impl Commands,
    writer: &mut BufWriter<File>,
    key: RedisKey,
    with_ttl: bool,
) -> AnyResult<()> {
    // 获取键类型
    let key_type: String = redis::cmd("TYPE")
        .arg(&key)
        .query(conn)?;

    // 写入类型注释
    writeln!(writer, "-- {}: {}", key_type.to_uppercase(), key)?;

    // 根据类型导出
    match key_type.as_str() {
        "string" => export_string_as_command(conn, writer, &key)?,
        "hash" => export_hash_as_command(conn, writer, &key)?,
        "list" => export_list_as_command(conn, writer, &key)?,
        "set" => export_set_as_command(conn, writer, &key)?,
        "zset" => export_zset_as_command(conn, writer, &key)?,
        "stream" => export_stream_as_command(conn, writer, &key)?,
        "ReJSON-RL" => export_json_as_command(conn, writer, &key)?,
        other => {
            warn!("unsupported key type: {}, skipping key: {}", other, key);
            writeln!(writer, "-- UNSUPPORTED TYPE: {}", other)?;
        }
    }

    // 导出 TTL
    if with_ttl {
        let ttl: i64 = conn.ttl(&key)?;
        if ttl > 0 {
            writeln!(writer, "EXPIRE {} {}", escape_identifier(&key), ttl)?;
        }
    }

    writeln!(writer)?; // 空行分隔
    Ok(())
}

/// 导出 STRING 类型
fn export_string_as_command(
    conn: &mut impl Commands,
    writer: &mut BufWriter<File>,
    key: &str,
) -> AnyResult<()> {
    let value: Vec<u8> = redis::cmd("GET").arg(key).query(conn)?;
    let key_escaped = escape_identifier(key);

    if is_binary_data(&value) {
        // 二进制数据,使用 BASE64 编码
        let encoded = BASE64.encode(&value);
        writeln!(writer, "SET {} BASE64:{}", key_escaped, encoded)?;
    } else {
        // 普通字符串
        let value_str = String::from_utf8_lossy(&value);
        writeln!(writer, "SET {} {}", key_escaped, escape_value(&value_str))?;
    }
    Ok(())
}

/// 导出 HASH 类型
fn export_hash_as_command(
    conn: &mut impl Commands,
    writer: &mut BufWriter<File>,
    key: &str,
) -> AnyResult<()> {
    let pairs: Vec<(String, String)> = conn.hgetall(key)?;
    let key_escaped = escape_identifier(key);

    for (field, value) in pairs {
        writeln!(
            writer,
            "HSET {} {} {}",
            key_escaped,
            escape_identifier(&field),
            escape_value(&value)
        )?;
    }
    Ok(())
}

/// 导出 LIST 类型
fn export_list_as_command(
    conn: &mut impl Commands,
    writer: &mut BufWriter<File>,
    key: &str,
) -> AnyResult<()> {
    let items: Vec<String> = redis::cmd("LRANGE")
        .arg(key)
        .arg(0)
        .arg(-1)
        .query(conn)?;
    let key_escaped = escape_identifier(key);

    for item in items {
        writeln!(writer, "RPUSH {} {}", key_escaped, escape_value(&item))?;
    }
    Ok(())
}

/// 导出 SET 类型
fn export_set_as_command(
    conn: &mut impl Commands,
    writer: &mut BufWriter<File>,
    key: &str,
) -> AnyResult<()> {
    let members: Vec<String> = conn.smembers(key)?;
    let key_escaped = escape_identifier(key);

    for member in members {
        writeln!(writer, "SADD {} {}", key_escaped, escape_value(&member))?;
    }
    Ok(())
}

/// 导出 ZSET 类型
fn export_zset_as_command(
    conn: &mut impl Commands,
    writer: &mut BufWriter<File>,
    key: &str,
) -> AnyResult<()> {
    let pairs: Vec<(String, f64)> = redis::cmd("ZRANGE")
        .arg(key)
        .arg(0)
        .arg(-1)
        .arg("WITHSCORES")
        .query(conn)?;
    let key_escaped = escape_identifier(key);

    for (member, score) in pairs {
        writeln!(
            writer,
            "ZADD {} {} {}",
            key_escaped,
            score,
            escape_value(&member)
        )?;
    }
    Ok(())
}

/// 导出 STREAM 类型
fn export_stream_as_command(
    conn: &mut impl Commands,
    writer: &mut BufWriter<File>,
    key: &str,
) -> AnyResult<()> {
    // 获取所有条目
    let entries: Vec<StreamInfo> = redis::cmd("XRANGE")
        .arg(key)
        .arg("-")
        .arg("+")
        .query(conn)?;
    let key_escaped = escape_identifier(key);

    for entry in entries {
        // 构建 field-value 对
        let mut fields = String::new();
        for (field, value) in entry.fields {
            fields.push_str(&format!(" {} {}",
                escape_identifier(&field),
                escape_value(&value)
            ));
        }
        writeln!(writer, "XADD {} *{}", key_escaped, fields)?;
    }
    Ok(())
}

/// 导出 JSON 类型 (RedisJSON)
fn export_json_as_command(
    conn: &mut impl Commands,
    writer: &mut BufWriter<File>,
    key: &str,
) -> AnyResult<()> {
    let json_str: String = redis::cmd("JSON.GET")
        .arg(key)
        .query(conn)?;
    let key_escaped = escape_identifier(key);

    writeln!(writer, "JSON.SET {} $ {}", key_escaped, &json_str)?;
    Ok(())
}

/// 检查是否为二进制数据
fn is_binary_data(value: &[u8]) -> bool {
    value.iter().any(|&b| b < 0x20 && b != 0x09 && b != 0x0A && b != 0x0D)
}

/// 转义标识符 (键名、字段名)
fn escape_identifier(s: &str) -> String {
    // 如果包含空格或特殊字符,添加引号
    if s.contains(|c: char| c.is_whitespace() || c == '"' || c == '\\') {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

/// 转义值字符串
fn escape_value(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\")
                         .replace('"', "\\\"")
                         .replace('\n', "\\n")
                         .replace('\r', "\\r")
                         .replace('\t', "\\t"))
}
```

#### 3.1.3 单键复制 API（已实现，见 08）

**实际 API**（`api.rs`）：`get_key_as_command(key: RedisKey) -> String`  
实现：`client_trait.rs` 的 `get_key_as_command0` → `key_as_command_lines`。  
入口：`RedisValue.vue` 扩展菜单。无批量 `get_keys_commands`、无 `KeyMain` 批量复制。

#### 3.1.4 修改导出调度函数

**文件: `src-tauri/src/client/impl_single.rs` 和 `impl_cluster.rs`**

在 `export_csv` 方法中,根据 `export_format` 字段选择导出方式:

```rust
fn export_csv(&self, app_handle: AppHandle, param: RedisExportCsv) -> AnyResult<()> {
    let mut conn = self.get_conn()?;
    let key_list = if param.key_list.is_empty() {
        self.scan_keys(param.pattern.clone())?
    } else {
        param.key_list.clone()
    };

    let running = self.export_import_running.clone();
    export_import_check_running(running.clone())?;

    let id = param.pattern.clone();
    std::thread::spawn({
        let running = running.clone();
        let app_handle = app_handle.clone();
        move || {
            match param.export_format.as_str() {
                "command" => {
                    export_keys_as_command(
                        &mut conn, key_list, &param.file,
                        param.with_ttl, running, app_handle, id
                    );
                }
                _ => {
                    // 默认使用原有的 DUMP 格式
                    export_csv_0_thread(
                        &mut conn, key_list, param.file,
                        param.with_ttl, running, app_handle, id
                    );
                }
            }
        }
    });

    Ok(())
}
```

---

### 3.2 前端: Vue 实现

#### 3.2.1 导出对话框添加格式选择

**文件: `src/views/key/KeyBatch.vue`**

```vue
<script setup>
// 在 initForm 中添加
const initForm = readonly({
  match: '',
  keyList: [],
  deleteDirect: false,

  file: '',
  withTtl: true,
  exportFormat: 'command', // 新增: 'command' 或 'dump'
})

// 修改文件后缀建议
const exportFileSuffix = computed(() => (form.value.exportFormat === 'command' ? 'redis' : 'csv'))
</script>

<template>
  <el-form ref="formRef" :model="form" :rules="rules" label-position="top">
    <!-- 现有表单项... -->

    <!-- 新增: 导出格式选择 -->
    <el-form-item :label="t('keyBatch.exportFormat')" v-if="isExport">
      <el-radio-group v-model="form.exportFormat">
        <el-radio value="command">
          {{ t('keyBatch.formatCommand') }}
        </el-radio>
        <el-radio value="dump">
          {{ t('keyBatch.formatDump') }}
        </el-radio>
      </el-radio-group>
    </el-form-item>

    <!-- 修改: 文件输入框,根据格式建议后缀 -->
    <el-form-item :label="t('keyBatch.exportFile')" v-if="isExport" prop="file">
      <me-file-input
        v-model="form.file"
        :placeholder="t('keyBatch.exportFileTip')"
        :file-prefix="exportFilePrefix"
        :file-suffix="exportFileSuffix" />
    </el-form-item>

    <!-- 现有表单项... -->
  </el-form>
</template>
```

#### 3.2.2 新增"复制为命令"功能

**文件: `src/views/KeyMain.vue`**

```vue
<script setup>
import { meCopy, meCommands, meErr, meOk } from '@/utils/util'

// 新增: 复制选中的键为命令
async function copyCheckedAsCommand() {
  if (checkedKeyList.value.length === 0) return

  try {
    loading.value = true
    const commands = await meCommands.getKeysCommands(share.conn!.id, checkedKeyList.value)

    const commandText = commands.join('\n')
    meCopy(commandText, t('keyMain.copyCommandOk'))
  } catch (e) {
    meErr(e, t('keyMain.copyCommandErr'))
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <!-- 在多操作区域添加按钮 -->
  <div class="me-flex" v-else style="width: 70px; margin-left: 10px">
    <!-- 现有按钮... -->

    <!-- 新增: 复制为命令 -->
    <el-link underline="never" :disabled="checkedDisabled" @click="copyCheckedAsCommand">
      <me-icon
        :name="t('keyMain.copyAsCommand')"
        icon="el-icon-document-copy"
        hint
        :class="checkedBtnClass"
        placement="top" />
    </el-link>

    <!-- 现有按钮... -->
  </div>
</template>
```

#### 3.2.3 国际化文本

**文件: `src/locales/lang/zh-cn.js`**

```javascript
export default {
  keyBatch: {
    exportFormat: '导出格式',
    formatCommand: '命令格式 (推荐)',
    formatDump: 'DUMP 格式 (兼容旧版)',
    // ... 现有文本
  },

  keyMain: {
    copyAsCommand: '复制为命令',
    copyCommandOk: '已复制 {count} 个键的命令',
    copyCommandErr: '复制命令失败',
    // ... 现有文本
  },
}
```

**文件: `src/locales/lang/en.js`**

```javascript
export default {
  keyBatch: {
    exportFormat: 'Export Format',
    formatCommand: 'Command Format (Recommended)',
    formatDump: 'DUMP Format (Legacy)',
    // ... existing text
  },

  keyMain: {
    copyAsCommand: 'Copy as Commands',
    copyCommandOk: 'Copied {count} keys as commands',
    copyCommandErr: 'Failed to copy commands',
    // ... existing text
  },
}
```

---

## 四、导入功能适配

### 4.1 自动识别文件格式

导入时自动检测文件格式:

```rust
fn detect_file_format(file: &str) -> &'static str {
    if file.ends_with(".redis") {
        return "command";
    }
    if file.ends_with(".csv") {
        return "dump";
    }
    // 读取文件首行检测
    // 如果以 "--" 开头,则为命令格式
    "dump" // 默认
}
```

### 4.2 命令格式导入

**文件: `src-tauri/src/client/client_trait.rs`**

```rust
fn import_keys_from_command_file(
    conn: &mut impl Commands,
    file: &str,
    running: Arc<AtomicBool>,
    app_handle: AppHandle,
    id: String,
) -> AnyResult<()> {
    let reader = BufReader::new(File::open(file)?);
    let mut ok_count = 0;
    let mut err_count = 0;
    let mut ignore_count = 0;

    for line in reader.lines() {
        let line = line?.trim().to_string();

        // 跳过注释和空行
        if line.is_empty() || line.starts_with("--") {
            continue;
        }

        if running.load(Relaxed) {
            let result = execute_command(conn, &line);
            match result {
                Ok(_) => ok_count += 1,
                Err(e) => {
                    if e.to_string().contains("Target key name") {
                        ignore_count += 1;
                    } else {
                        warn!("import command err: {}", e);
                        err_count += 1;
                    }
                }
            }
        }
    }

    Ok(())
}

fn execute_command(conn: &mut impl Commands, command: &str) -> AnyResult<()> {
    // 解析命令并执行
    // 例如: SET key "value" → conn.set(key, value)
    // 例如: SET key BASE64:xxx → conn.set(key, base64_decode(xxx))
    unimplemented!()
}
```

---

## 五、修改文件清单（已实现）

| 文件                                 | 说明                                                                                     |
| ------------------------------------ | ---------------------------------------------------------------------------------------- |
| `client_trait.rs`                    | `export_keys_as_command`、`export_key_as_command`、`key_as_command_lines`、`import_cmds` |
| `redis_cli_format.rs`                | `format_quoted`、各类型 `format_*_command`                                               |
| `impl_single.rs` / `impl_cluster.rs` | `export_format == "cmd"` 分支                                                            |
| `api.rs` / `lib.rs`                  | `import_cmd`、`get_key_as_command`                                                       |
| `KeyBatch.vue`                       | 导出格式 csv/cmd 分段选择                                                                |
| `RedisValue.vue`                     | 单键复制为命令（见 08）                                                                  |
| `locales/lang/zh-cn.ts`、`en.ts`     | `keyBatch.exportFormat*` 等                                                              |

**总计:** 约 +540 行代码

---

## 六、测试计划

### 6.1 单元测试

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_binary_data() {
        assert!(!is_binary_data(b"hello world"));
        assert!(is_binary_data(b"hello\x00world"));
        assert!(!is_binary_data(b"hello\tworld")); // 制表符不算
    }

    #[test]
    fn test_escape_value() {
        assert_eq!(escape_value("hello"), "\"hello\"");
        assert_eq!(escape_value("he\"llo"), "\"he\\\"llo\"");
        assert_eq!(escape_value("he\nllo"), "\"he\\nllo\"");
    }

    #[test]
    fn test_escape_identifier() {
        assert_eq!(escape_identifier("mykey"), "mykey");
        assert_eq!(escape_identifier("my key"), "\"my key\"");
    }
}
```

### 6.2 集成测试

1. **导出测试:**
   - 导出 STRING 类型 (普通字符串、二进制)
   - 导出 HASH 类型 (多个字段)
   - 导出 LIST/SET/ZSET 类型
   - 导出带 TTL 的键
   - 导出空键

2. **导入测试:**
   - 导入命令格式文件
   - 导入 DUMP 格式文件 (向后兼容)
   - 导入混合类型文件

3. **复制测试:**
   - 复制单个键为命令
   - 复制多个键为命令
   - 复制到剪贴板后粘贴验证

---

## 七、性能考虑

### 7.1 大键处理

对于包含大量元素的键 (如 HASH 有 10000 个字段):

```rust
// 分批获取数据
fn export_hash_as_command(...) -> AnyResult<()> {
    let mut cursor = 0;
    loop {
        let result: (i64, Vec<(String, String)>) = redis::cmd("HSCAN")
            .arg(key)
            .arg(cursor)
            .arg("COUNT")
            .arg(1000)
            .query(conn)?;

        cursor = result.0;
        let pairs = result.1;

        for (field, value) in pairs {
            writeln!(writer, "HSET {} {} {}", ...)?;
        }

        if cursor == 0 {
            break;
        }
    }
    Ok(())
}
```

### 7.2 进度通知

导出过程中定期发送进度事件:

```rust
// 每处理 100 个键发送一次进度
if ok_count % 100 == 0 {
    let event = ExportImportEvent { ... };
    let _ = app_handle.emit(EVENT_EXPORT, event);
}
```

### 7.3 阻塞风险

- `GET`/`HGETALL`/`LRANGE` 等命令可能阻塞 Redis
- 对于超大键,应考虑分批获取
- 前端显示进度提示,告知用户可能的延迟

---

## 八、用户体验流程

### 8.1 场景 1: 批量导出

```
1. KeyMain → 更多 → 批量导出（KeyBatch.vue）
2. 选择导出格式：csv（默认）/ cmd
3. 可选包含 TTL；cmd 后缀 .redis，csv 后缀 .csv
4. 确认 → 进度事件 → 完成统计
```

### 8.2 场景 2: 复制为命令

```
1. 打开键值页 RedisValue.vue
2. 扩展菜单 → 复制为命令
3. 后端 get_key_as_command 全量读取 → 剪贴板
```

> 键列表多选批量复制 **未实现**（原设计草案）。

---

## 九、优势总结

| 特性           | DUMP 格式         | 命令格式 (新)                    |
| -------------- | ----------------- | -------------------------------- |
| **版本兼容性** | ❌ 可能不兼容     | ✅ 兼容性好                      |
| **可读性**     | ❌ 二进制编码     | ✅ 纯文本                        |
| **可编辑性**   | ❌ 不可编辑       | ✅ 可编辑                        |
| **直接执行**   | ❌ 需要 RESTORE   | ✅ redis-cli 可直接执行          |
| **文件大小**   | ✅ 较小 (二进制)  | ⚠️ 较大 (文本)                   |
| **导出速度**   | ✅ 快 (DUMP 命令) | ⚠️ 慢 (需要 GET 等)              |
| **二进制支持** | ✅ 原生支持       | ✅ `\xHH` 转义（redis-cli 风格） |

**推荐使用场景:**

- ✅ **迁移数据**: 使用命令格式,避免版本兼容问题
- ✅ **备份恢复**: 使用命令格式,可读可编辑
- ✅ **快速分享**: 使用"复制为命令",方便粘贴
- ⚠️ **大量数据备份**: 使用 DUMP 格式,速度更快

---

## 十、未来扩展

### 10.1 导入命令格式文件

✅ **已实现**：`import_cmd` / `import_cmds` 逐行 `parse_command` + 执行（见 §2.4）。

### 10.2 导出为脚本文件

- 支持导出为 Python/Shell 脚本
- 包含连接信息和错误处理
- 可直接运行自动化脚本

### 10.3 增量导出

- 只导出上次导出后变更的键
- 结合 `MONITOR` 或 `COMMAND LOG` 实现

---

## 附录: 命令格式示例（与实际导出一致）

### 完整导出示例

```redis
SET "session:user:1001" "{\"userId\":1001,\"token\":\"abc123\"}"
EXPIRE "session:user:1001" 3600
HMSET "user:1001" "name" "张三" "email" "zhangsan@example.com" "age" "28"
RPUSH "notifications:user:1001" "新订单通知" "系统更新通知" "活动提醒"
SADD "tags:article:2001" "Redis" "数据库" "缓存"
ZADD "leaderboard:game:3001" 9850 "玩家A" 9720 "玩家B" 9680 "玩家C"
XADD "events:order" "1700000000000-0" "event" "created" "userId" "1001" "amount" "199.99"
SET "image:avatar:1001" "\x89PNG\r\n\x1a\n..."
SET "counter:visits" "1234567"
```

### 单键复制示例（`RedisValue.vue` 扩展菜单，见 08）

```redis
HMSET "user:1001" "name" "张三" "email" "zhangsan@example.com" "age" "28"
```

---

**文档版本:** v1.2  
**状态:** ✅ 已实现（批量导出/导入 CSV+CMD；单键复制见 `08_copy-key-as-command.md`）
