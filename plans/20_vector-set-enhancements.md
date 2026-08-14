# 20. Vector Set 功能增强分析

> **类型**：设计决策 + 实施计划  
> **关联**：[19_vector-set-support.md](./19_vector-set-support.md)、[20260809_rdm-tools-recent-changelogs.md](./20260809_rdm-tools-recent-changelogs.md)  
> **对标参考**：RedisInsight 3.6.0（2026-06）Vector Set 端到端支持  
> **日期**：2026-08-12（2026-08-14 对照实际代码更新）

---

## 一、当前实现状态

### 19.x 基础能力（已落地 ✅）

| 能力                                                          | 状态 |
| ------------------------------------------------------------- | ---- |
| 类型识别 + Tag（V / VECTORSET）                               | ✅   |
| VRANGE 分页浏览（exclusive 游标）                             | ✅   |
| VADD / VREM 增删                                              | ✅   |
| VISMEMBER 精确搜索                                            | ✅   |
| VCARD + VDIM + TTL 展示                                       | ✅   |
| FieldAdd 单元素添加（向量 + attrs）                           | ✅   |
| FieldSet 编辑面板（向量 + attrs，含维度预检）                 | ✅   |
| VGETATTR / VSETATTR                                           | ✅   |
| VINFO 元数据弹窗                                              | ✅   |
| VSIM 相似度搜索（ELE/VALUES + FILTER/EPSILON/EF/WITHATTRIBS） | ✅   |
| 键级复制为命令（VRANGE 批量，上限 1000）                      | ✅   |
| 行级复制为命令（VEMB+VGETATTR→VADD）                          | ✅   |
| commandFlags 全 V* 命令注册                                   | ✅   |
| 计划延后：VLINKS / VEMB RAW / FP32 / REDUCE / 量化细调 UI     | ⏸️   |

### 阶段一：底层优化 + 核心体验（已落地 ✅，部分实现方式与初版计划不同）

| 计划项                                      | 状态 | 实际实现说明                                                                                       |
| ------------------------------------------- | ---- | -------------------------------------------------------------------------------------------------- |
| 步骤 1：后端扫描改造（向量+属性随扫描返回） | ✅   | 见 2.1                                                                                             |
| 步骤 2：前端接收新数据格式                  | ✅   | `dataList` 解析 `{name, vector, attrs}`；打开面板零 RTT；`field_get0` 保留兜底（单行刷新走该路径） |
| 步骤 3：主表属性列 + 向量预览列             | ✅   | attrs / vector 两列，CSS 溢出省略，不截断数据                                                      |
| 步骤 4：操作列 + "以此查询"                 | ✅   | 布局：编辑/查看 \| VSIM 查询 \| 更多（删除移入更多下拉，`deleteElement`）                          |
| 步骤 5：VRANDMEMBER 适配                    | ✅   | 实际为**显式双模式**切换（见 2.4），非自动降级                                                     |

### 阶段二：VSIM 体验增强（范围收窄后已落地 ✅）

经讨论收窄范围（2026-08-14），只做弹窗内独有价值的部分：

| 计划项                        | 状态            | 实际实现说明                                                                                                   |
| ----------------------------- | --------------- | -------------------------------------------------------------------------------------------------------------- |
| 步骤 6：结果行操作            | ✅ 只做种子搜索 | 结果行“以此为种子查询”（预填 ELE 重查）；查看详情/删除与主表重复，不做                                         |
| 步骤 7：命令预览              | ✅ 退化版       | 不做可折叠预览区，改为表单区“复制为命令”按钮（参数顺序与 `v_sim0` 一致，redis-cli 引号风格）                   |
| 步骤 8：高亮                  | ✅ 只做高亮     | score ≥ 0.85 整行用连接颜色高亮（row-class-name + row-style）；动态属性列砍掉（attrs 原文列 + 本地过滤已够用） |
| 步骤 13：版本感知 WITHATTRIBS | ✅ 方案 b       | 报错重试：带 WITHATTRIBS 失败则去掉重试，再用 VGETATTR pipeline 补属性（`v_sim0`，前端无感）                   |

---

## 二、设计决策与实际实现

### 2.1 扫描时 pipeline 批量获取向量 + 属性 ✅

`field_scan_vectorset_page` 返回 `RedisVectorSetItem { name, vector, attrs }`：

- 元素名列表（VRANDMEMBER / VRANGE）拿到后，**双 pipeline 拆分**批量获取：
  - pipeline 1：VEMB × N → 向量 JSON
  - pipeline 2：VGETATTR × N → 属性 JSON
- 同一 key 同 slot，用 `req_packed_commands` 整批路由（单次 RTT），比 ClusterPipeline 更轻
- 单元素 VEMB/VGETATTR 失败不拖死整页：向量 `"-"` 占位、属性空串

### 2.2 向量不截断，直接显示全量 ✅

表格中以 JSON 数组全文显示，列宽溢出由 CSS 统一省略（无 tooltip）；完整数据通过复制单元格 / 编辑面板（CodeMirror 全屏）查看。

### 2.3 主表属性列 + 向量预览列 ✅

element + vector + attrs 三列，与 Hash（field+value）、ZSet（member+score）对齐。

### 2.4 浏览双模式（与初版计划不同，以此为准）✅

初版计划为"VRANGE 优先 → 失败自动降级 VRANDMEMBER"；实际实现为**显式双模式**：

- 工具栏 `el-segmented` 切换：**随机采样**（VRANDMEMBER，默认，全版本支持，无分页）/ **范围查询**（VRANGE，需 ≥ 8.4）
- 模式经 `FieldScanMeta.vectorset_sample` 传入后端；切换时重置游标重扫
- 错误直接透出原则：命令不支持 / 无权限等报错由前端提示，**不做隐式降级**

### 2.5 VSIM 弹窗增强 ✅

VSIM 维持 `me-dialog` 弹窗（TableVSim.vue）。当前已有：ELE/VALUES 模式、COUNT、WITHATTRIBS、FILTER、EPSILON、EF、本地过滤、结果排序（默认 score 降序），另新增：

- 结果行“以此为种子查询”（me-icon-rank，预填元素名重新查询）
- 表单区“复制为命令”图标按钮（el-icon-document + tooltip 顶部提示，位于查询按钮左侧；前端拼接 VSIM 命令文本）
- score ≥ 0.85 整行高亮（row-class-name + row-style，与命令日志同方案；颜色：`share.conn?.color || share.color || var(--el-color-primary)`）

砍掉不做：结果行查看详情 / 删除（与主表功能重复）；命令预览区（退化为复制按钮）；动态属性列（attrs 原文列已够用）。

### 2.6 主表“以此查询” ✅

操作列 🔍（me-icon-rank）按钮 → `showVSimWithElement(row)` → 打开 VSIM 弹窗预填元素名并自动查询。

### 2.7 WITHATTRIBS 兼容降级（方案 b：报错重试）✅

Redis 8.0.0–8.0.2 的 VSIM 不支持 WITHATTRIBS。处理策略：

- 首次带 WITHATTRIBS 发送，失败则去掉重试（不预判版本，对代理/云厂商环境最稳）
- 重试成功后用 VGETATTR pipeline（同 key 同 slot，1 RTT）补属性，结果语义无损，前端无感
- 重试仍失败则透出重试错误；属性获取失败保留空串不阻断

---

## 三、剩余待办（仅 P2，按需）

| 步骤 | 内容                                                           | 备注                                           |
| ---- | -------------------------------------------------------------- | ---------------------------------------------- |
| 9    | VSIM FILTER 属性感知自动补全                                   | 借鉴 RedisInsight `FilterInputWithSuggestions` |
| 10   | 样例数据集一键导入（vec2word：30 词 × 300 维 + attrs）         | 借鉴 `AddKeyVectorSet` + `LoadSampleDataset`   |
| 11   | FieldAdd 多行批量添加元素                                      | 当前仅单元素                                   |
| 12   | FP32 转义格式输入                                              | 前端解析 + 后端 `VADD FP32` / `VSIM FP32`      |
| 14   | 量化选项 UI（NOQUANT/Q8/BIN）/ VLINKS / VEMB RAW / REDUCE 降维 | 延后                                           |

### 差距矩阵补充项（P2）

- **向量复制/下载**：独立于"复制为命令"的向量导出能力（RedisInsight 有截断下载/完整复制）
- **详情抽屉**：维持 FieldSet 面板方案，不再对齐 RedisInsight 抽屉形态

---

## 四、技术注意事项

### 4.1 Pipeline 失败处理（已落地）

VEMB / VGETATTR 单元素失败用占位值，不拖死整页。

### 4.2 `field_get0` 兜底（已落地）

`field_get0` 的 VectorSet 分支（VISMEMBER + VEMB + VGETATTR）保留：正常浏览用扫描缓存（零 RTT），单行刷新（`refreshRow`）走该路径。

### 4.3 错误处理原则（已确立）

命令不支持 / 无权限等错误不做隐式降级，由前端直接提示；浏览模式由用户显式选择。例外：WITHATTRIBS 兼容降级（见 2.7），因其语义无损且属实现细节而非用户选择。

---

## 五、参考资料

### 5.1 RedisInsight 源码架构参考

> RedisInsight 3.6.0 Vector Set 实现的源码级分析，供设计参考。

#### 后端架构（NestJS）

文件结构（25 个文件）：`api/src/modules/browser/vector-set/`，含 controller（9 个 REST 端点）、service（613 行核心逻辑）、utils（VADD/VSIM 命令构建 + 回复解析）、12 个 DTO。

核心架构模式：

1. **`writeVsimTokens` 共享策略模式**：可执行命令构建和预览格式化器共享同一套 token 布局，确保预览和实际执行命令绝对一致
2. **`VsimWithAttribsOption` 版本感知模式**：Redis 8.0.0–8.0.2 的 VSIM 不支持 WITHATTRIBS，运行时检测并回退 VGETATTR pipeline
3. **`trySendCommand` 优雅降级模式**：VRANGE 不可用时返回 `null` → 自动回退 VRANDMEMBER
4. **Pipeline 批量操作**：VGETATTR、VEMB、VADD、VREM 均 pipeline
5. **VSIM 回复解析**：基于 stride 解析（WITHATTRIBS=3，否则=2）
6. **向量体截断**：`VECTOR_EMBEDDING_MAX_DISPLAY_LENGTH`，超出标记 `vectorTruncated: true`

#### 前端架构（React + Redux Toolkit）

组件树（~100 个文件）：`key-details/components/vector-set-details/`，含主编排组件、相似度搜索表单（FILTER 自动补全）、结果表（动态属性列 + 列切换弹窗）、元素列表（行操作 View/Find/Delete）、详情抽屉、6 个自定义 hooks。

关键 UI 模式：

1. **VSIM 结果表动态属性列**：`buildAttributeColumn(key)` + WeakMap 缓存 + 列切换 popover，≥0.85 高亮
2. **FILTER 自动补全**：`.attribute` token 检测，排除已有属性，键盘导航
3. **详情抽屉**：向量 readonly textarea，截断下载/完整复制，属性 JSON 编辑器
4. **行操作**：View / Find similar / Delete，共享 `useVectorSetActionsConfig` hook

状态管理（Redux Toolkit）：`vectorSet.ts`（755 行），15 个 async thunks，`mergeAttributeKeys` 跨分页累积，预览取消，prefill 防护。

样例数据集 **vec2word**：30 个常见英文单词，每个 300 维向量，`SETATTR '{"pos":"noun"}'`。

### 5.2 相关文档与源码

- [19_vector-set-support.md](./19_vector-set-support.md)
- [20260718_rdm-competitive-analysis.md](./20260718_rdm-competitive-analysis.md)
- [20260809_rdm-tools-recent-changelogs.md](./20260809_rdm-tools-recent-changelogs.md)
