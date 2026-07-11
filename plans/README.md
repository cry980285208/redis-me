# plans 方案文档索引

## 命名规范

| 前缀 / 形式               | 含义                                                    |
| ------------------------- | ------------------------------------------------------- |
| `01_`～`NN_` + kebab-case | **已实现**的功能方案（按实现先后顺序编号）              |
| `YYYYMMDD_` + kebab-case  | 阶段性审计 / 对标分析（非功能方案，日期在前，无连字符） |

正文为普通 Markdown；文首「实现状态 / 实际实现 / 关键代码」与最新代码对齐。

## 已实现（编号）

| 编号 | 文件                                                                   | 概要                             |
| ---- | ---------------------------------------------------------------------- | -------------------------------- |
| 01   | [01_command-logging.md](./01_command-logging.md)                       | 命令执行日志                     |
| 02   | [02_value-display-format.md](./02_value-display-format.md)             | 值区多格式查看（前端视图层）     |
| 03   | [03_redis-acl-management.md](./03_redis-acl-management.md)             | ACL 用户管理                     |
| 04   | [04_rdm-import.md](./04_rdm-import.md)                                 | 多 RDM 连接导入                  |
| 05   | [05_custom-formatter.md](./05_custom-formatter.md)                     | 自定义编解码                     |
| 06   | [06_scan-keys-optimization.md](./06_scan-keys-optimization.md)         | 键扫描体验优化                   |
| 07   | [07_favorite-keys.md](./07_favorite-keys.md)                           | 收藏键                           |
| 08   | [08_copy-key-as-command.md](./08_copy-key-as-command.md)               | 单键复制为 redis-cli 命令        |
| 09   | [09_command-export-format.md](./09_command-export-format.md)           | 批量导出/导入 CSV 与 CMD         |
| 10   | [10_cluster-numbered-databases.md](./10_cluster-numbered-databases.md) | 集群多库（Valkey 9，连接时指定） |

## 待实现（编号）

| 编号 | 文件                                                   | 概要                                      |
| ---- | ------------------------------------------------------ | ----------------------------------------- |
| 11   | [11_field-scan-pattern.md](./11_field-scan-pattern.md) | Hash/Set/ZSet 字段扫描 pattern + 前端循环 |

## 分析 / 参考

| 文件                                                                                   | 说明         |
| -------------------------------------------------------------------------------------- | ------------ |
| [20260425_project-deep-audit.md](./20260425_project-deep-audit.md)                     | 项目深度审计 |
| [20260425_redis-client-competitive-gap.md](./20260425_redis-client-competitive-gap.md) | 竞品差距     |
