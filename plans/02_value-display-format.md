# 数据查看器增强 - 多格式显示方案

> **实现状态**：✅ 已实现  
> **关键代码**：`src/utils/format.ts`、`src/views/tab/RedisValue.vue`  
> **实际实现**：hex / msgpack / strjson / custom 均为前端视图格式；IPC 层 `BytesFormat` 仅 `utf8` / `base64`；无后端 `DisplayFormat` 与 `field_scan(display_format)`。

## 背景

当前 Redis 数据查看器只显示字符串格式，后端使用 `vec8_to_display_string()` 将 `Vec<u8>` 通过 `String::from_utf8_lossy()` 转换为字符串。对于包含非 UTF-8 字节的二进制数据，无效字节会被替换为 ``（U+FFFD 替换字符），导致原始数据信息丢失。

### 需求

支持多种显示格式，让用户能够精确查看和编辑二进制数据：

- **UTF8**（`utf8`）：默认字符串显示
- **Hex**（`hex`）：十六进制，如 `00 FF 80`
- **Base64**（`base64`）：Base64 编码
- **MsgPack** / **StrJson**：仅 STRING 整键可选，前端解码展示
- **Custom**（`custom:{name}`）：自定义 shell 脚本编解码（见 05）

> 无 **Binary** 位视图；二进制内容用 Hex 或 `\xHH` 混合展示。

---

## 主流 Redis GUI 实现分析

### AnotherRDM (Another Redis Desktop Manager)

**技术栈**：Electron + Vue + Node.js

**实现方式**：前端转换

```
Redis → Node.js redis 客户端返回 Buffer → 前端 → bufToHex/bufToString 转换 → 显示
```

**关键设计**：

1. **后端返回原始 Buffer**：Node.js 的 `redis` 客户端天然返回 `Buffer` 对象
2. **前端自由转换**：接收 Buffer 后调用 `bufToHex()` 或 `bufToString()` 转换
3. **智能检测**：`bufVisible()` 判断数据是否可无损转为字符串，否则自动降级为 Hex 视图
4. **混合显示**：Hex 视图中 ASCII 可打印字符（32~126）保留原文，其他显示为 `\xHH`

**核心代码**（简化版）：

```javascript
// src/utils/util.ts
bufToHex(buffer) {
  return buffer.reduce((str, byte) => {
    const hex = byte.toString(16).padStart(2, '0').toUpperCase();
    const char = byte >= 32 && byte <= 126 ? String.fromCharCode(byte) : '.';
    return str + `\\x${hex}`;  // 或 hex + ' ' 纯十六进制
  }, '');
}

bufVisible(buffer) {
  // 判断是否可无损转为字符串
  return buffer.every(b => b >= 32 && b <= 126 || [9,10,13].includes(b));
}
```

**优势**：

- Electron 架构下 Buffer 可直接传递，无序列化开销
- 前端自由切换格式，无需再次请求后端
- 混合显示兼顾精确性和可读性

**局限**：

- 依赖 Node.js Buffer，不适用于 Webview2/Tauri 等需要 JSON 序列化的架构

---

### TinyRDM (Tiny Redis Desktop Manager)

**技术栈**：Wails + Vue + Go

**实现方式**：后端转换 + 前端触发

```
Redis → Go 获取原始值 → 前端请求 ConvertValue → Go 后端按格式转换 → 返回字符串 → 显示
```

**关键设计**：

1. **后端转换器注册表**：`backend/utils/convert/convert.go`

   ```go
   var BuildInFormatters = map[string]DataConvert{
       "Raw":    rawConvert,
       "Hex":    hexConvert,
       "Binary": binaryConvert,
       "JSON":   jsonConvert,
       "YAML":   yamlConvert,
       "XML":    xmlConvert,
       // ...
   }
   ```

2. **ConvertValue API**：切换格式时调用，对已有值转换，不重新从 Redis 获取

   ```go
   func (b *browserService) ConvertValue(value any, decode, format string) (resp types.JSResp) {
       str := strutil.DecodeRedisKey(value)
       value, decode, format = convutil.ConvertTo(str, decode, format)
       resp.Data = map[string]any{"value": value, "decode": decode, "format": format}
   }
   ```

3. **集合类型的 DisplayValue**：List/Hash/Set/ZSet 每个元素有独立 `DisplayValue` 字段存储转换后的值

   ```go
   if doConvert {
       if dv, _, _ := ConvertTo(val, param.Decode, param.Format); dv != val {
           items[len(items)-1].DisplayValue = dv
       }
   }
   ```

4. **自动检测**：打开键时 `autoViewAs()` 自动检测二进制内容，建议用 Hex 显示
   ```go
   func autoViewAs(str string) (value, resultFormat string) {
       if strutil.ContainsBinary(str) {
           if value, ok = hexConv.Decode(str); ok {
               resultFormat = types.FORMAT_HEX
               return
           }
       }
       return str, types.FORMAT_RAW
   }
   ```

**优势**：

- 切换格式零延迟（不调用 Redis，仅对已有值转换）
- Go 的 `string` 底层是 `[]byte`，不会丢失任何字节信息
- 完整的解码支持（Base64/GZip/MsgPack 等）+ 格式化

**局限**：

- Go 字符串天然兼容二进制，不需要额外处理
- 需要维护转换器和注册表

---

## 方案对比

| 维度           | AnotherRDM         | TinyRDM                  | RedisME（方案）       |
| -------------- | ------------------ | ------------------------ | --------------------- |
| **技术栈**     | Electron + Node.js | Wails + Go               | Tauri + Rust          |
| **转换位置**   | 前端（JavaScript） | 后端（Go）               | **前端（format.ts）** |
| **数据传输**   | Buffer 对象        | JSON 字符串              | JSON + base64 wire    |
| **切换方式**   | 前端本地转换       | ConvertValue API         | **前端本地转换**      |
| **二进制保真** | 100%（Buffer）     | 100%（Go string=[]byte） | 100%（base64 wire）   |
| **切换延迟**   | 0ms（前端）        | 0ms（后端转换）          | 0ms（前端）           |
| **改动量**     | -                  | -                        | ~75行                 |

### 为什么 RedisME 选择重新 field_scan 而非 ConvertValue？

**核心问题**：Rust 的 `String::from_utf8_lossy()` 会将无效 UTF-8 字节替换为 ``（U+FFFD），信息永久丢失。

如果首次返回字符串，后续转换只能对 `` 操作，无法还原原始字节。

**可选方案对比**：

| 方案                 | 优势                               | 劣势                                 | 复杂度     |
| -------------------- | ---------------------------------- | ------------------------------------ | ---------- |
| **重新 field_scan**  | 每次都获取新鲜原始字节，零缓存问题 | 需要重新请求 Redis                   | 最低       |
| **前端本地转换**     | 零延迟，零后端调用                 | 首次需返回 base64 wire，前端内存占用 | **已采用** |
| **后端缓存原始字节** | 切换快                             | 需要缓存过期/淘汰策略，内存泄漏风险  | 最高       |

**结论**：采用 **前端视图格式 + IPC wire（utf8/base64）**。后端 `field_scan` 仍返回原始字节（base64 编码传输），hex/msgpack/strjson/custom 均在前端 `format.ts` 转换，切换格式无需重新请求 Redis。

---

## 实际实现（与代码对齐）

### 架构

```
Redis field_scan / get → 后端 BytesFormat（utf8 | base64）→ 前端 wire 缓存
    ↓
RedisValue / FieldSet 选择 ViewBytesFormat（utf8 | hex | base64 | msgpack | strjson | custom:*）
    ↓
format.ts：meFormatViewValue / meViewToWireAsync 本地编解码 → 展示或写回
```

### 类型与文件

| 层                     | 说明                                                                                    |
| ---------------------- | --------------------------------------------------------------------------------------- |
| IPC `BytesFormat`      | `utf8` / `base64`（`tauri-specta.ts`）                                                  |
| 前端 `ViewBytesFormat` | `utf8` \| `hex` \| `base64` \| `msgpack` \| `strjson` \| `custom:{name}`（`format.ts`） |
| 键重命名等             | `BYTES_FORMAT = ['UTF8','Hex','Base64']`                                                |
| STRING 扩展            | `EXT_FORMAT = ['StrJson','MsgPack']` + settings.customCodecs                            |
| 字段弹窗               | `fieldViewOptions`：MsgPack/custom 在 field 级降级为 utf8（`viewFmtForField`）          |

### 关键组件

- `src/utils/format.ts` — 视图/wire 转换、custom codec shell 执行
- `src/views/tab/RedisValue.vue` — 值区格式下拉
- `src/views/key/FieldSet.vue` — 字段编辑格式（含 custom 选项，field 级不跑脚本）

### 未实现（相对初稿）

- 后端 `DisplayFormat` 枚举与 `field_scan(display_format=…)`
- Binary 位视图
- 切换格式时重新 `field_scan` 的后端格式化路径

---

## 原设计草案（未按此实现）

> 以下「最终设计 / Rust 端 / 改动清单」为早期后端格式化方案，**已废弃**；实现以上文「实际实现」为准。

### 架构（草案，未采用）

```
用户选择格式(Hex) → display_format="hex"
    ↓
field_scan(display_format="hex")   ← 未实现
    ↓
Rust: format_bytes(bytes, "hex")     ← 未实现
```

### Rust 端设计

#### 1. 显示格式枚举

```rust
// src-tauri/src/utils/model.rs 或 util.rs

/// 数据显示格式
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DisplayFormat {
    #[default]
    String,   // 默认字符串（UTF-8 lossy）
    Hex,      // 十六进制：00 FF 80
    Binary,   // 二进制：00000000 11111111 10000000
    Base64,   // Base64 编码
}

impl DisplayFormat {
    pub fn from_str(s: &str) -> Self {
        match s {
            "hex" => DisplayFormat::Hex,
            "binary" => DisplayFormat::Binary,
            "base64" => DisplayFormat::Base64,
            _ => DisplayFormat::String,
        }
    }
}
```

#### 2. FieldScanParam 增加 display_format

```rust
api_model!(FieldScanParam {
    key: RedisKey,
    hash_key: Option<String>,
    count: u64,
    cursor: Option<ScanCursor>,
    load_all: bool,
    meta: Option<FiledScanMeta>,
    display_format: Option<DisplayFormat>,  // 新增
});
```

#### 3. format_bytes 转换函数

```rust
// src-tauri/src/utils/util.rs

use base64::prelude::BASE64_STANDARD;
use base64::Engine;

/// 按指定格式转换字节数组
pub fn format_bytes(bytes: &[u8], format: &DisplayFormat) -> String {
    match format {
        DisplayFormat::Hex => bytes
            .iter()
            .map(|b| format!("{:02X}", b))
            .collect::<Vec<_>>()
            .join(" "),
        DisplayFormat::Binary => bytes
            .iter()
            .map(|b| format!("{:08b}", b))
            .collect::<Vec<_>>()
            .join(" "),
        DisplayFormat::Base64 => BASE64_STANDARD.encode(bytes),
        DisplayFormat::String => vec8_to_display_string(bytes),
    }
}
```

#### 4. 改造现有辅助函数

```rust
// 增加 format 参数

pub fn ui_list_value(value: &[Vec<u8>], format: &DisplayFormat) -> Vec<String> {
    value.iter().map(|v| format_bytes(v, format)).collect()
}

pub fn ui_hash_value(value: &[(Vec<u8>, Vec<u8>)], format: &DisplayFormat) -> Vec<RedisHashItem> {
    value
        .iter()
        .map(|(key, value)| RedisHashItem {
            key: format_bytes(key, format),
            value: format_bytes(value, format),
            ttl: None,
        })
        .collect()
}

pub fn ui_set_value(value: HashSet<Vec<u8>>, format: &DisplayFormat) -> Vec<String> {
    value.into_iter().map(|v| format_bytes(&v, format)).collect()
}

pub fn ui_zset_value(value: Vec<(Vec<u8>, f64)>, format: &DisplayFormat) -> Vec<RedisZetItem> {
    value
        .into_iter()
        .map(|(value, score)| RedisZetItem {
            value: format_bytes(&value, format),
            score,
        })
        .collect()
}
```

#### 5. field_scan_0_get 使用格式参数

```rust
pub fn field_scan_0_get(
    mut conn: &mut MutexGuard<impl Commands>,
    param: FieldScanParam,
) -> AnyResult<(Option<serde_json::Value>, ValueType, ScanCursor)> {
    let display_format = param.display_format.as_ref().cloned().unwrap_or_default();

    match key_type {
        ValueType::String => {
            let bytes: Vec<u8> = conn.get(&key)?;
            let value = format_bytes(&bytes, &display_format);
            cc.finished = true;
            Some(serde_json::to_value(value)?)
        }
        ValueType::List => {
            let bytes_list: Vec<Vec<u8>> = conn.lrange(&key, cc.now_cursor as isize, end_index)?;
            let value = ui_list_value(&bytes_list, &display_format);
            Some(serde_json::to_value(value)?)
        }
        // ... 其他类型类似
    }
}

pub fn field_scan_2_value(
    conn: &mut impl Commands,
    key_type: &ValueType,
    scan_value: &mut FieldScanValue,
    new_value: Value,
    key: &RedisKey,
    capabilities: &ServerCapabilities,
    display_format: &DisplayFormat,  // 新增参数
) -> AnyResult<usize> {
    match key_type {
        ValueType::Hash => {
            let value: Vec<(Vec<u8>, Vec<u8>)> = FromRedisValue::from_redis_value(new_value)?;
            scan_value.hash.extend(ui_hash_value(&value, display_format));
            // ... ttl 逻辑不变
        }
        ValueType::Set => {
            let value: HashSet<Vec<u8>> = FromRedisValue::from_redis_value(new_value)?;
            scan_value.set.extend(ui_set_value(value, display_format));
        }
        ValueType::ZSet => {
            let value: Vec<(Vec<u8>, f64)> = FromRedisValue::from_redis_value(new_value)?;
            scan_value.zset.extend(ui_zset_value(value, display_format));
        }
    }
}
```

### 前端设计

#### 1. 格式选择器

```vue
<!-- src/views/tab/RedisValue.vue -->
<el-select v-model="displayFormat" size="small" style="width: 110px" @change="refreshKey">
  <el-option :label="t('redisFormat.string')" value="string" />
  <el-option :label="t('redisFormat.hex')" value="hex" />
  <el-option :label="t('redisFormat.binary')" value="binary" />
  <el-option :label="t('redisFormat.base64')" value="base64" />
</el-select>
```

#### 2. 传递格式参数

```javascript
import { meCommands } from '@/utils/util'

const displayFormat = ref('string')

async function refreshKey(reset = true, useCursor = false, loadAll = false) {
  loading.value = true
  try {
    const param = {
      key: share.redisKey,
      hashKey: hashKey.value,
      count: meTauri.settings.fieldScanCount ?? 10,
      cursor: cursor.value,
      loadAll,
      meta: meta.value,
      displayFormat: displayFormat.value,  // 新增
    }
    const data = await meCommands.fieldScan(share.conn!.id, param)
    // ... 后续不变
  }
}
```

#### 3. 国际化

```json
// src/locales/lang/zh-CN.json
{
  "redisFormat.string": "字符串",
  "redisFormat.hex": "十六进制",
  "redisFormat.binary": "二进制",
  "redisFormat.base64": "Base64"
}

// src/locales/lang/en.json
{
  "redisFormat.string": "String",
  "redisFormat.hex": "Hex",
  "redisFormat.binary": "Binary",
  "redisFormat.base64": "Base64"
}
```

---

## 改动清单

### 后端（Rust）~55 行

| 文件                                   | 改动                                                                                       | 行数 |
| -------------------------------------- | ------------------------------------------------------------------------------------------ | ---- |
| `src-tauri/src/utils/model.rs`         | 新增 `DisplayFormat` 枚举，`FieldScanParam` 增加 `display_format`                          | ~15  |
| `src-tauri/src/utils/util.rs`          | 新增 `format_bytes()`，改造 `ui_list_value`/`ui_hash_value`/`ui_set_value`/`ui_zset_value` | ~30  |
| `src-tauri/src/client/client_trait.rs` | `field_scan_0_get`/`field_scan_2_value` 使用格式参数                                       | ~10  |
| `src-tauri/src/client/impl_single.rs`  | 传递 `display_format` 参数                                                                 | ~3   |
| `src-tauri/src/client/impl_cluster.rs` | 传递 `display_format` 参数                                                                 | ~3   |

### 前端（Vue）~20 行

| 文件                                 | 改动                           | 行数 |
| ------------------------------------ | ------------------------------ | ---- |
| `src/views/tab/RedisValue.vue`       | 增加格式选择器，切换时重新请求 | ~15  |
| `src/locales/lang/zh-cn.ts`、`en.ts` | 国际化文案                     |

### 总计

- **文件数**：8 个
- **代码量**：~75 行
- **新增命令**：无
- **破坏性变更**：无（`display_format` 为 Optional，默认为 String）

---

## 后续扩展

1. **自动检测二进制**：参考 TinyRDM 的 `autoViewAs`，打开键时自动检测是否包含非 UTF-8 字节，建议切换到 Hex
2. **更多格式**：Octal（八进制）、Decimal（十进制）、Mixed（Hex + ASCII 混合）
3. **编码/解码**：参考 TinyRDM 的 decodeTypes，支持 Base64/GZip/MsgPack 等解码后再格式化
4. **编辑支持**：Hex 编辑模式下直接修改，保存时解析为字节数组写回 Redis
