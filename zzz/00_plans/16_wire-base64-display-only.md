# 传输层固定 Base64 + 数据编码仅控展示

> **实现状态**：✅ 已实现  
> **关联 backlog**：`docs/zh/changelog/future.md`  
> **关联已有方案**：
>
> - [02_value-display-format.md](./02_value-display-format.md)（多格式展示）
> - [05_custom-formatter.md](./05_custom-formatter.md)（自定义编解码）
> - [13_java-serialization.md](./13_java-serialization.md)（JavaSerial 只读）  
>   **关键代码**：`src/utils/format.ts`、`src/views/tab/RedisValue.vue`、`src/views/ext/FieldSet.vue`、`src/views/ext/FieldAdd.vue`、`TableHashKeys` / `TableZsetRange` 等

## 背景与动机

### 现状问题

1. **「数据编码」与 Redis 拉取绑定**：切换 UTF8 / Hex / Base64 会触发 `fieldScan` / GET，按不同 `valFmt`/`bytesFormat` 格式化返回。
2. **UTF8 路径丢字节**：后端 `format_bytes(..., UTF8)` 使用 `from_utf8_lossy`，二进制（如 Java 序列化）进入表格后不可恢复。
3. **Hash/List 字段看 JavaSerial 麻烦**：键级常为 UTF8 浏览 → 字段弹窗选项被裁成 UTF8/StrJson → 用户得先改键级 Base64 再开字段再选 JavaSerial。
4. **STRING 同样存在切 UTF8 重拉并 lossy 的风险**；Auto 虽已用 base64，模型不统一。

### 目标

| 目标     | 说明                                                            |
| -------- | --------------------------------------------------------------- |
| 不丢字节 | IPC 传输永远是完整原始 bytes 的 Base64                          |
| 操作简单 | 切编码不打 Redis；字段弹窗可 Auto 出 JavaSerial                 |
| 模型统一 | STRING 与 Hash/List/Set/ZSet 同一套：wire = base64，view = 前端 |
| 体积可控 | 相对「utf8 + 另附 valueBytes」更省；相对纯 utf8 约 +33%         |

Stream / JSON：**数据编码下拉禁用**，本方案不改其传输与展示。

---

## 决策摘要

| 项                       | 结论                                                                                                                                                  |
| ------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| IPC wire                 | **永远 `base64`**（STRING / Hash / List / Set / ZSet 的值与字段名）                                                                                   |
| 「数据编码」下拉         | **仅控制展示**；切换 **不** 调用 Redis                                                                                                                |
| 何时打 Redis             | 换键、手动刷新、加载更多、加载完整大值、保存/删除/新增字段等写操作                                                                                    |
| 字段弹窗                 | 直接用行内 base64 wire；打开时可 `detectViewFormat`（Auto）                                                                                           |
| JavaSerial / Pickle      | 继续 **只读**；STRING 整键 + Hash/List/Set/ZSet **字段弹窗**均可用                                                                                    |
| 键级扩展格式             | STRING：保留 Auto / MsgPack / StrJson / JavaSerial / Pickle / custom；非 STRING 键级建议仅 UTF8/Hex/Binary/Base64（整表统一展示），高级格式放字段弹窗 |
| valueBytes 双字段        | **不做**（wire 已是 base64，无需再附一份）                                                                                                            |
| field_get 兜底重拉       | **不做**（扫描/get 已保证 wire）                                                                                                                      |
| 后端 `BytesFormat::UTF8` | 本方案适用路径不再依赖；可保留枚举供其它场景或过渡                                                                                                    |

---

## 数据流

```
Redis bytes
    │  GET / HGET / LRANGE / SSCAN / ZSCAN …
    ▼
IPC：恒 base64  ──►  srcWire（权威，不被展示层覆盖）
    │
    ├─ 键级 view（UTF8 / Hex / … / Auto）──► 表格单元格 / STRING 编辑器
    │
    └─ 字段弹窗 view（可 Auto → JavaSerial 等）──► 字段编辑器
         │
         └─ 保存（非只读）── encode ──► base64 ──► SET / HSET / LSET …
```

**禁止**：把「解码后的展示文本」写回 `srcWire`（当前 Auto 认出 utf8/strjson 后改写 `displayWire` 的路径需拆开）。

---

## 分类型行为

### STRING

| 行为   | 规则                                                                |
| ------ | ------------------------------------------------------------------- |
| 拉取   | `fieldScan` / GET 固定 `bytesFormat: base64`（含大值预览 GETRANGE） |
| 切编码 | 本地 `detect` / `meFormatViewValue(Async)`，不 `refreshKey`         |
| Auto   | 对 `srcWire` 跑 `detectViewFormat`；仅影响展示                      |
| 保存   | `meViewToWire` / custom encode → 提交 `inputFormat: base64`         |
| 只读   | `javaserial` / `pickle` → `canSave = false`                         |
| 大值   | 「加载完整值」仍打 Redis；与编码切换无关                            |

### Hash / List / Set / ZSet

| 行为                         | 规则                                                                                                                               |
| ---------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| fieldScan / fieldGet         | `valFmt` / `bytesFormat` 固定 `base64`（field key 与 value 同契约）                                                                |
| 表格展示                     | `srcWire → 键级 view → 展示文本`（过滤、排序、复制展示值均基于展示层）                                                             |
| 切键级编码                   | 不重扫；只重算各行展示                                                                                                             |
| 打开字段弹窗                 | `srcFieldWire = row.value`（已是 base64）；可选 Auto                                                                               |
| 字段下拉                     | 放开 Hex/Binary/Base64/MsgPack/JavaSerial/Pickle/custom 等（不再被「键级 utf8」裁掉）                                              |
| 保存字段                     | value 按字段 view encode 为 base64；**fieldKey / ZSet·Set 的 src 成员也按 base64** 提交（与后端 `parse_bytes` 共用 `valFmt` 一致） |
| 删除 / 复制为命令 / 刷新单行 | 一律带 base64 wire                                                                                                                 |

### Stream / JSON

- 下拉禁用，维持现有拉取与展示。
- 勿把 Stream 条目 fields map 强行纳入「一律 base64 单值」模型。

---

## 键级 vs 字段级编码

| 位置        | STRING                                                                        | Hash/List/Set/ZSet                                                                   |
| ----------- | ----------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| 键级下拉    | Auto、UTF8、Hex、Binary、Base64、StrJson、MsgPack、JavaSerial、Pickle、custom | 建议：UTF8、Hex、Binary、Base64（整表展示）                                          |
| 字段弹窗    | —                                                                             | UTF8、Hex、Binary、Base64、StrJson、MsgPack、JavaSerial、Pickle、custom；打开可 Auto |
| 混格式 Hash | —                                                                             | 键级用 UTF8 浏览；单字段弹窗 Auto/JavaSerial，避免整表 MsgPack 一片错误              |

---

## 实现要点（按模块）

### 1. `RedisValue.vue`

- 去掉编码下拉 `@change="refreshKey(false)"`，改为本地同步展示。
- `buildFieldScanParam`：非 Stream/JSON 固定 `bytesFormat: 'base64'`；删除「非 STRING 先探测再 utf8 重拉」。
- 拆分状态：`srcWire`（base64） vs `displayText` / 表格 display；Auto 不得覆盖 `srcWire`。
- `formatTableCell` / `fieldRowDisplayValue` / 本地过滤 / 排序：对 base64 wire 按键级 view 解码。
- 复制：明确复制「展示文本」还是「原始 wire」（建议默认展示；需要时可另入口）。
- 保存 STRING：恒 base64 写回。
- `fieldDel` / `get_field_as_command` / `field_get` 刷新：`valFmt: 'base64'`，参数用行内 wire。

### 2. `format.ts`

- 弱化或删除「仅 STRING 才有扩展格式」对 **字段弹窗** 的限制；`fieldViewOptions` 在 wire 恒 base64 下提供完整列表。
- `viewFmtForField` / `isStringOnlyView`：按「键级整表」与「字段弹窗」重新划分，避免再把字段降成 utf8 wire。
- `toWireFormat`：业务写路径对适用类型可直接恒 `base64`（或保留函数但调用方固定传 base64 view）。

### 3. `FieldSet.vue`

- 打开：用行内 base64；默认 view = Auto 识别结果或跟随键级展示格式（建议：**有魔数则 Auto 优先，否则跟键级**）。
- `isReadonlyView` → 禁保存 + i18n 提示（对齐 STRING）。
- `field_get` 刷新：请求/回写均为 base64，再 `syncFieldEditor`。
- 提交：`valFmt: 'base64'`；Hash `fieldKey`、ZSet/Set `srcFieldValue` 均为 base64 wire。

### 4. `FieldAdd.vue` / `TableHashKeys` / `TableZsetRange` / Pop 等

- 读写传 base64；UI 展示再按 view 解码。
- FieldAdd 的 key/value 编码下拉语义改为「编辑区展示格式」，提交前转到 base64。

### 5. 后端

- 前端固定传 `base64` 即可，**可不改** `BytesFormat` 枚举。
- 可选清理：适用路径忽略 UTF8、或文档标明 UTF8 仅遗留；非必须首版做。
- `get_field_as_command` 等继续 `parse_bytes(..., base64)`。

### 6. 文档

- 更新 `docs/zh/guide/usage/codec.md`（及 en）：传输 base64、编码只控展示、字段支持 JavaSerial/Pickle 只读。
- changelog：功能说明「切换编码不再请求 Redis；Hash 等字段可直接查看 JavaSerial」。

---

## 明确不做

- 扫描结果同时返回 `value` + `valueBytes`（双倍体积）。
- 打开字段时再 `field_get` 兜底（契约完整则不需要）。
- JavaSerial / Pickle 写回。
- Stream / JSON 纳入本模型。
- 键级对 Hash 整表启用 Auto/JavaSerial（混格式体验差；放字段弹窗）。

---

## 风险与对策

| 风险                             | 对策                                                  |
| -------------------------------- | ----------------------------------------------------- |
| 表格露出原始 base64 串           | 展示层强制 decode；禁止 `view===utf8` 时原样返回 wire |
| 切编码误触发刷新                 | 去掉下拉 `refreshKey`；单测/手测切换 Hex↔UTF8 无网络  |
| 保存时 fieldKey 仍是「展示文本」 | 行内存 wire；提交只用 wire；展示与 wire 分字段        |
| 体积 +33%                        | 可接受；极大 value 已有截断/预览机制                  |
| 每格反复 atob                    | 扫完预处理 display，或按需缓存                        |
| 空值                             | `""` wire；展示、Auto、保存空值回归                   |
| 精确扫描二进制 field 名          | 服务端 match 仍偏文本；本地过滤走展示层；文档说明     |
| custom 编解码                    | 已基于 base64 wire，与本方案契合                      |

---

## 建议实施顺序

1. **STRING**：固定 base64 拉取；拆 `srcWire`；切编码不刷新；保存恒 base64。
2. **Hash/List/Set/ZSet 扫描与表格**：固定 base64；表格 decode 展示；切键级编码不重扫。
3. **字段弹窗**：完整格式列表 + Auto + JavaSerial/Pickle 只读；保存/删除/命令复制契约对齐。
4. **旁路 UI**（FieldAdd、HashKeys、ZsetRange、Pop 等）统一 base64。
5. **文档与回归测试**。

---

## 验收清单

- [ ] STRING：切 Auto/UTF8/Hex/JavaSerial/custom **无** fieldScan/GET；二进制切 UTF8 再切回 Hex **字节一致**
- [ ] STRING：Auto 识别 JavaSerial/Pickle/MsgPack；只读不可保存
- [ ] STRING：保存后 Redis 字节与编辑意图一致（含 custom）
- [ ] Hash：键级保持 UTF8 展示时，含 Java 序列化的 field 打开弹窗可 Auto 为 JavaSerial
- [ ] Hash/List：切键级 Hex/Base64 **不**重扫；表格展示正确变化
- [ ] 字段保存（MsgPack/Hex/UTF8）、删除、复制为命令、单行刷新均正确
- [ ] 空值、大值预览/加载完整、Stream/JSON 行为无回归
- [ ] 文档已更新

---

## 参考：与备选方案对比

| 方案                                                | 优点                                     | 缺点                                   | 结论     |
| --------------------------------------------------- | ---------------------------------------- | -------------------------------------- | -------- |
| A. 字段弹窗切格式时 field_get(base64)               | 扫描体积不变                             | 每次打开/切换可能 RTT；键级仍易踩 utf8 | 否       |
| B. utf8 扫描 + 附带 valueBytes                      | 打开零 RTT                               | API 双轨、体积近翻倍                   | 否       |
| **C. 适用类型 IPC 恒 base64，编码只展示（本方案）** | 模型统一、不丢字节、切格式流畅、无双字段 | 相对 utf8 扫描约 +33%                  | **采用** |
