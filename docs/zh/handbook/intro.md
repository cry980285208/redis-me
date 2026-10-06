# 实战手册

本分区收录用 RedisME 排查与解决实际问题的文章，按场景组织操作步骤与思路。

与 [使用指南](/zh/guide/intro/about) 的区别：

- **使用指南**：介绍产品功能与界面用法
- **实战手册**：围绕具体问题，演示如何用 RedisME 定位、分析并处理

## 文章列表

- [慢日志治理](/zh/handbook/slowlog-governance)：生产慢日志专项实践（RedisME 观测与改造闭环）
- [SSL 加密改造](/zh/handbook/ssl-encryption)：新建 TLS 集群，应用各挑 2 个场景验证，同一个迭代内全部上线；并行期间把旧集群数据同步到新集群，确认已无应用连接后再停同步并下线

## Redis安装

- [Redis Docker 安装](/zh/handbook/redis-install)：按目标环境生成可复制到 Linux 的 Docker 脚本
- [TLS 证书](/zh/handbook/tls-cert)：生成 Redis TLS 自签证书的 OpenSSL 脚本
