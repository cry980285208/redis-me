//! 活测试连哪台 Redis，只改本文件顶部的常量。
//!
//! 端口沿用安装向导的默认值：明文单机 6379、集群 7001、哨兵进程 27701；
//! TLS 单机 6380、集群 8001、哨兵进程 28801。哨兵主库名是 `mymaster`。
//! Redis 密码读 `REDIS_ME_REDIS_PASSWORD`，SSH 读 `REDIS_ME_SSH_PASSWORD`，没设就是无密码。
//! 客户端证书填 `SSL_CERT` / `SSL_KEY`，CA 可选。这台机器的 TLS 会要求客户端证书，三个都留空会连不上。
//!
//! `single` / `cluster` 现在都返回 `Some`，所以单机和集群测试总会去连，连不上就是失败。
//! `live_conns` 的名字要和 `live_conn.rs` 的测试函数一致。没写进去的会跳过：
//! 私钥登录、各种代理、以及再单独配一份证书的 `ssl_mtls`。这台机器没有那些环境。
//!
//! 怎么跑：
//! - `cargo test --lib` 不连 Redis。
//! - 活测试一次只写一个 `--test`：`live_single`、`live_cluster`、`live_conn`。多个 `--test` 时 Cargo 只跑最后一个。
//! - `tls_x509` 只读 `fixtures/` 里的证书，不连网。
//! - 键 SCAN 的 COUNT 在 `live.rs`，是 1000，因为要走完整个库。字段扫描仍是 2，用来翻页。
//! - 只删 `redis-me:test:` 前缀的临时键。不要在测试里 FLUSH 或 `CONFIG SET`。

use redis_me_lib::model::{ConnConfig, SentinelOption, SshOption, SslOption};

const HOST: &str = "ali.hepengju.com";
const SINGLE_DB: u16 = 15;
const MASTER_NAME: &str = "mymaster";

const PORT_SINGLE: u16 = 6379;
const PORT_CLUSTER: u16 = 7001;
const PORT_SENTINEL: u16 = 27_701;
const PORT_SINGLE_SSL: u16 = 6380;
const PORT_CLUSTER_SSL: u16 = 8001;
const PORT_SENTINEL_SSL: u16 = 28_801;

const SSH_PORT: u16 = 22;
const SSH_USER: &str = "root";

const REDIS_PASSWORD_ENV: &str = "REDIS_ME_REDIS_PASSWORD";
const SSH_PASSWORD_ENV: &str = "REDIS_ME_SSH_PASSWORD";

/// 客户端证书、私钥、CA。CA 可留空。安装向导生成的一般是 `redis.crt`、`redis.key`、`ca.crt`。
const SSL_CERT: &str = "~/redis-ssl/redis.crt";
const SSL_KEY: &str = "~/redis-ssl/redis.key";
const SSL_CA: &str = "~/redis-ssl/ca.crt";

/// 单机。15 号库。
pub fn single() -> Option<ConnConfig> {
    Some(open("single", PORT_SINGLE, SINGLE_DB))
}

/// 集群种子。只写一台，其余节点由集群自己发现。
pub fn cluster() -> Option<ConnConfig> {
    Some(as_cluster(open("cluster", PORT_CLUSTER, 0)))
}

/// 扩展连接。名字要和 `live_conn.rs` 里的测试函数一致。
pub fn live_conns() -> Vec<ConnConfig> {
    vec![
        as_ssl(open("ssl", PORT_SINGLE_SSL, SINGLE_DB)),
        as_ssh(open("ssh_pwd", PORT_SINGLE, SINGLE_DB)),
        as_sentinel(open("sentinel", PORT_SENTINEL, SINGLE_DB)),
        as_ssl(as_sentinel(open(
            "sentinel_ssl",
            PORT_SENTINEL_SSL,
            SINGLE_DB,
        ))),
        as_ssl(as_cluster(open("cluster_ssl", PORT_CLUSTER_SSL, 0))),
        as_ssh(as_cluster(open("cluster_ssh", PORT_CLUSTER, 0))),
    ]
}

/// 一台 Redis。`name` 用来对上测试名。
fn open(name: &str, port: u16, db: u16) -> ConnConfig {
    ConnConfig {
        id: name.into(),
        name: name.into(),
        host: HOST.into(),
        port,
        db,
        password: secret(REDIS_PASSWORD_ENV),
        ..ConnConfig::default()
    }
}

fn as_ssl(mut conf: ConnConfig) -> ConnConfig {
    conf.ssl = true;
    conf.ssl_option = SslOption {
        cert: SSL_CERT.into(),
        key: SSL_KEY.into(),
        ca: SSL_CA.into(),
    };
    conf
}

fn as_cluster(mut conf: ConnConfig) -> ConnConfig {
    conf.cluster = true;
    conf.db = 0;
    conf
}

/// 哨兵地址写在 `host`/`port`，主库账号用同一份 Redis 密码。
fn as_sentinel(mut conf: ConnConfig) -> ConnConfig {
    conf.sentinel = true;
    conf.sentinel_option = SentinelOption {
        master_name: MASTER_NAME.into(),
        master_password: secret(REDIS_PASSWORD_ENV),
        ..SentinelOption::default()
    };
    conf
}

/// SSH 落到同一台机器，再连 `conf` 里的 Redis 端口。不要和代理一起开。
fn as_ssh(mut conf: ConnConfig) -> ConnConfig {
    conf.ssh = true;
    conf.ssh_option = SshOption {
        host: HOST.into(),
        port: SSH_PORT,
        login_type: "pwd".into(),
        username: SSH_USER.into(),
        password: secret(SSH_PASSWORD_ENV),
        ..SshOption::default()
    };
    conf
}

fn secret(name: &str) -> String {
    super::env_secret(name)
}
