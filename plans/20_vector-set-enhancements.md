# 20. Vector Set 功能增强分析

> **类型**：设计决策 + 实施计划  
> **关联**：[19_vector-set-support.md](./19_vector-set-support.md)、[20260809_rdm-tools-recent-changelogs.md](./20260809_rdm-tools-recent-changelogs.md)  
> **对标参考**：RedisInsight 3.6.0（2026-06）Vector Set 端到端支持  
> **日期**：2026-08-12

---

## 一、当前实现状态回顾

19.1～19.3 三阶段均已落地 ✅：

| 能力                                                          | 状态 |
| ------------------------------------------------------------- | ---- |
| 类型识别 + Tag（V / VECTORSET）                               | ✅   |
| VRANGE 分页浏览（exclusive 游标）                             | ✅   |
| VADD / VREM 增删                                              | ✅   |
| VISMEMBER 精确搜索                                            | ✅   |
| VCARD + VDIM + TTL 展示                                       | ✅   |
| FieldAdd 单元素添加（向量 + attrs）                           | ✅   |
| FieldSet 编辑面板（向量 + attrs）                             | ✅   |
| VGETATTR / VSETATTR                                           | ✅   |
| VINFO 元数据弹窗                                              | ✅   |
| VSIM 相似度搜索（ELE/VALUES + FILTER/EPSILON/EF/WITHATTRIBS） | ✅   |
| 键级复制为命令（VRANGE 批量，上限 1000）                      | ✅   |
| 行级复制为命令（VEMB+VGETATTR→VADD）                          | ✅   |
| commandFlags 全 V* 命令注册                                   | ✅   |
| 计划延后：VLINKS / VEMB RAW / FP32 / REDUCE / 量化细调 UI     | ⏸️   |

---

## 二、设计决策

以下决策基于对 RedisME 当前架构（Rust + Vue/Tauri）的评估，并参考了 RedisInsight 的设计思路。

### 2.1 页面加载时 pipeline 获取向量 + 属性

**当前问题**：`index.vue:1008-1027` 每次打开编辑面板都调用 `fieldGet`，背后是 `client_trait.rs:1376-1397` 的 VISMEMBER + VEMB + VGETATTR，**3 次独立 RTT**。

**决策**：页面加载时 pipeline VEMB + VGETATTR 一次拿齐当前页所有元素的向量和属性。

**性能评估**（100 个元素，300 维向量）：

| 操作                    | 命令数         | 数据量 | RTT       |
| ----------------------- | -------------- | ------ | --------- |
| VRANGE                  | 1              | 小     | 1         |
| Pipeline VEMB × 100     | 100            | ~240KB | 0（合并） |
| Pipeline VGETATTR × 100 | 100            | ~5KB   | 0（合并） |
| **合计**                | **201 条命令** | ~245KB | **1 RTT** |

本地 Redis：10~~30ms，远程 Redis 50ms + 传输：50~~100ms。**速度完全可接受。**

**编辑时零额外查询**：FieldSet.vue 打开时直接使用页面上已缓存的数据，无需 `fieldGet`。

### 2.2 向量不截断，直接显示全量

**决策**：表格中向量以 JSON 数组格式直接显示，不做截断。

**理由**：

- 高维向量虽然在表格中会因列宽溢出而显示省略号（CSS 全局处理，无 tooltip 气泡），但用户可以通过以下方式查看全量：
  - 复制单元格内容（右键复制或点击复制按钮，得到完整向量）
  - 点击编辑 → CodeMirror 编辑器支持全屏
- 不截断的优势：复制时得到完整向量，不存在"复制的数据不完整"的陷阱
- 与 RedisInsight 不同（RedisInsight 在详情抽屉中截断，是因为他们用 textarea 展示），我们在表格中用 JSON 文本，溢出由浏览器处理

### 2.3 主表属性列 + 向量预览列

**决策**：主表格增加两列：

| 新列               | 内容                          | 显示策略               |
| ------------------ | ----------------------------- | ---------------------- |
| **属性（attrs）**  | 属性 JSON 字符串              | 默认显示，CSS 溢出省略 |
| **向量（vector）** | `[v1, v2, v3, ...]` JSON 数组 | 默认显示，CSS 溢出省略 |

**与其他类型一致**：Hash 显示 field + value，ZSet 显示 member + score，VectorSet 显示 element + vector + attrs。

### 2.4 VRANGE 不支持时降级到 VRANDMEMBER

**决策**：`field_scan_vectorset_page` 中实现 `try_vrange` → `try_vrandmember` 双降级。

**降级行为**：

```
VRANGE 成功 → 正常分页，游标继续
VRANGE 失败 → VRANDMEMBER key count
  → cc.finished = true（无分页）
  → 前端显示"随机采样 N 条"标签
  → 工具栏增加"刷新采样"按钮
VRANDMEMBER 也失败 → 报错提示
```

**VRANDMEMBER 分页方案**：它没有游标，每次调用返回随机元素。所以不存在"下一页"的概念——直接标记完成，用户通过"刷新采样"获取新一批随机元素。这与 RedisInsight 的 `isPaginationSupported: false` 处理一致。

### 2.5 VSIM 保持弹窗，增强功能

**决策**：VSIM 保持 `me-dialog` 弹窗形式，不改为内联替换。

**理由**：

- 已实现且工作良好
- 重构内联替换成本高，收益有限
- 核心改进集中在**弹窗内部的增强**：
  - 行操作（View / Find similar / Delete）
  - 命令预览（可折叠）
  - 动态属性列（解析 attrs JSON 自动生成列）
  - 结果排序 + 高亮（≥0.85）

### 2.6 主表行操作增加"以此查询"按钮

**决策**：主表操作列新增 🔍 按钮，点击后打开 VSIM 弹窗并预填当前元素名。

**实现**：`index.vue` 中加一行按钮 + 一行函数：

```typescript
function findSimilar(row: ValueTableRow) {
  vSimRef.value?.open(currentViewFmt, { elementDisplay: String(row.value ?? '') })
}
```

`TableVSim.vue` 的 `open()` 已支持 `seed?.elementDisplay`（第 55 行），打开后自动触发查询（第 74-76 行）。

---

## 三、功能差距矩阵

| 功能域     | 功能点                    | RedisInsight        | RedisME 当前     | 差距 |
| ---------- | ------------------------- | ------------------- | ---------------- | ---- |
| **创建**   | 手动创建 VectorSet        | ✅                  | ✅               | —    |
|            | 一键样例集（vec2word）    | ✅                  | ❌               | P2   |
|            | 多行批量添加              | ✅                  | ❌               | P1   |
|            | FP32 转义输入             | ✅                  | ❌               | P1   |
| **浏览**   | 元素列表                  | ✅                  | ✅               | —    |
|            | 属性列（attrs）           | ❌ 元素列表无       | ❌               | P1   |
|            | 向量预览列                | ❌ 元素列表无       | ❌               | P1   |
|            | 向量+属性 pipeline 预载   | ❌ 按需加载         | ❌               | P1   |
|            | 详情抽屉                  | ✅                  | ⚠️ FieldSet 面板 | P2   |
|            | 向量复制/下载             | ✅                  | ❌               | P2   |
|            | VRANGE/VRANDMEMBER 双模式 | ✅                  | ❌ 仅 VRANGE     | P0   |
| **编辑**   | 内联 VADD 面板            | ✅                  | ⚠️ 弹窗          | —    |
|            | 量化选项                  | ❌ 无               | ❌               | P2   |
| **搜索**   | VSIM ELE/VALUES           | ✅                  | ✅               | —    |
|            | VSIM 结果行操作           | ✅ View/Find/Delete | ❌               | P0   |
|            | VSIM 结果排序 + 高亮      | ✅                  | ⚠️ 仅排序        | P1   |
|            | 动态属性列                | ✅                  | ❌               | P1   |
|            | 命令预览                  | ✅                  | ❌               | P1   |
|            | FILTER 自动补全           | ✅                  | ❌               | P2   |
|            | **主表"以此查询"**        | ✅                  | ❌               | P0   |
| **元数据** | VINFO                     | ✅                  | ✅               | —    |
|            | VLINKS                    | ❌ 未确认           | ❌               | P2   |
| **其他**   | VRANDMEMBER 降级          | ✅                  | ❌               | P0   |
|            | VEMB RAW                  | ❌ 未确认           | ❌ 延后          | —    |
|            | REDUCE 降维               | ❌ 未确认           | ❌ 延后          | —    |

---

## 四、实施计划（分阶段）

### 阶段一：底层优化 + 核心体验（P0）

**目标**：消除 RTT 浪费、增加降级容错、补齐核心操作。

---

#### 步骤 1：后端 `field_scan_vectorset_page` 改造

**文件**：`src-tauri/src/client/client_trait.rs`

**改动内容**：

**1a. 新增 `try_vrange` / `try_vrandmember` 降级函数**

```rust
/// 尝试 VRANGE，失败返回 None（优雅降级）
fn try_vrange(
    conn: &mut impl Commands,
    key: &RedisKey,
    start: &[u8],
    count: u64,
) -> AnyResult<Option<Vec<Vec<u8>>>> {
    match redis::cmd("VRANGE")
        .arg(key).arg(start).arg("+").arg(count)
        .query(conn)
    {
        Ok(names) => Ok(Some(names)),
        Err(e) if is_unknown_command(&e) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// 尝试 VRANDMEMBER，失败返回 None
fn try_vrandmember(
    conn: &mut impl Commands,
    key: &RedisKey,
    count: u64,
) -> AnyResult<Option<Vec<Vec<u8>>>> {
    match redis::cmd("VRANDMEMBER")
        .arg(key).arg(count)
        .query(conn)
    {
        Ok(names) => Ok(Some(names)),
        Err(e) if is_unknown_command(&e) => Ok(None),
        Err(e) => Err(e.into()),
    }
}
```

**1b. 新增 `RedisVectorSetItem` 结构体**（`src-tauri/src/utils/model.rs`）

```rust
api_model!(RedisVectorSetItem {
    /// 元素名（val_fmt 编码）
    name: String,
    /// 向量 JSON 数组字符串 "[1.0, 2.0, ...]"
    vector: String,
    /// 属性 JSON 对象字符串 "{\"pos\":\"noun\"}"
    attrs: String,
});
```

**1c. 改造 `field_scan_vectorset_page` 返回值**

从 `Vec<String>` 改为 `Vec<RedisVectorSetItem>`，包含 pipeline 获取的向量和属性：

```rust
fn field_scan_vectorset_page(
    conn: &mut MutexGuard<impl Commands>,
    key: &RedisKey,
    param: &FieldScanParam,
    bytes_format: &BytesFormat,
    cc: &mut ScanCursor,
) -> AnyResult<Vec<RedisVectorSetItem>> {
    let count = field_scan_batch_count(param.count);
    let start: Vec<u8> = /* ... existing cursor logic ... */;

    // 1. 优先 VRANGE，回退 VRANDMEMBER
    let names = match try_vrange(conn, key, &start, count)? {
        Some(names) => {
            if (names.len() as u64) < count {
                cc.finished = true;
            } else {
                cc.finished = false;
                if let Some(last) = names.last() {
                    cc.stream_cursor = format_bytes(last, bytes_format);
                }
            }
            names
        }
        None => {
            // VRANGE 不支持 → VRANDMEMBER
            match try_vrandmember(conn, key, count)? {
                Some(names) => {
                    cc.finished = true; // 无分页
                    cc.stream_cursor = String::new(); // 标记降级模式
                    names
                }
                None => bail!("VRANGE and VRANDMEMBER both unsupported"),
            }
        }
    };

    // 2. Pipeline VEMB + VGETATTR 批量获取
    let mut elements = Vec::with_capacity(names.len());
    for name_bytes in &names {
        let name = format_bytes(name_bytes, bytes_format);
        let vector = vemb_json_or_dash(conn, key, name_bytes);
        let attrs = vgetattr_opt(conn, key, name_bytes).unwrap_or_default();
        elements.push(RedisVectorSetItem { name, vector, attrs });
    }

    Ok(elements)
}
```

**1d. 调整 `field_scan_0_get` 的 VectorSet 分支**

```rust
// 原来：value 是 Vec<String>（仅元素名）
ValueType::VectorSet => {
    let items = field_scan_vectorset_page(&mut conn, key, param, bytes_format, &mut cc)?;
    // 转为 JSON：每个元素 { name, vector, attrs }
    let json_items: Vec<serde_json::Value> = items
        .into_iter()
        .map(|el| serde_json::json!({ "name": el.name, "vector": el.vector, "attrs": el.attrs }))
        .collect();
    value = Some(serde_json::Value::Array(json_items));
    length = vcard0(&mut conn, key)? as usize;
}
```

**注意**：VEMB/VGETATTR pipeline 中单个元素失败不拖死全页。失败时 vector 用 `"-"` 占位，attrs 用空串。

---

#### 步骤 2：前端接收新数据格式

**文件**：`src/views/tab/RedisValue/index.vue`

**改动内容**：

**2a. `dataList` 中 `vectorsetType` 分支**

```typescript
// 原来（第 554 行）：set / vectorset 为裸字符串
if (setType.value || vectorsetType.value) data.push({ value })

// 改为：
if (setType.value) {
  data.push({ value })
} else if (vectorsetType.value) {
  // 新格式：{ name, vector, attrs } 对象
  const el = value as { name: string; vector: string; attrs: string }
  data.push({
    value: el.name, // 元素名（兼容现有 column）
    vector: el.vector, // 新增：向量 JSON
    attrs: el.attrs, // 新增：属性 JSON
  })
}
```

**2b. `openFieldPanel` 中 `vectorsetType` 分支**

```typescript
// 原来（第 1008-1027 行）：调用 fieldGet 获取向量和属性
// 改为：直接从 row 中取，零 RTT
if (vectorsetType.value) {
  vectorValue = String(row.vector ?? '')
  vectorAttrs = String(row.attrs ?? '')
}
```

**2c. `field_get0` 保留降级**（Rust 后端）

`field_get0` 的 VectorSet 分支（VISMEMBER + VEMB + VGETATTR）保留：

- 正常情况下前端使用缓存数据，不走 `field_get`
- 单行刷新时（`refreshRow` 命令）走 `field_get0` 刷新

---

#### 步骤 3：主表新增属性列 + 向量预览列

**文件**：`src/views/tab/RedisValue/index.vue`（模板区域）

**改动**：在现有元素名列之后、操作列之前插入两列：

```vue
<!-- 属性列（attrs） -->
<el-table-column v-if="vectorsetType" :label="t('redisValue.attrs')" prop="attrs" min-width="120" />

<!-- 向量预览列（vector） -->
<el-table-column
  v-if="vectorsetType"
  :label="t('redisValue.vector')"
  prop="vector"
  min-width="200" />
```

**不使用 `show-overflow-tooltip`**：CSS 已全局处理 `.cell` 的 `overflow: hidden; text-overflow: ellipsis`（`index.vue:2411-2417`），悬停时不会弹出气泡，避免干扰。

**向量不截断**：完整 JSON 数组存储在 `row.vector` 中，内容溢出时由 CSS 自动省略。用户可通过复制单元格或打开编辑面板（CodeMirror 全屏）查看全量。

---

#### 步骤 4：主表操作列重构 + 增加"以此查询"按钮

**文件**：`src/views/tab/RedisValue/index.vue`

**操作列布局调整**（保证最多 3 个按钮）：

```
原有：编辑 | 删除 | 更多
新布局（vectorsetType）：编辑 | 以此查询 | 更多
```

**4a. 操作列模板调整**

```vue
<!-- 编辑/查看 -->
<me-icon
  v-if="canEdit && !streamType"
  :info="t('edit')"
  icon="el-icon-edit"
  class="icon-btn"
  @click.stop="openFieldPanel(scope.row, scope.$index, false)" />
<me-icon
  v-else
  :info="t('view')"
  icon="el-icon-view"
  class="icon-btn"
  @click.stop="openFieldPanel(scope.row, scope.$index, true)" />

<!-- 以此查询（VectorSet 专属） -->
<me-icon
  v-if="vectorsetType"
  :info="t('redisValue.vSimFindSimilar')"
  icon="el-icon-search"
  class="icon-btn"
  @click.stop="findSimilar(scope.row)" />

<!-- 更多 → 下拉框（删除移入其中） -->
<el-dropdown
  trigger="click"
  placement="bottom-end"
  @command="(cmd: string) => onFieldRowMoreCommand(cmd, scope.row)">
  <me-icon icon="el-icon-more-filled" class="icon-btn" />
  <template #dropdown>
    <el-dropdown-menu>
      <!-- 删除：VectorSet 在 dropdown 中，其他类型保持原有布局 -->
      <el-dropdown-item v-if="vectorsetType" command="deleteElement">
        <me-icon icon="el-icon-delete" :name="t('delete')" />
      </el-dropdown-item>
      <!-- ... 原有其他 dropdown 项 ... -->
    </el-dropdown-menu>
  </template>
</el-dropdown>
```

> **注意**：非 VectorSet 类型保持原有布局不变（编辑 | 删除 | 更多）。

**4b. 新增函数**

```typescript
function findSimilar(row: ValueTableRow) {
  vSimRef.value?.open(displayBytesFormat.value, { elementDisplay: String(row.value ?? '') })
}
```

**4c. 处理 dropdown 删除命令**

```typescript
function onFieldRowMoreCommand(cmd: string, row: ValueTableRow) {
  if (vectorsetType.value && cmd === 'deleteElement') {
    fieldDel(row)
    return
  }
  // ... 原有其他命令处理
}
```

> 删除确认弹窗从 `el-popconfirm` 改为 `el-dropdown-item` 后，确认逻辑不变（`fieldDel` 内部已有确认提示）。

---

#### 步骤 5：VRANDMEMBER 降级前端适配

**文件**：`src/views/tab/RedisValue/index.vue`

**改动**：

**5a. 检测降级模式**

当 `cursor.stream_cursor` 为空但 `cursor.finished === true` 且数据不为空时，表明正在使用 VRANDMEMBER 降级模式。

**5b. 显示"随机采样"提示**

在表格上方或工具栏中显示标签：

```vue
<el-tag v-if="vrandmemberMode" type="info" size="small">
  {{ t('redisValue.vrandmemberHint', { count: dataList.length }) }}
</el-tag>
```

**5c. 工具栏增加"刷新采样"按钮**

```vue
<me-icon
  v-if="vrandmemberMode"
  :info="t('redisValue.refreshSample')"
  icon="el-icon-refresh-right"
  class="icon-btn"
  @click="refreshKey(true)" />
```

**5d. 隐藏分页控件**

当 `vrandmemberMode` 时，`showMore` 强制为 `false`（因为 `cc.finished = true` 已经处理了）。

---

### 阶段二：VSIM 体验增强（P1）

**目标**：补齐 VSIM 搜索结果的操作闭环。

---

#### 步骤 6：VSIM 结果行操作

**文件**：`src/views/tab/RedisValue/TableVSim.vue`

**改动**：

**6a. 结果表增加操作列**

```vue
<el-table-column :label="t('action')" width="120" fixed="right" align="center">
  <template #default="scope">
    <div class="field-row-actions me-flex" style="justify-content: center; gap: 8px">
      <!-- 查看详情 -->
      <me-icon
        :info="t('view')"
        icon="el-icon-view"
        class="icon-btn"
        @click.stop="viewElement(scope.row)" />
      <!-- 以此为种子搜索 -->
      <me-icon
        :info="t('redisValue.vSimFindSimilar')"
        icon="el-icon-search"
        class="icon-btn"
        @click.stop="findSimilarFromResult(scope.row)" />
      <!-- 删除 -->
      <el-popconfirm
        :title="t('redisValue.deleteConfirm')"
        @confirm.stop="deleteElement(scope.row)">
        <template #reference>
          <me-icon :info="t('delete')" icon="el-icon-delete" class="icon-btn" />
        </template>
      </el-popconfirm>
    </div>
  </template>
</el-table-column>
```

**6b. 实现操作函数**

- `viewElement(row)`：通过 emit 或回调通知父组件打开 FieldSet 面板（只读模式）
- `findSimilarFromResult(row)`：预填 `elementText`，重新查询
- `deleteElement(row)`：调用 VREM，刷新列表，最后元素时自动删除 key

**6c. 操作回调**：TableVSim 通过 `defineEmits` 暴露 `viewElement` / `deleteElement` 事件，父组件处理。

---

#### 步骤 7：VSIM 命令预览

**文件**：`src/views/tab/RedisValue/TableVSim.vue`

**改动**：

**7a. 新增预览状态**

```typescript
const showPreview = ref(false)
const previewText = ref('')
```

**7b. 构建预览命令字符串**

```typescript
function buildPreviewCommand(): string {
  const parts = ['VSIM', keyStr]
  if (mode.value === 'ele') {
    parts.push('ELE', quoteArg(elementText.value))
  } else {
    parts.push('VALUES', ...vectorText.value.trim().split(/\s+/))
  }
  parts.push('COUNT', String(count.value))
  if (withAttribs.value) parts.push('WITHSCORES', 'WITHATTRIBS')
  if (filterText.value.trim()) parts.push('FILTER', filterText.value.trim())
  if (epsilonText.value.trim()) parts.push('EPSILON', epsilonText.value.trim())
  if (efText.value.trim()) parts.push('EF', efText.value.trim())
  return parts.join(' ')
}
```

**7c. 模板**

```vue
<el-button text @click="showPreview = !showPreview">
  {{ showPreview ? t('redisValue.hidePreview') : t('redisValue.showPreview') }}
</el-button>
<el-input
  v-if="showPreview"
  :model-value="previewText"
  readonly
  type="textarea"
  :rows="2"
  class="vsim-preview" />
```

---

#### 步骤 8：VSIM 结果动态属性列

**文件**：`src/views/tab/RedisValue/TableVSim.vue`

**改动**：

**8a. 解析属性 key**

```typescript
const attrKeys = computed(() => {
  const keys = new Set<string>()
  for (const item of itemList.value) {
    try {
      const attrs = JSON.parse(item.attrs || '{}')
      for (const key of Object.keys(attrs)) {
        keys.add(key)
      }
    } catch {
      /* ignore */
    }
  }
  return [...keys].sort()
})
```

**8b. 动态列渲染**

```vue
<el-table-column v-for="attrKey in attrKeys" :key="attrKey" :label="attrKey" min-width="140">
  <template #default="scope">
    {{ getAttrValue(scope.row.attrs, attrKey) }}
  </template>
</el-table-column>
```

**8c. 列可见性切换**

```typescript
const visibleAttrKeys = ref(new Set<string>())
// 默认全部隐藏，用户通过 popover 切换
```

**8d. 高相似度标记**

score ≥ 0.85 的行使用特殊样式（绿色文字或加粗），参考 RedisInsight 的 `HIGH_SIMILARITY_THRESHOLD = 0.85`。

---

### 阶段三：体验打磨（P2，按需）

- **步骤 9**：VSIM FILTER 属性感知自动补全（借鉴 `FilterInputWithSuggestions.tsx`）
- **步骤 10**：样例数据集 vec2word（借鉴 `AddKeyVectorSet` + `LoadSampleDataset`）
- **步骤 11**：批量添加元素（FieldAdd 多行模式）
- **步骤 12**：FP32 转义格式输入（前端解析 + 后端 `VADD FP32` / `VSIM FP32`）
- **步骤 13**：版本感知 `WITHATTRIBS`（运行时检测 + 回退 VGETATTR pipeline）
- **步骤 14**：量化选项 UI / VLINKS / VEMB RAW / REDUCE 降维

---

## 五、技术注意事项

### 5.1 Pipeline 失败处理

VEMB 或 VGETATTR 对单个元素失败时不应拖死全页：

```rust
// 每个元素独立处理，失败时用 "-" 或空串占位
for name_bytes in &names {
    let name = format_bytes(name_bytes, bytes_format);
    let vector = vemb_json_or_dash(conn, key, name_bytes);
    let attrs = vgetattr_opt(conn, key, name_bytes).unwrap_or_default();
    elements.push(RedisVectorSetItem { name, vector, attrs });
}
```

### 5.2 `field_get0` 保留降级

`field_get0` 的 VectorSet 分支（VISMEMBER + VEMB + VGETATTR）保留作为降级路径：

- 正常情况下前端使用缓存数据，不走 `field_get`
- 如果 `field_scan_vectorset_page` 返回的数据因某些原因丢失或不可用，`field_get0` 作为兜底
- 或者当用户刷新单行时（`refreshRow` 命令），走 `field_get0` 刷新

### 5.3 属性 key 跨分页累积

前端需要一个数据结构来累积所有已加载元素的属性 key：

```typescript
// 在 index.vue 中
const vectorSetAttrKeys = ref(new Set<string>())

// 每次加载新页时更新
function updateVectorSetAttrKeys(elements: RedisVectorSetItem[]) {
  for (const el of elements) {
    try {
      const attrs = JSON.parse(el.attrs || '{}')
      for (const key of Object.keys(attrs)) {
        vectorSetAttrKeys.value.add(key)
      }
    } catch {
      /* 忽略解析失败 */
    }
  }
}
```

### 5.4 向量预览的显示格式

向量在表格中以 JSON 数组格式显示，不截断。列宽通过 `min-width` 控制，内容溢出时由 CSS 自动省略（`overflow: hidden; text-overflow: ellipsis`，无 tooltip）：

```
┌───────────────────────────────────────┐
│ [0.048865, 0.053362, -0.038857, 0.09…│  ← 列宽溢出，CSS 省略号
└───────────────────────────────────────┘
```

用户可以通过以下方式查看全量：

- **复制单元格**：右键复制或点击复制按钮，得到完整向量
- **打开编辑面板**：CodeMirror 支持全屏查看

### 5.5 数据格式兼容性

`field_scan_vectorset_page` 的返回值从 `Vec<String>` 改为 `Vec<RedisVectorSetItem>` 后，需要确保：

- 前端 `dataList` 的 `vectorsetType` 分支能正确解析新格式
- `fieldValueRows` 能正确提取数组
- 向后兼容：如果旧数据格式（纯字符串数组）仍然出现，需要兼容处理

### 5.6 VRANDMEMBER 降级标记

通过 `cc.stream_cursor` 为空但 `cc.finished === true` 来判断降级模式。更稳健的做法是新增一个 `cc.is_vrandmember_fallback: bool` 字段，但为了最小化改动，可以复用现有字段。

---

## 六、参考资料

### 6.1 RedisInsight 源码架构参考

> 以下为 RedisInsight 3.6.0 Vector Set 实现的源码级分析，供设计参考。

#### 后端架构（NestJS）

**文件结构**（25 个文件）：

```
api/src/modules/browser/vector-set/
├── vector-set.controller.ts   — REST 端点（9 个）
├── vector-set.service.ts      — 核心业务逻辑（613 行）
├── vector-set.utils.ts        — VADD/VSIM 命令构建 + 回复解析（362 行）
├── vector-set.module.ts       — NestJS 模块定义
├── constants.ts               — VSIM 步长常量 + 协议 token
└── dto/                       — 12 个 DTO 文件
```

**核心架构模式**：

1. **`writeVsimTokens` 共享策略模式**（`vector-set.utils.ts:134-189`）
   - 可执行命令构建和预览格式化器共享同一套 token 布局
   - 通过 `VsimTokenWriter<T>` 泛型策略接口渲染单个 token
   - 确保预览和实际执行命令**绝对一致**

2. **`VsimWithAttribsOption` 版本感知模式**（`vector-set.utils.ts:25`）
   - Redis 8.0.0–8.0.2 的 VSIM 不支持 WITHATTRIBS
   - 运行时检测 `RedisFeature.VsimWithAttribs`，回退到 VGETATTR pipeline

3. **`trySendCommand` 优雅降级模式**（`vector-set.service.ts:506-518`）
   - VRANGE 不可用时返回 `null` → 自动回退 VRANDMEMBER
   - 通过 `isUnsupportedCommandError` 检测 `ERR unknown command`

4. **Pipeline 批量操作**：VGETATTR、VEMB、VADD、VREM 均 pipeline

5. **VSIM 回复解析**：基于 stride 解析（WITHATTRIBS=3，否则=2），防御性处理

6. **向量体截断**：`VECTOR_EMBEDDING_MAX_DISPLAY_LENGTH`，超出标记 `vectorTruncated: true`

#### 前端架构（React + Redux Toolkit）

**组件树**（~100 个文件）：

```
key-details/components/vector-set-details/
├── VectorSetDetails.tsx                    — 主编排组件
├── similarity-search-form/
│   ├── SimilaritySearchForm.tsx            — 相似度搜索表单
│   └── filter-input-with-suggestions/
│       └── FilterInputWithSuggestions.tsx  — FILTER 自动补全
├── similarity-search-results/
│   ├── SimilaritySearchResultsTable.tsx    — 结果表
│   ├── SimilaritySearchResultsTable.config.tsx — 列定义
│   └── components/SimilarityColumnsPopover/ — 列切换弹窗
├── vector-set-element-list/
│   ├── VectorSetElementList.tsx            — 元素列表
│   └── components/RowActionsCell/          — 行操作（View/Find/Delete）
├── element-details/
│   └── ElementDetails.tsx                  — 详情抽屉
└── hooks/                                 — 6 个自定义 hooks
```

**关键 UI 模式**：

1. **VSIM 结果表动态属性列**：`buildAttributeColumn(key)` + WeakMap 缓存 + `SimilarityColumnsPopover` 列切换，≥0.85 高亮
2. **FILTER 自动补全**：`.attribute` token 检测，排除已有属性，键盘导航
3. **详情抽屉**：向量 readonly textarea，截断下载/完整复制，属性 JSON 编辑器
4. **行操作**：View / Find similar / Delete，共享 `useVectorSetActionsConfig` hook

#### 状态管理（Redux Toolkit）

**Slice**（`vectorSet.ts`，755 行）：15 个 async thunks，`mergeAttributeKeys` 跨分页累积，`AbortController` 预览取消，`prefillElement` nonce+forKey 防护。

#### 样例数据集

**vec2word**：30 个常见英文单词，每个 300 维向量，`SETATTR '{"pos":"noun"}'`。

### 6.2 相关文档与源码
