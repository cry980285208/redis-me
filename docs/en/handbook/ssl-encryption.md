# SSL Encryption

Redis has supported TLS natively since 6.0. The settings are `tls-*`, and the URL scheme is `rediss://`. This article uses “SSL” in the everyday sense: that encrypted channel.

## Background

Redis listens in plaintext by default. Passwords, commands, and values are on the wire; a packet capture is enough to replay them. **A Level 3 classified-protection assessment (等保三级) treats unencrypted transport as a finding** and requires Redis 6 or newer. Earlier versions have no TLS, so it cannot be turned on in the old process.

Switching the existing cluster to `tls-port` only drops every plaintext client at once. This time we stood up a TLS cluster beside it, synced the old cluster’s data with RedisShake, and moved applications over in a single iteration by changing configuration. The old cluster stayed up until every application had moved and no application connections remained.

## Goals

- The new cluster disables the plaintext port (`port 0`) and listens only on `tls-port`. Replication and the cluster bus use TLS as well
- After the cutover, applications use the new cluster, and the old cluster is retired

## Approach

Issue certificates → new cluster → sync the old cluster to the new one while both run → migrate and release every application in one iteration (two scenarios per application) → confirm the old cluster has no application connections → stop sync and retire the old cluster.

Move through dev, test, staging, and production. Do not cut applications over in the next environment until the current one has been accepted.

### 1. Build a new cluster instead of changing the old one

If the old cluster is still 4.x or 5.x, move to 6 or newer (7.x official images in this case). The new cluster is built on its own machines and runs beside the old one. Applications point a new alias (such as `redis-ssl-1`) at the new address. Rolling back is pointing the alias back.

Cluster mode uses the host network. Write the IP that is actually reachable on the machine.

### 2. Certificates

Have a CA sign the server certificate. `tls-ca-cert-file` points at `ca.crt`. `redis.crt` is the server certificate, not the CA.

| File        | Role                                                                                                     |
| ----------- | -------------------------------------------------------------------------------------------------------- |
| `ca.key`    | CA private key. Keep it on the machine that signs certificates. Do not put it in an image or on a client |
| `ca.crt`    | CA. Redis `tls-ca-cert-file`, used to verify client certificates                                         |
| `redis.key` | Server private key. Clients use it as well when mutual TLS is on                                         |
| `redis.crt` | Server certificate. Extended key usage includes `serverAuth` and `clientAuth`                            |

Put every name a client actually connects with into the SAN: each node IP, `127.0.0.1`, `localhost`, and the DNS aliases in application config (such as `redis-ssl-1`).

Use OpenSSL 3.2 or newer, which issues X.509 v3 by default. Older OpenSSL (CentOS 7, for example) may issue v1, and some TLS stacks reject that. The private key should be readable only by the user Redis runs as. The generator at the end of this article is the same one RedisME uses.

Java needs `redis.crt` and `redis.key` packed into a keystore, used only as the client certificate. Recent JDK 8 builds and JDK 17/21 can use PKCS12. Oracle JDK 8u202 fails to load PKCS12 with `Invalid secret key format`; use JKS there. Use one store format for every JDK in the same environment.

```bash
openssl pkcs12 -export -in redis.crt -inkey redis.key -certfile ca.crt \
  -out redis.p12 -passout pass:your-store-password

# Convert to JKS when an old Oracle JDK 8 cannot load PKCS12
keytool -importkeystore -srckeystore redis.p12 -srcstoretype PKCS12 \
  -destkeystore redis.jks -deststoretype JKS \
  -srcstorepass 'your-store-password' -deststorepass 'your-store-password'
```

### 3. Server settings that matter

`port 0` turns plaintext listening off. `tls-replication` and `tls-cluster` put replication and the cluster bus on TLS as well.

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

- `tls-auth-clients` defaults to `yes`, so clients must present a certificate. `redis.crt` includes `clientAuth`, and clients reuse it. For encryption only, set it to `no` explicitly.
- The cluster bus is the client port plus 10000. Open both in the firewall.
- With more than one NIC, set `cluster-announce-ip`. Otherwise `CLUSTER SLOTS` advertises an address clients cannot reach.
- Mount the certificates into the container read-only. The official image runs as uid 999 and must be able to read them.

Wait until every node answers `PONG`, then create the cluster:

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

Hand the address to applications and to the sync tool only after `cluster_state:ok`.

### 4. Two scenarios per application

Pick two scenarios from how the application actually uses Redis. Skip a full regression. If the address is configured but the code never calls Redis, leave it off the schedule. Applications already slated for retirement are noted in the release notes and left unchanged.

| Usage            | What you need to see                                                                       |
| ---------------- | ------------------------------------------------------------------------------------------ |
| Connect only     | The process starts and the health check passes                                             |
| Cache read       | One read path returns the same hit as the old cluster, or misses and refills               |
| Cache write      | A write can be read back                                                                   |
| Distributed lock | Two concurrent attempts: the second is blocked, and a later attempt succeeds after release |
| ID / increment   | Consecutive IDs are monotonic, unique, and do not go backwards                             |
| Idempotency      | The same business id returns the first result on the second call                           |

Prefer locks, ID generation, login state, and gateway auth. For caches with a TTL, confirm the TTL is still set after the cutover.

Applications that share keys move in the same batch. While both clusters run, writes on the old cluster are synced to the new one. If only half of those applications move, the ones still on the old cluster overwrite updates the others already made on the new cluster.

Where a shared starter already wraps SSL, upgrade it, turn the switch on, and check the two scenarios. Anything that does not go through the starter is changed as in the next section.

### 5. Clients

Use `rediss://`, or the client’s own SSL switch. A cluster client still needs only one node.

Certificates are issued as in section 2, with a SAN. During the application migration the certificates did not have a SAN yet, so every language skipped hostname checks and trusted every CA. Clients still present `redis.crt` / `redis.key`; otherwise the server rejects the handshake. Java keystores are PKCS12 or JKS.

#### Lettuce

`disablePeerVerification()` is `SslVerifyMode.NONE`.

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

Node addresses use `rediss://host:port`. `SslVerificationMode.NONE` skips both certificate and hostname checks. Older releases use `setSslEnableEndpointIdentification(false)`.

```java
config.useClusterServers()
        .addNodeAddress(nodeAddresses)          // rediss://10.0.0.11:8001
        .setPassword(password)
        .setSslVerificationMode(SslVerificationMode.NONE)
        .setSslKeystore(clientStoreUrl)
        .setSslKeystorePassword(storePassword);
```

#### Jedis

An empty `X509TrustManager` trusts every certificate. Do not set a `HostnameVerifier`. `keyManagers` comes from the client keystore.

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

`InsecureSkipVerify: true` skips both certificate and hostname checks.

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

In redis-py 5, `ssl_cert_reqs` defaults to `required`. Set it to `none`. `ssl_check_hostname` already defaults to `False`; set it here anyway.

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

### 6. Sync while both clusters run

[RedisShake](https://tair-opensource.github.io/RedisShake/en/guide/mode.html) `sync_reader` pretends to be a replica of the old cluster: a full RDB first, then the incremental stream. A self-hosted cluster supports PSYNC, so use this mode. If the cloud endpoint has no PSYNC, switch to `scan_reader`.

4.x has no checkpoint resume and does not notice cluster topology changes. It covers this parallel window only. If replication drops, start it again. A too-small `client-output-buffer-limit replica` on the source is a common cause. The target version should be at least the source version.

```toml
[sync_reader]
cluster = true
address = "10.0.0.1:7001"    # any node of the old cluster, plaintext
password = "old-password"
tls = false
sync_rdb = true
sync_aof = true

[redis_writer]
cluster = true
address = "10.0.0.11:8001"   # any node of the new cluster
password = "new-password"
tls = true

# Present a client certificate to the new cluster. Match the sample shipped with your build.
[redis_writer.tls_config]
cert = "/path/redis.crt"
key = "/path/redis.key"
ca_cert = "/path/ca.crt"
```

`tls = true` means RedisShake does not verify the server certificate. The new cluster still requires a client certificate by default. A build without `tls_config` fails the handshake: use a build that has this block, or set `tls-auth-clients` to `no` for the sync and set it back to `yes` with `CONFIG REWRITE` on every node when the sync ends.

Start the sync against an empty new cluster. The full sync uses `RESTORE` and exits by default when a key already exists. To run the full sync again, set `rdb_restore_command_behavior` to `rewrite`. Applications should not write to the new cluster until incremental sync has caught up.

Caught up means RedisShake is in the incremental phase, and keys still being updated on the old cluster have the same value on the new one. `DBSIZE` is per node; sum the nodes.

### 7. Cutover

1. The cluster reports `cluster_state:ok`, and sync is in the incremental phase.
2. Point every application at the new address within the same iteration. Applications that share keys move in the same batch.
3. Check each batch with the two scenarios you picked. On failure, point that batch back at the old address. Sync is still running.
4. When every planned application points at the new cluster, check the client list on each old node and confirm no application connections remain. RedisShake’s own connection does not count. Then stop the sync.
5. Check the client list once more before retiring the old cluster. Retire it only when application connections are still absent.

## Where RedisME fits

1. **Certificates and install**  
   [TLS certificates](/handbook/tls-cert) generates the set in section 2: a CA and a `redis.crt` with a SAN (`serverAuth` + `clientAuth`). [Redis Docker install](/handbook/redis-install) can emit Compose and the cluster-create command with `port 0`, `tls-port`, `tls-cluster`, and `tls-replication`. “Redis install help” in the client is the same generator.
2. **Checking the clusters**  
   In [Connections](/guide/usage/connection), enable SSL and cluster mode, and set the public certificate and private key to `redis.crt` / `redis.key`. The connection does not verify the server certificate; the CA field can be left empty. One node is enough to list the whole cluster. Use the client list to confirm the old cluster has no application connections left. X.509 v1 certificates connect as well.
3. **Changing TLS settings**  
   Change `tls-auth-clients` in the config view and broadcast it to every node, then run `CONFIG REWRITE` in the [terminal](/guide/usage/terminal).

Further reading: [Redis TLS](https://redis.io/docs/latest/operate/oss_and_stack/management/security/encryption/), [RedisShake migration modes](https://tair-opensource.github.io/RedisShake/en/guide/mode.html), [Sync Reader](https://tair-opensource.github.io/RedisShake/en/reader/sync_reader.html), [Redis Writer](https://tair-opensource.github.io/RedisShake/en/writer/redis_writer.html).
