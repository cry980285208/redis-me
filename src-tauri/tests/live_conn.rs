//! 扩展连接。名字不在 `common/conn.rs` 的 `live_conns` 里就跳过，写进去却连不上则失败。
//!
//! 测的是「这条路能连上并读写」，不重复单机那套翻页：
//! - 普通连接：写入一条中文 String，再用 Base64 读回一段非 UTF-8
//! - 哨兵（含 TLS 哨兵）：先确认 `sentinel masters` 至少一条，再做上面的读写
//! - 集群加 TLS / SSH：跑单机那套往返，再加槽位和跨 slot 重命名
//! - `meta.protocol = resp3` 时，`CLIENT INFO` 里的 `resp` 必须是 3
//! - 系统代理：本机检测不到就跳过
//!
//! 新增一种连法（例如新的代理类型）时，在 `conn.rs` 加一项，并在本文件加同名测试。

#[path = "common/live.rs"]
#[allow(dead_code)]
mod live;
#[path = "common/check.rs"]
#[allow(dead_code)]
mod check;

use redis_me_lib::client::me_client::MeClient;
use redis_me_lib::model::{CliOutputMode, ConnConfig, RedisCommand};
use redis_me_lib::net::conn::sentinel_masters;
use redis_me_lib::net::system_proxy::{DetectOutcome, detect_system_proxy};
use std::time::Duration;

/// 按连接名取扩展连接。`conn.rs` 里没有这一项就跳过。
fn spec(name: &str) -> Option<ConnConfig> {
    live::live_conns()
        .into_iter()
        .find(|item| item.name == name)
}

/// 连上后做同一件数据断言。集群段再跑槽位和跨 slot 重命名。
fn run(name: &str) {
    let Some(spec) = spec(name) else {
        eprintln!("skip: [{name}]");
        return;
    };
    if name == "proxy_system" && matches!(detect_system_proxy(), DetectOutcome::NotFound) {
        eprintln!("skip: proxy_system not detected");
        return;
    }
    if spec.sentinel {
        let masters = sentinel_masters(&spec, Duration::from_secs(15), Duration::from_secs(20))
            .unwrap_or_else(|err| panic!("[{name}] sentinel masters: {err}"));
        assert!(!masters.is_empty(), "sentinel masters is empty");
    }
    let client = live::open_live(&spec).unwrap_or_else(|err| panic!("[{name}] {err}"));
    if spec.cluster {
        check::chinese_and_binary_string(client.as_ref());
        check::key_scan_collects_five(client.as_ref());
        check::hash_list_set_zset_stream_pages(client.as_ref());
        check::optional_modules_when_present(client.as_ref());
        check::cluster_slot_and_rename(client.as_ref());
    } else {
        check::string_roundtrip(client.as_ref());
    }
    if spec.is_resp3() {
        assert_resp3(client.as_ref());
    }
}

/// `CLIENT INFO` 里的 `resp` 必须是 3。
fn assert_resp3(client: &dyn MeClient) {
    let text = client
        .execute_command(RedisCommand {
            command: "CLIENT INFO".into(),
            node: None,
            auto_broadcast: Some(false),
            output_mode: Some(CliOutputMode::Raw),
        })
        .expect("CLIENT INFO");
    assert!(text.contains("resp=3"), "{text}");
}

/// TLS 单机。不校验服务端证书，但会带上 `conn.rs` 里的客户端证书。没写进 `live_conns` 就跳过。
#[test]
fn ssl() {
    run("ssl");
}

/// 双向证书。没配这一节就跳过。
#[test]
fn ssl_mtls() {
    run("ssl_mtls");
}

/// SSH 密码登录。没配这一节就跳过。
#[test]
fn ssh_pwd() {
    run("ssh_pwd");
}

/// SSH 私钥登录。没配这一节就跳过。
#[test]
fn ssh_key() {
    run("ssh_key");
}

/// HTTP 代理。没配这一节就跳过。
#[test]
fn proxy_http() {
    run("proxy_http");
}

/// HTTPS 代理。没配这一节就跳过。
#[test]
fn proxy_https() {
    run("proxy_https");
}

/// SOCKS5 代理，域名由本机解析。没配这一节就跳过。
#[test]
fn proxy_socks5() {
    run("proxy_socks5");
}

/// SOCKS5H 代理，域名由代理解析。没配这一节就跳过。
#[test]
fn proxy_socks5h() {
    run("proxy_socks5h");
}

/// 带账号的代理。没配这一节就跳过。
#[test]
fn proxy_auth() {
    run("proxy_auth");
}

/// 系统代理。本机检测不到就跳过。
#[test]
fn proxy_system() {
    run("proxy_system");
}

/// 哨兵先列出 master，再连上去做读写。没配这一节就跳过。
#[test]
fn sentinel() {
    run("sentinel");
}

/// TLS 哨兵。没配这一节就跳过。
#[test]
fn sentinel_ssl() {
    run("sentinel_ssl");
}

/// 集群加 TLS。没配这一节就跳过。
#[test]
fn cluster_ssl() {
    run("cluster_ssl");
}

/// 集群加 SSH。没配这一节就跳过。
#[test]
fn cluster_ssh() {
    run("cluster_ssh");
}
