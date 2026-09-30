//! 扩展连接。toml 里没有的段跳过，写了却连不上则失败。

#![allow(dead_code)]

#[path = "common/redis.rs"]
mod redis;
#[path = "common/roundtrip.rs"]
mod roundtrip;

use redis::LiveConn;
use redis_me_lib::client::client_trait::MeClient;
use redis_me_lib::model::{CliOutputMode, RedisCommand};
use redis_me_lib::net::conn::sentinel_masters;
use redis_me_lib::net::system_proxy::{DetectOutcome, detect_system_proxy};
use std::time::Duration;

/// 按段名取扩展连接。toml 里没有这一段就跳过。
fn spec(name: &str) -> Option<LiveConn> {
    match redis::load_live_conns() {
        Ok(list) => list.into_iter().find(|item| item.name == name),
        Err(err) => panic!("redis conn config: {err}"),
    }
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
        let masters = sentinel_masters(
            &redis::live_config(&spec),
            Duration::from_secs(15),
            Duration::from_secs(20),
        )
        .unwrap_or_else(|err| panic!("[{name}] sentinel masters: {err}"));
        assert!(!masters.is_empty(), "sentinel masters is empty");
    }
    let client = redis::open_live(&spec).unwrap_or_else(|err| panic!("[{name}] {err}"));
    if spec.cluster {
        roundtrip::chinese_and_binary_string(client.as_ref());
        roundtrip::key_scan_collects_five(client.as_ref());
        roundtrip::hash_list_set_zset_stream_pages(client.as_ref());
        roundtrip::optional_modules_when_present(client.as_ref());
        roundtrip::cluster_slot_and_rename(client.as_ref());
    } else {
        roundtrip::string_roundtrip(client.as_ref());
    }
    if spec.resp3 {
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

/// 不校验证书的 TLS。没配这一节就跳过。
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
