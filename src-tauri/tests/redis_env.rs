//! 只测配置解析，不连接 Redis。
//! 与 `live_*.rs` 共用 `common/redis.rs`，活测试辅助函数在本 crate 里用不到。
#![allow(dead_code)]

#[path = "common/redis.rs"]
mod redis;

use std::fs;
use std::sync::{Mutex, MutexGuard};

use redis::{Endpoint, parse_live_conns, resolve, toml_text_from};

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvGuard {
    _lock: MutexGuard<'static, ()>,
    saved: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    fn set(pairs: &[(&'static str, Option<&str>)]) -> Self {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|err| err.into_inner());
        let saved = pairs
            .iter()
            .map(|(key, _)| (*key, std::env::var(key).ok()))
            .collect();
        for (key, value) in pairs {
            // Rust 2024 将修改进程环境标为 unsafe；测试用锁串行，避免互相覆盖。
            unsafe {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
        Self { _lock, saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in &self.saved {
            unsafe {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
}

fn endpoint(host: &str, port: u16, db: u16, username: &str, password: &str) -> Endpoint {
    Endpoint {
        host: host.into(),
        port,
        db,
        username: username.into(),
        password: password.into(),
    }
}

#[test]
fn url_parses_host_port_db_and_password() {
    let profile = resolve(Some("redis://alice:s3cret@127.0.0.1:6380/15"), None, None).unwrap();
    assert_eq!(
        profile.single,
        Some(endpoint("127.0.0.1", 6380, 15, "alice", "s3cret"))
    );
    assert!(profile.cluster.is_none());
}

#[test]
fn missing_file_and_env_has_no_single() {
    let profile = resolve(None, None, None).unwrap();
    assert!(profile.single.is_none());
    assert!(profile.cluster.is_none());
}

#[test]
fn redis_url_ignores_single_in_file() {
    let toml = r#"
[single]
url = "redis://10.0.0.8:6379/1"

[cluster]
nodes = ["10.0.0.9:7001"]
password = "file-secret"
"#;
    let profile = resolve(Some("redis://127.0.0.1:6379/15"), None, Some(toml)).unwrap();
    assert_eq!(
        profile.single,
        Some(endpoint("127.0.0.1", 6379, 15, "", ""))
    );
    let cluster = profile.cluster.unwrap();
    assert_eq!(cluster.nodes[0].host, "10.0.0.9");
    assert_eq!(cluster.nodes[0].password, "file-secret");
}

#[test]
fn cluster_url_ignores_file_cluster() {
    let toml = r#"
[cluster]
nodes = ["10.0.0.9:7001"]
password = "file-secret"
"#;
    let profile = resolve(
        None,
        Some("redis://:env-secret@127.0.0.1:7001,redis://:env-secret@127.0.0.1:7002"),
        Some(toml),
    )
    .unwrap();
    let nodes = profile.cluster.unwrap().nodes;
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0], endpoint("127.0.0.1", 7001, 0, "", "env-secret"));
    assert_eq!(nodes[1].port, 7002);
    assert!(profile.single.is_none());
}

#[test]
fn only_cluster_section_leaves_single_missing() {
    let toml = r#"
[cluster]
nodes = ["127.0.0.1:7001"]
password = ""
"#;
    let profile = resolve(None, None, Some(toml)).unwrap();
    assert!(profile.single.is_none());
    assert_eq!(
        profile.cluster.unwrap().nodes[0],
        endpoint("127.0.0.1", 7001, 0, "", "")
    );
}

#[test]
fn local_toml_wins_over_home_toml() {
    let _guard = EnvGuard::set(&[("REDIS_URL", None), ("REDIS_CLUSTER_URL", None)]);
    let dir = std::env::temp_dir().join(format!("redis-me-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let local = dir.join("local.toml");
    let home = dir.join("home.toml");
    fs::write(&home, "[single]\nurl = \"redis://10.1.1.1:6379/1\"\n").unwrap();
    fs::write(&local, "[single]\nurl = \"redis://10.2.2.2:6379/2\"\n").unwrap();

    let text = toml_text_from(&local, &home).unwrap().unwrap();
    let profile = resolve(None, None, Some(&text)).unwrap();
    assert_eq!(profile.single.unwrap().host, "10.2.2.2");

    let only_home = toml_text_from(&dir.join("missing.toml"), &home)
        .unwrap()
        .unwrap();
    let profile = resolve(None, None, Some(&only_home)).unwrap();
    assert_eq!(profile.single.unwrap().host, "10.1.1.1");

    assert!(
        toml_text_from(&dir.join("missing.toml"), &dir.join("also-missing.toml"))
            .unwrap()
            .is_none()
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn process_env_is_what_load_reads() {
    let _guard = EnvGuard::set(&[
        ("REDIS_URL", Some("redis://9.9.9.9:6390/4")),
        ("REDIS_CLUSTER_URL", None),
    ]);
    let profile = redis::load().unwrap();
    let single = profile.single.unwrap();
    assert_eq!(single.host, "9.9.9.9");
    assert_eq!(single.db, 4);
}

#[test]
fn extended_section_is_optional_and_does_not_merge() {
    let text = r#"
[ssl]
url = "rediss://10.0.0.2:6380/15"
resp3 = true

[proxy_http]
url = "redis://10.0.0.3:6379/0"
host = "127.0.0.1"
port = 7890

[cluster_ssh]
nodes = ["10.0.0.4:7001"]
password = "redis-secret"
host = "10.0.0.9"
username = "root"
ssh_password = "ssh-secret"
"#;
    let cases = parse_live_conns(text).unwrap();
    assert!(cases.iter().all(|c| c.name != "ssh_pwd"));
    let ssl = cases.iter().find(|c| c.name == "ssl").unwrap();
    assert!(ssl.ssl && ssl.resp3);
    assert_eq!(ssl.endpoint.host, "10.0.0.2");
    assert_eq!(ssl.endpoint.port, 6380);
    assert_eq!(ssl.endpoint.db, 15);
    let proxy = cases.iter().find(|c| c.name == "proxy_http").unwrap();
    assert_eq!(proxy.proxy_type, "http");
    assert_eq!(proxy.proxy_host, "127.0.0.1");
    assert_eq!(proxy.proxy_port, 7890);
    assert_eq!(proxy.endpoint.host, "10.0.0.3");
    let cluster = cases.iter().find(|c| c.name == "cluster_ssh").unwrap();
    assert!(cluster.cluster && cluster.ssh);
    assert_eq!(cluster.endpoint.password, "redis-secret");
    assert_eq!(cluster.ssh_password, "ssh-secret");
    assert_eq!(cluster.login_type, "pwd");
    assert!(parse_live_conns("[ssl_mtls]\nurl = \"rediss://127.0.0.1:6380/0\"\n").is_err());
}
