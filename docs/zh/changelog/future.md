## Future

- Redis 8.8 新类型 Array 的支持（方案见 `plans/18_array-type-support.md`）
  - 18.1 基础读写（ARSCAN / ARGET / ARSET / ARDEL）✅️
  - 18.2 索引范围 / ARLASTITEMS / ARINSERT ✅️
  - 18.3 ARGREP / AROP / ARINFO / ARDELRANGE
- 研究下怎么上线Linux的应用商店
- Vector Set 类型的支持
- Time Series 的支持

### 可逐步完善（小点）

按「单点可做、可验收」排，便于分批落地：

- Viewer 解压查看：Gzip / Deflate / Brotli（对标 TinyRDM / Another）
- 连接级自动解压：优先 GZIP，可选 LZ4 / ZSTD / Snappy（对标 RedisInsight；即此前「自动解压缩」）
- 网络代理：HTTP / SOCKS5
- Unix Socket 支持
- 解析剪贴板 URL / redis:// 反向解析填表
- 树形节点的内存占用显示
- 内存分析：扫描实时显示进度
- 内存分析可以快速停止
- 图表：集群多节点折线同屏展示
- 图表显示优化
- SSH 隧道支持集群和哨兵
- 哨兵支持 SSL
- 旧版 SSH 证书的支持

### 中长期

- RedisSearch 的支持
- redis 的扩展模块命令支持
- Redis8.8 已发布，更新命令列表（等 9 发布再更新吧）、配置文件等
- ACL 管理支持自定义角色
- cli 功能支持
- 黑金主题
- 慢日志治理的微信公众号文章

### 暂缓 / 观望

- 新增 CPU 占比: 需要两次 INFO 才能出来，作用也不大，暂不处理
- 账号登录(谷歌 firebase，腾讯 cloudbase)

### 已完成

- 数据编码魔数自动识别（Auto：ACED / Pickle PROTO / MsgPack / StrJson / UTF-8 / Hex）✅️
- String类型阈值保护 ✅️
- Binary格式恢复支持（setbit等场景还是需要的）✅️
- Hash/Set/ZSet的扫描模式支持 ✅️
- Hash仅查询键的支持 ✅️
- List/Stream支持正序倒序查询 ✅️
- Memory Usage命令不支持时前端显示大小 ✅️
- Java序列化的支持方案 ✅️
- Java/Pickle序列化查看 ✅️（JavaSerial / Pickle 均为只读）
- 命令执行日志✅️
- 搜索历史记录✅️
- 全局快捷键✅️
- 自定义序列化 ✅️
- 终端: 只读命令的识别 ✅️
- 首页连接分组功能 ✅️
- 新增用户管理 ✅️
- 删除多余的I18n键 ✅️
- 复制为命令，支持列表查看中的单行数据 ✅️
- 集群模型下数据库的选择(Valkey9.0开始支持) ✅️
- 键列表支持快速定位正在浏览的键 ✅️
- 收藏键功能 ✅️
- 复制为命令及导出命令(已有方案) ✅️
- 官网升级，参考cc-switch的官网: https://www.ccswitch.io/zh/ ✅️
- 发布订阅的编码支持 ✅️
- 键输入框的提示 ✅️
- 集群模式下的多数据库(valkey9.0支持) ✅️
- 空值保存（新增键、保存值、保存字段值等场景）✅️
- 传输层固定 Base64 + 数据编码仅控展示（Hash/List 字段可直接看 JavaSerial）✅️
- 收藏文件夹功能 ✅️
- Linux Flatpak/Snap 商店版识别（关自更新）✅️
