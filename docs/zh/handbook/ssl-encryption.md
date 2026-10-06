# SSL 加密改造

Redis 6.0 起原生支持 TLS。配置项叫 `tls-*`，连接串是 `rediss://`。下文按日常说法写成 SSL。

## 背景简述

Redis 默认明文。口令、命令和值都在链路上，抓包就能复现。**等保三级一类测评会把「传输未加密」列为问题**，并要求实例不低于 6。更早的版本没有 TLS，没法在旧进程上打开。

原地改成只监听 `tls-port` 会立刻断开所有明文客户端。这次并列建一套 TLS 集群，用 RedisShake 把旧集群的数据同步过去，应用在同一个迭代内改配置切过去。旧集群留到全部切完，并且确认已经没有应用连接。

## 改造目标

- 新集群禁用明文端口（`port 0`），只监听 `tls-port`；主从复制和集群总线也走 TLS
- 应用切完后业务连接新集群，旧集群下线

## 改造方案

签发证书 → 新集群 → 并行期把旧集群数据同步到新集群 → 同一个迭代内改造并全部上线（每个应用挑 2 个场景）→ 确认旧集群已无应用连接 → 停同步并下线。

环境按开发、测试、预发、生产推进。

### 1. 新建集群，不在旧实例上改端口

旧集群若还是 4.x / 5.x，换成 6 及以上（实践里是 7.x 官方镜像）。新集群在新的机器单独搭建，与旧集群并存。应用用新别名（如 `redis-ssl-1`）指向新地址，回退就是把别名指回去。

集群走宿主机网络，配置里写机器上真实可达的 IP。

### 2. 证书

用 CA 签发服务端证书。`tls-ca-cert-file` 指向 `ca.crt`，不要和 `redis.crt` 写成同一个文件。

| 文件        | 用途                                                  |
| ----------- | ----------------------------------------------------- |
| `ca.key`    | CA 私钥，只留在签发机，不要放进镜像或客户端           |
| `ca.crt`    | CA。Redis 的 `tls-ca-cert-file`，用来校验客户端证书   |
| `redis.key` | 服务端私钥。双向认证时客户端也用它                    |
| `redis.crt` | 服务端证书。扩展用途包含 `serverAuth` 和 `clientAuth` |

SAN 写上客户端实际连接用的名字：每个节点的 IP、`127.0.0.1`、`localhost`，以及应用配置里的 DNS 别名（如 `redis-ssl-1`）。

用 OpenSSL 3.2 及以上，默认签发 X.509 v3。更老的 OpenSSL（如 CentOS 7）可能签出 v1，部分 TLS 库会拒绝。私钥只给运行 Redis 的用户读。生成脚本见文末，和 RedisME 里的是同一套。

Java 要把 `redis.crt` 和 `redis.key` 收成密钥库，只当客户端证书。JDK 8 新版本和 JDK 17/21 用 PKCS12。Oracle JDK 8u202 加载 PKCS12 会报 `Invalid secret key format`，改用 JKS。同一套环境的 JDK 用同一种格式。

```bash
openssl pkcs12 -export -in redis.crt -inkey redis.key -certfile ca.crt \
  -out redis.p12 -passout pass:your-store-password

# 旧版 Oracle JDK 8 加载 PKCS12 失败时，再转成 JKS
keytool -importkeystore -srckeystore redis.p12 -srcstoretype PKCS12 \
  -destkeystore redis.jks -deststoretype JKS \
  -srcstorepass 'your-store-password' -deststorepass 'your-store-password'
```

### 3. 服务端关键配置

`port 0` 是关掉明文监听。`tls-replication` 和 `tls-cluster` 让复制和集群总线也走 TLS。

```conf
port 0
tls-port 8001
tls-cert-file /etc/redis/redis.crt
tls-key-file /etc/redis/redis.key
tls-ca-cert-file /etc/redis/ca.crt
tls-protocols "TLSv1.2 TLSv1.3"
tls-replication yes
tls-cluster yes

cluster-enabled yes
cluster-announce-ip 10.0.0.11
cluster-announce-port 8001
cluster-announce-bus-port 18001
```

- `tls-auth-clients` 默认 `yes`，客户端必须出示证书。`redis.crt` 带了 `clientAuth`，客户端复用它即可。只做传输加密时显式写成 `no`。
- 集群总线是对外端口 + 10000，防火墙两边都要放行。
- 多网卡要写 `cluster-announce-ip`，否则 `CLUSTER SLOTS` 会带上不可达地址。
- 证书只读挂进容器。官方镜像里 redis 用户是 uid 999，要能读到文件。

每个节点都 `PONG` 后再建集群：

```bash
redis-cli --tls \
  --cert /etc/redis/redis.crt \
  --key /etc/redis/redis.key \
  --cacert /etc/redis/ca.crt \
  -a 'your-password' --no-auth-warning \
  --cluster create \
  10.0.0.11:8001 10.0.0.11:8002 10.0.0.11:8003 \
  10.0.0.12:8004 10.0.0.12:8005 10.0.0.12:8006 \
  --cluster-replicas 1 --cluster-yes
```

`cluster_state:ok` 之后再交给应用和同步。

### 4. 每个应用挑 2 个场景

按真实用法挑 2 个，不做全量回归。配置了地址但代码没用到的不排期。准备下线的在发布说明里标注，不改造。

| 用法       | 最少要看到的结果                                         |
| ---------- | -------------------------------------------------------- |
| 只建连     | 进程起来，健康检查通过                                   |
| 读缓存     | 走一条读路径，命中的值与旧集群一致，或未命中时能回源写回 |
| 写缓存     | 写入后再读回                                             |
| 分布式锁   | 并发两次，第二次被挡住；释放后可以再次拿到               |
| 发号、自增 | 连续取号单调、不重复、不回退                             |
| 幂等       | 同一业务单号第二次返回第一次的结果                       |

优先挑锁、发号、登录态、网关鉴权。带过期时间的缓存，切过去之后确认过期时间还在。

共用 key 的应用同一批切。并行期间旧集群上的写入会同步到新集群，只切一半时，后切的一方会盖掉先切一方在新集群上的更新。

公共 starter 已经封装 SSL 的，升级并打开开关，按这 2 个场景验收。没走 starter 的，按下一节改。

### 5. 客户端

`rediss://`，或客户端自己的 SSL 开关。集群只配一个节点即可。

证书按第 2 节签发，带 SAN。应用改造时证书还没有 SAN，所以各语言不校验主机名，并信任全部 CA。客户端证书用 `redis.crt` / `redis.key`，否则服务端拒绝握手。Java 的密钥库是 PKCS12 或 JKS。

#### Lettuce

`disablePeerVerification()` 就是 `SslVerifyMode.NONE`。

```java
SslOptions sslOptions = SslOptions.builder()
        .keystore(clientStoreUrl, storePassword)
        .build();
ClientOptions clientOptions = ClientOptions.builder().sslOptions(sslOptions).build();

LettuceClientConfiguration clientConfig = LettuceClientConfiguration.builder()
        .clientOptions(clientOptions)
        .useSsl()
        .disablePeerVerification()
        .build();
return new LettuceConnectionFactory(clusterConfig, clientConfig);
```

#### Redisson

地址用 `rediss://host:port`。`SslVerificationMode.NONE` 不校验证书，也不校验主机名。旧版本写 `setSslEnableEndpointIdentification(false)`。

```java
config.useClusterServers()
        .addNodeAddress(nodeAddresses)          // rediss://10.0.0.11:8001
        .setPassword(password)
        .setSslVerificationMode(SslVerificationMode.NONE)
        .setSslKeystore(clientStoreUrl)
        .setSslKeystorePassword(storePassword);
```

#### Jedis

空的 `X509TrustManager` 信任全部证书。不设置 `HostnameVerifier`。`keyManagers` 来自客户端密钥库。

```java
TrustManager[] trustAll = new TrustManager[]{
        new X509TrustManager() {
            public void checkClientTrusted(X509Certificate[] chain, String authType) {}
            public void checkServerTrusted(X509Certificate[] chain, String authType) {}
            public X509Certificate[] getAcceptedIssuers() { return new X509Certificate[0]; }
        }
};

SSLContext sslContext = SSLContext.getInstance("TLS");
sslContext.init(keyManagers, trustAll, null);

DefaultJedisClientConfig config = DefaultJedisClientConfig.builder()
        .password(password)
        .ssl(true)
        .sslSocketFactory(sslContext.getSocketFactory())
        .build();
JedisCluster cluster = new JedisCluster(hostAndPorts, config, poolConfig);
```

#### Go

`InsecureSkipVerify: true` 同时跳过证书校验和主机名校验。

```go
// github.com/redis/go-redis/v9
cert, err := tls.LoadX509KeyPair("redis.crt", "redis.key")
if err != nil {
    return err
}

rdb := redis.NewClusterClient(&redis.ClusterOptions{
    Addrs:    []string{"redis-ssl-1:8001", "redis-ssl-2:8002"},
    Password: "your-password",
    TLSConfig: &tls.Config{
        Certificates:       []tls.Certificate{cert},
        InsecureSkipVerify: true,
        MinVersion:         tls.VersionTLS12,
    },
})
```

#### Python

redis-py 5 的 `ssl_cert_reqs` 默认是 `required`，要写成 `none`。`ssl_check_hostname` 默认已是 `False`，这里仍写上。

```python
import redis

client = redis.RedisCluster(
    host="redis-ssl-1",
    port=8001,
    password="your-password",
    ssl=True,
    ssl_cert_reqs="none",
    ssl_check_hostname=False,
    ssl_certfile="redis.crt",
    ssl_keyfile="redis.key",
)
client.ping()
```

### 6. 并行期同步

用 [RedisShake](https://tair-opensource.github.io/RedisShake/zh/guide/mode.html) 的 `sync_reader`：伪装成旧集群的副本，先拉全量 RDB，再转发增量。自建集群支持 PSYNC，用这个。云上没有 PSYNC 时再改 `scan_reader`。

4.x 没有断点续传，也不感知集群拓扑变化，只覆盖这次并行期。复制断开后要重新拉起。源端 `client-output-buffer-limit replica` 过小是断连的常见原因。目标版本不低于源端。

```toml
[sync_reader]
cluster = true
address = "10.0.0.1:7001"    # 旧集群任意节点，明文
password = "old-password"
tls = false
sync_rdb = true
sync_aof = true

[redis_writer]
cluster = true
address = "10.0.0.11:8001"   # 新集群任意节点
password = "new-password"
tls = true

# 向新集群出示客户端证书。字段以安装包自带示例为准。
[redis_writer.tls_config]
cert = "/path/redis.crt"
key = "/path/redis.key"
ca_cert = "/path/ca.crt"
```

`tls = true` 表示 RedisShake 不校验服务端证书。新集群默认仍要客户端证书，没有 `tls_config` 的旧版会握手失败：换带这段配置的版本，或同步期间把 `tls-auth-clients` 设为 `no`，结束再改回 `yes` 并 `CONFIG REWRITE`（每个节点）。

新集群先空着再同步。全量用 `RESTORE`，遇到同名 key 默认退出。重跑全量时把 `rdb_restore_command_behavior` 设为 `rewrite`。增量跟上之前，应用不要写新集群。

跟上的标准：已经进入增量，旧集群上还在更新的 key，新集群上的值一致。`DBSIZE` 是单节点的，按节点加总。

### 7. 切换

1. 集群 `cluster_state:ok`，同步进入增量。
2. 同一个迭代内全部改到新地址。共用 key 的应用同一批。
3. 每批用挑好的 2 个场景验收。失败就把这批配置指回旧地址，同步还在。
4. 都切完后看旧集群各节点的客户端列表，确认没有应用连接。RedisShake 自己的连接不算。然后停同步。
5. 下线前再看一次客户端列表，仍然没有应用连接后再下线。

## RedisME 在改造中的作用

1. **证书与安装**  
   [TLS 证书](/zh/handbook/tls-cert) 生成的就是第 2 节这套：CA、带 SAN 的 `redis.crt`（`serverAuth` + `clientAuth`）。[Redis Docker 安装](/zh/handbook/redis-install) 能生成 `port 0`、`tls-port`、`tls-cluster`、`tls-replication` 的 Compose 和建集群命令。客户端里的「Redis 安装帮助」是同一套生成器。
2. **验收**  
   [连接](/zh/guide/usage/connection) 勾选 SSL 和集群，公钥、私钥填 `redis.crt` / `redis.key`。连接不校验服务端证书，授权（CA）可以不填。填一个节点就能列出全部节点。客户端列表用来确认旧集群已经没有应用连接。X.509 v1 证书也能连。
3. **改 TLS 参数**  
   `tls-auth-clients` 可在配置界面改完并广播到所有节点，再在 [终端](/zh/guide/usage/terminal) 里执行 `CONFIG REWRITE`。

相关说明：[Redis TLS](https://redis.io/docs/latest/operate/oss_and_stack/management/security/encryption/)、[RedisShake 迁移模式](https://tair-opensource.github.io/RedisShake/zh/guide/mode.html)、[Sync Reader](https://tair-opensource.github.io/RedisShake/zh/reader/sync_reader.html)、[Redis Writer](https://tair-opensource.github.io/RedisShake/zh/writer/redis_writer.html)。
