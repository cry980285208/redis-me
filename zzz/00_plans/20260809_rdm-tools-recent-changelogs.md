# 主流 RDM 工具近期更新日志扫描（2026-08-09）

> **类型**：竞品动态速览（非功能方案）  
> **目的**：从 GitHub / 官方 Release Notes 提炼近 12～18 个月常见 RDM 在做什么，便于 RedisME 选题  
> **关联**：[20260718_rdm-competitive-analysis.md](./20260718_rdm-competitive-analysis.md)、[docs/zh/changelog/future.md](../docs/zh/changelog/future.md)、[18_array-type-support.md](./18_array-type-support.md)

扫描对象与资料来源：

| 产品             | 仓库 / 文档                                                                                                                                                    | 最近关注版本                                           |
| ---------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| Redis Insight    | [redis/RedisInsight](https://github.com/redis/RedisInsight/releases) + [官方 Release Notes](https://redis.io/docs/latest/develop/tools/insight/release-notes/) | 3.0 → **3.8.0**（2026-07）                             |
| Tiny RDM         | [tiny-craft/tiny-rdm](https://github.com/tiny-craft/tiny-rdm/releases)                                                                                         | 1.2.0 → **1.2.7**（2026-03）；作者重心转向 Redisee     |
| Redisee          | [tiny-craft/redisee-app](https://github.com/tiny-craft/redisee-app/releases) + [redisee.com](https://redisee.com)                                              | **v1.1.9**（2026-08-06）；Release 几乎无文字 changelog |
| Another RDM      | [qishibo/AnotherRedisDesktopManager](https://github.com/qishibo/AnotherRedisDesktopManager/releases)                                                           | 1.6.x → **1.7.2**（2026-07-29）                        |
| Redis UI（参考） | [patrikx3/redis-ui](https://github.com/patrikx3/redis-ui) change-log                                                                                           | 2026.4.x：Array + AI command hints                     |

方法：只摘「功能向」条目；安全补丁、构建链、纯 bugfix 从略。与 RedisME 现状对照时，以 `future.md` + 20260718 对标为准。

---

## 1. 跨产品趋势（一眼看方向）

1. **新数据类型抢占心智**：Array（Redis 8.8）、Vector Set（Redis 8）是 Insight 2026 上半年主线；社区客户端（Redis UI）也在跟 Array。
2. **生产安全护栏**：连接标 Dev/Prod、危险操作二次确认（Insight 3.6）——日常 GUI 开始认真对待误删。
3. **云厂商一键连**：Azure Managed Redis / Entra ID（Insight 3.2～3.4）仍是官方护城河，开源 RDM 很少碰。
4. **搜索 / 模块工作区**：Insight 独立 Search 工作区 + Query Library；Tiny/Another 仍不跟模块深度。
5. **连接与隧道打磨**：SSH Agent、SSH×Sentinel、SSL SNI、IPv4/IPv6 显式选择、连接分组补齐。
6. **值格式与编解码**：Markdown 查看、LZ4、MsgPack 检测、Bitset、Hash 字段 TTL、列排序——小步高频。
7. **体验卫生**：What's New、Copy diagnostics、键列表列排序、DB 名过滤、命令从文件导入。

Tiny 作者已公开把新产品放在 **Redisee**；Tiny RDM 进入维护/小优化期。Another 节奏慢但 2026-07 又发了一版（连接分组）。

---

## 2. Redis Insight（官方，更新最密）

资料：[Releases](https://github.com/redis/RedisInsight/releases)、[v3.8.0](https://redis.io/docs/latest/develop/tools/insight/release-notes/v.3.8.0/)、[v3.6.0](https://redis.io/docs/latest/develop/tools/insight/release-notes/v.3.6.0/)、[v3.4.1](https://redis.io/docs/latest/develop/tools/insight/release-notes/v.3.4.1/)、[v3.2.0](https://redis.io/docs/latest/develop/tools/insight/release-notes/v.3.2.0/)、[v3.0.0](https://redis.io/docs/latest/develop/tools/insight/release-notes/v.3.0.0/)

### 3.8.0（2026-07）

- **Array 端到端**：创建（连续/稀疏/样例集）、浏览、多谓词搜索、聚合、行内/多行编辑、按元素/多选/索引范围删除、命令预览；大整数精度保留。
- 值格式新增 **Markdown**。
- 连接：**IPv4 / IPv6** 显式选择；非托管连接可改 host/port。
- What's New 弹窗；Settings **Copy diagnostics**；Query Engine 文案改为 Redis Search；Azure 多租户 Entra 修复。

### 3.6.0（2026-06）

- **Vector Sets** 端到端：创建、VADD、相似度搜索、属性过滤、结果表操作。
- **Dev / Production / Unspecified** 环境标签 + PROD 徽章；生产库危险操作 **type-to-confirm**（批量删、危险 CLI/Workbench、Profiler）；可跳过非生产确认。
- **Geodata Workbench** 插件：GEO 结果地图 / 热力图。
- JSON/String 的 Copy / Download；Settings 显示 build SHA。

### 3.4.1（2026-04）

- 独立 **Search 工作区**：索引生命周期、查询编辑器（Profile/Explain）、Query Library；与 Browser 互跳。
- Azure：Docker Entra、Access Key、手动连接表单。
- Browser 键列表客户端列排序（Key / TTL / Size）；Linux ARM64 包。

### 3.2.0（2026-02）

- Azure Managed Redis / Cache：订阅发现、一键导入、Entra ID OAuth、多账号。

### 3.0.0（2025-11）

- UI / 顶栏导航大改版（对齐 Redis Cloud 视觉）。

**对 RedisME 的含义**：Insight 押注「新类型 + Search + 云 + 生产护栏」。Array 我方已有基础（见 plan 18）；Vector Set / Search / Azure 仍属中长期或观望。Dev/Prod 确认、Markdown Viewer、IPv6 显式选择、诊断信息一键复制，投入小、可跟。

---

## 3. Tiny RDM（轻量开源 → 维护期）

资料：[tiny-craft/tiny-rdm releases](https://github.com/tiny-craft/tiny-rdm/releases)

| 版本         | 时间    | 功能向要点                                                                                    |
| ------------ | ------- | --------------------------------------------------------------------------------------------- |
| **1.2.7**    | 2026-03 | MsgPack 检测优化；长文本删除提示；**HASH 键列手动排序**；公告 Redisee 首发                    |
| **1.2.6**    | 2026-02 | **Bitset 查看**；CLI 显示 null；**SSH Agent**；筛选/连接名 `#`/Stream 反序列化等修复          |
| **1.2.5**    | 2025-08 | CLI 非英文输入法；Pub/Sub 复制消息；大条目加载；Pickle datetime；SSH+TLS 修复                 |
| **1.2.4**    | 2025-07 | Pub/Sub & MONITOR 优化；键匹配默认改**模糊匹配**；终端/IME/TTL 等修复                         |
| 1.2.3～1.2.1 | 2025    | SSH×Sentinel、IPv6、MsgPack 数字、自动刷新与 Tab 干扰等稳定性                                 |
| **1.2.0**    | 2024-08 | **LZ4** 编解码；列表对齐；CLI Home/End                                                        |
| 1.1.x 片段   | 2024    | 批量删免预扫确认；键列表快捷键；非标准 JSON；HASH 单字段刷新；移除复杂类型自动识别（防卡 UI） |

**对 RedisME 的含义**：方向仍是「连接隧道 + Viewer/解码 + 列表体验」。与 `future.md` 已列项高度重合：解压/LZ4、SSH×集群/哨兵、代理/Agent、大条目性能。Bitset、HASH 列排序、键默认模糊匹配、Pub/Sub 复制，都是可单独验收的小点。

---

## 4. Redisee（Tiny 重写，商业向）

- 官网宣称：Standalone / Sentinel / Cluster、树浏览、内置 CLI、监控与慢查询、批量导入导出删、多维搜索过滤。
- GitHub [redisee-app](https://github.com/tiny-craft/redisee-app/releases) 发版很勤（至 **v1.1.9 / 2026-08-06**），但 Release body 几乎无 changelog，**暂无法从 GitHub 细抠功能增量**。
- 建议：后续若要对标，以官网功能页 + 实际安装体验为准，不必等他们的 Release 文案。

---

## 5. Another Redis Desktop Manager（老牌开源）

资料：[qishibo/AnotherRedisDesktopManager releases](https://github.com/qishibo/AnotherRedisDesktopManager/releases)；README Feature Log 另有 2025-10「新功能即将到来」预告。

| 版本         | 时间       | 功能向要点                                                                                                                                           |
| ------------ | ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| **1.7.2**    | 2026-07-29 | **连接分组**；SSL **SNI**；Keys 导出优化；越南语                                                                                                     |
| **1.7.1**    | 2024-12    | 自动深色模式；命令日志 UI；**DB 选择器按名过滤**；Tair JSON                                                                                          |
| **1.7.0**    | 2024-11    | **从文件导入命令**；搜索历史；ZSet ASC/DESC；Console/Slowlog UI                                                                                      |
| **1.6.8**    | 2024-10    | **Hash field TTL**（Redis≥7.4）；各类型列表展示优化                                                                                                  |
| 1.6.6 及更早 | 2024       | 连接搜索/状态；内存分析过滤；HEX_FILE Viewer；DB 自定义名；导出带 TTL；仅加载选中文件夹；CLI 启动参数；Java/Pickle；Stream Groups；集群 per-key info |

**对 RedisME 的含义**：连接分组我方已有，不必跟。值得看的增量：**SSL SNI**、Hash field TTL、从文件导入命令、DB 别名/过滤、导出带 TTL。节奏偏慢，但「生产小功能」仍在补。

---

## 6. Redis UI（patrikx3，轻量参考）

2026.4.x change-log 反复出现：

- Array 类型支持
- AI-powered command hints

说明社区 GUI 也在跟 Array + 轻量 AI 提示；与 Insight Copilot 不是一条赛道，但对「命令辅助」有启发（可观望，不必急）。

---

## 7. RedisME 可开发 / 优化选题（结合本次扫描）

下列按「近期竞品在推 × 我方是否已有/已规划」整理，便于排期。已在 `future.md` 的标「已有规划」。

### A. 建议优先（投入可控、竞品在推、差异感强）

| 选题                                         | 竞品信号                                           | RedisME 现状                    | 备注                                          |
| -------------------------------------------- | -------------------------------------------------- | ------------------------------- | --------------------------------------------- |
| Array 进阶（ARGREP / AROP / 聚合或搜索体验） | Insight 3.8 端到端搜索/聚合                        | 18.1～18.3 基础已落地，进阶延后 | 跟 Insight 体验差距主要在「搜/聚合」而非 CRUD |
| Viewer 解压 + 连接级自动解压（GZIP→LZ4）     | Tiny LZ4；Insight 连接级；Another 历史已有 Gzip 等 | **已有规划**                    | 仍是 P0 级「打开即看」                        |
| SSH Agent / SSH×Cluster·Sentinel / 哨兵 SSL  | Tiny 1.2.6 Agent；Tiny 修 SSH×Sentinel；对标表 P0  | **已有规划**                    | 生产连接刚需                                  |
| HTTP/SOCKS5 代理                             | Tiny 长期能力                                      | **已有规划**                    |                                               |
| 生产库危险操作确认（轻量 Dev/Prod）          | Insight 3.6                                        | 有只读模式，无环境标签/二次确认 | 可做极简版：连接级「生产」开关 + 批量删确认   |
| Hash field TTL                               | Another 1.6.8                                      | 未单列                          | Redis 7.4+ 场景越来越多                       |
| SSL SNI                                      | Another 1.7.2                                      | 需核对是否已支持                | 缺则小补丁                                    |

### B. 体验小步（可穿插发版）

| 选题                                 | 竞品信号                    | 说明                             |
| ------------------------------------ | --------------------------- | -------------------------------- |
| Markdown 值格式                      | Insight 3.8                 | 与现有 Viewer 体系一致，成本低   |
| Bitset / 位图友好查看                | Tiny 1.2.6                  | Binary 已有基础时可增强          |
| HASH（及列表）列手动排序             | Tiny / Insight Browser 排序 | 前端排序即可先做                 |
| 键匹配默认策略（前缀 vs 模糊）可配置 | Tiny 1.2.4 默认模糊         | 设置项即可                       |
| DB 别名 / DB 选择器过滤              | Another                     | 多 DB 用户友好                   |
| 从文件导入命令批量执行               | Another 1.7.0               | 与命令日志/终端相邻              |
| Pub/Sub 消息一键复制                 | Tiny 1.2.5                  | 小优化                           |
| IPv6 / 协议族显式选择                | Insight 3.8；Tiny IPv6      | 连接表单增强                     |
| Copy diagnostics（版本/OS/构建）     | Insight 3.8                 | 设置页一键复制，利于工单         |
| What's New / 版本亮点                | Insight 3.8                 | 可选；changelog 入口已有则可弱化 |

### C. 中长期 / 观望（跟则投入大）

| 选题                  | 竞品信号                      | 建议                              |
| --------------------- | ----------------------------- | --------------------------------- |
| Vector Set            | Insight 3.6；`future.md` 已列 | 等用户真实需求再开 plan           |
| Redis Search 工作区   | Insight 3.4                   | 保持中长期，不做「半吊子索引 UI」 |
| Azure / 云账号发现    | Insight 3.2+                  | 官方护城河，观望                  |
| Geodata 地图          | Insight 3.6 插件              | 利基，低优先级                    |
| AI 命令提示 / Copilot | Insight + Redis UI            | 暂缓（`future.md` 账号/AI 观望）  |
| Redisee 深度对标      | 发版勤但无 changelog          | 装一版手测后再写专项，勿盲跟      |

### D. 我方已较强、不必为「跟新」而做

连接分组、收藏键、命令执行日志、ACL 管理、自定义编解码 / JavaSerial、多 RDM 导入、Valkey 多 DB、只读动态切换、Array 基础读写等——见 20260718 §1。Another 1.7.2 才补连接分组，说明我方节奏在这条上已领先。

---

## 8. 建议的下一步（给排期用）

不必一次全做。较自然的三条线：

1. **连接可靠线**（代理 / SSH Agent / SSH×集群·哨兵 / SNI）——对齐 Tiny/Another 生产场景。
2. **打开即看线**（解压 Viewer → 连接级自动解压 → Markdown/Bitset）——对齐 Insight/Tiny 日常浏览。
3. **新类型与安全线**（Array 进阶体验；可选 Dev/Prod 确认；Hash field TTL）——对齐 Insight 2026 叙事中「用得上」的部分。

更细的能力矩阵与历史差距仍以 [20260718_rdm-competitive-analysis.md](./20260718_rdm-competitive-analysis.md) 为准；本文只补充「他们最近几个版本实际在发什么」。

---

## 9. 资料链接速查

- Insight Releases: https://github.com/redis/RedisInsight/releases
- Insight Notes: https://redis.io/docs/latest/develop/tools/insight/release-notes/
- Tiny RDM: https://github.com/tiny-craft/tiny-rdm/releases
- Redisee releases: https://github.com/tiny-craft/redisee-app/releases
- Another RDM: https://github.com/qishibo/AnotherRedisDesktopManager/releases
- Redis UI changelog: https://github.com/patrikx3/redis-ui/blob/HEAD/change-log.md
