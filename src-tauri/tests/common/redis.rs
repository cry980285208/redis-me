//! 本机 Redis 测试配置，以及单机/集群共用的往返断言。
//!
//! 单机、集群各自只认一个来源：环境变量优先，否则读 toml。
//! 两个 toml 不合并：`src-tauri/redis-test.local.toml` 存在就用它，否则用家目录那份。
//! 扩展连接段在后续步骤读取；这里只解析单机和集群。

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Once;
use std::time::Duration;

use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use redis_me_lib::client::me_client::MeClient;
use redis_me_lib::client::impl_cluster::MeCluster;
use redis_me_lib::client::impl_single::MeSingle;
use redis_me_lib::model::{
    ConnConfig, ConnMetaValue, FieldScanMeta, FieldScanParam, ProxyOption, RedisKey, ScanCursor,
    ScanParam, SentinelOption, SshOption, SslOption,
};
use url::Url;

/// 非 UTF-8 样例。读回时走 Base64，再解码比较。
pub const NON_UTF8: &[u8] = &[0xff, 0xfe, 0x00];

/// 从 URL 或 `host:port` 解析出的 Redis 地址。密码只来自这一处，不再另补。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub db: u16,
    pub username: String,
    pub password: String,
}

/// 集群种子。文件里的 `password` 只在该来源下填进没有 userinfo 的节点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cluster {
    pub nodes: Vec<Endpoint>,
}

/// 测哪些角色。`None` 表示没配，调用方跳过。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub single: Option<Endpoint>,
    pub cluster: Option<Cluster>,
}

/// 按默认路径和当前环境变量加载。不连接 Redis。
pub fn load() -> Result<Profile, String> {
    let text = toml_text_from(&local_toml_path(), &home_toml_path())?;
    let redis_url = env_nonempty("REDIS_URL");
    let cluster_url = env_nonempty("REDIS_CLUSTER_URL");
    resolve(
        redis_url.as_deref(),
        cluster_url.as_deref(),
        text.as_deref(),
    )
}

/// 给定已读出的环境和 toml 文本。`toml_text == None` 表示没有配置文件。
pub fn resolve(
    redis_url: Option<&str>,
    cluster_url: Option<&str>,
    toml_text: Option<&str>,
) -> Result<Profile, String> {
    let value = match toml_text {
        Some(text) => {
            toml::from_str::<toml::Value>(text).map_err(|e| format!("redis test toml: {e}"))?
        }
        None => toml::Value::Table(toml::map::Map::new()),
    };

    let single = if let Some(url) = redis_url {
        Some(parse_endpoint(url)?)
    } else {
        match value.get("single") {
            None => None,
            Some(section) => Some(parse_endpoint(section_str(section, "url", "single")?)?),
        }
    };

    let cluster = if let Some(url) = cluster_url {
        let nodes = url
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(parse_endpoint)
            .collect::<Result<Vec<_>, _>>()?;
        if nodes.is_empty() {
            return Err("REDIS_CLUSTER_URL is empty".into());
        }
        Some(Cluster { nodes })
    } else {
        match value.get("cluster") {
            None => None,
            Some(section) => Some(cluster_from_section(section)?),
        }
    };

    Ok(Profile { single, cluster })
}

/// 本地文件优先于家目录。都不存在则 `Ok(None)`。
pub fn toml_text_from(local: &Path, home: &Path) -> Result<Option<String>, String> {
    if local.is_file() {
        return read_toml(local).map(Some);
    }
    if home.is_file() {
        return read_toml(home).map(Some);
    }
    Ok(None)
}

pub fn local_toml_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("redis-test.local.toml")
}

pub fn home_toml_path() -> PathBuf {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"));
    match home {
        Some(home) => PathBuf::from(home)
            .join(".config")
            .join("redis-me")
            .join("test.toml"),
        None => PathBuf::from(".config").join("redis-me").join("test.toml"),
    }
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.is_empty())
}

fn read_toml(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))
}

fn section_str<'a>(
    section: &'a toml::Value,
    key: &str,
    section_name: &str,
) -> Result<&'a str, String> {
    section
        .get(key)
        .and_then(toml::Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("[{section_name}] {key} is required"))
}

fn cluster_from_section(section: &toml::Value) -> Result<Cluster, String> {
    let password = section
        .get("password")
        .and_then(toml::Value::as_str)
        .unwrap_or("")
        .to_string();
    let raw_nodes = section
        .get("nodes")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "[cluster] nodes is required".to_string())?;
    if raw_nodes.is_empty() {
        return Err("[cluster] nodes is empty".into());
    }
    let mut nodes = Vec::with_capacity(raw_nodes.len());
    for node in raw_nodes {
        let text = node
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "[cluster] node must be a string".to_string())?;
        let mut endpoint = parse_endpoint(text)?;
        if endpoint.password.is_empty() {
            endpoint.password = password.clone();
        }
        nodes.push(endpoint);
    }
    Ok(Cluster { nodes })
}

fn parse_endpoint(text: &str) -> Result<Endpoint, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("redis address is empty".into());
    }
    if text.contains("://") {
        return parse_url(text);
    }
    parse_host_port(text)
}

fn parse_url(text: &str) -> Result<Endpoint, String> {
    let url = Url::parse(text).map_err(|e| format!("redis url {text}: {e}"))?;
    let host = url
        .host_str()
        .ok_or_else(|| format!("redis url {text}: missing host"))?
        .to_string();
    let port = url.port().unwrap_or(6379);
    let db = url
        .path()
        .trim_start_matches('/')
        .parse::<u16>()
        .unwrap_or(0);
    Ok(Endpoint {
        host,
        port,
        db,
        username: url.username().to_string(),
        password: url.password().unwrap_or("").to_string(),
    })
}

fn parse_host_port(text: &str) -> Result<Endpoint, String> {
    let (host, port) = text
        .rsplit_once(':')
        .ok_or_else(|| format!("redis node {text}: expected host:port"))?;
    let host = host
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    if host.is_empty() {
        return Err(format!("redis node {text}: missing host"));
    }
    let port = port
        .parse::<u16>()
        .map_err(|_| format!("redis node {text}: bad port"))?;
    Ok(Endpoint {
        host,
        port,
        db: 0,
        username: String::new(),
        password: String::new(),
    })
}

fn install_rustls() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // 带 tls-rustls 的进程不装 provider，建连时会 panic
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

pub fn conn_config(endpoint: &Endpoint, cluster: bool) -> ConnConfig {
    ConnConfig {
        id: "redis-me-test".into(),
        name: "redis-me-test".into(),
        host: endpoint.host.clone(),
        port: endpoint.port,
        username: endpoint.username.clone(),
        password: endpoint.password.clone(),
        db: endpoint.db,
        cluster,
        ..ConnConfig::default()
    }
}

/// 没配单机时 `Ok(None)`。配了但建连失败是 `Err`。
pub fn single_client() -> Result<Option<Box<dyn MeClient>>, String> {
    install_rustls();
    let Some(endpoint) = load()?.single else {
        return Ok(None);
    };
    MeSingle::init(
        &conn_config(&endpoint, false),
        Duration::from_secs(5),
        Duration::from_secs(15),
    )
    .map(Some)
    .map_err(|err| err.to_string())
}

/// 没配集群时 `Ok(None)`。种子用节点列表的第一台，其余由集群自己发现。
pub fn cluster_client() -> Result<Option<Box<dyn MeClient>>, String> {
    install_rustls();
    let Some(cluster) = load()?.cluster else {
        return Ok(None);
    };
    let Some(seed) = cluster.nodes.first() else {
        return Err("cluster has no nodes".into());
    };
    MeCluster::init(
        &conn_config(seed, true),
        Duration::from_secs(15),
        Duration::from_secs(20),
    )
    .map(Some)
    .map_err(|err| err.to_string())
}

pub fn field_scan_param(key: RedisKey) -> FieldScanParam {
    FieldScanParam {
        key,
        count: 2,
        cursor: None,
        pattern: "*".into(),
        exact: false,
        meta: None,
        bytes_format: None,
        include_meta: None,
        key_type: None,
        include_field_ttl: None,
    }
}

pub fn empty_meta() -> FieldScanMeta {
    FieldScanMeta {
        max_id: String::new(),
        min_id: String::new(),
        value_byte_limit: None,
        value_preview_bytes: None,
        force_full_value: None,
        list_min_index: None,
        list_max_index: None,
        list_desc: None,
        stream_desc: None,
        vectorset_sample: None,
        zset_min_score: None,
        zset_max_score: None,
        ts_min: None,
        ts_max: None,
        ts_min_value: None,
        ts_max_value: None,
        ts_desc: None,
    }
}

pub fn b64(bytes: &[u8]) -> String {
    BASE64_STANDARD.encode(bytes)
}

pub fn decode_b64(text: &str) -> Result<Vec<u8>, String> {
    BASE64_STANDARD
        .decode(text)
        .map_err(|err| format!("base64: {err}"))
}

pub fn unknown_command(err: &dyn std::error::Error) -> bool {
    let mut current = Some(err);
    while let Some(item) = current {
        if item
            .to_string()
            .to_ascii_lowercase()
            .contains("unknown command")
        {
            return true;
        }
        current = item.source();
    }
    false
}

pub struct FieldPage {
    pub finished: bool,
    pub value: serde_json::Value,
}

/// 按返回游标把字段扫描走完。调用方自己判断第一页是否结束。
pub fn field_pages(
    client: &dyn MeClient,
    mut param: FieldScanParam,
) -> Result<Vec<FieldPage>, String> {
    let mut pages = Vec::new();
    for _ in 0..32 {
        let result = client
            .field_scan(param.clone())
            .map_err(|e| e.to_string())?;
        let finished = result.cursor.finished;
        pages.push(FieldPage {
            finished,
            value: result.value,
        });
        if finished {
            return Ok(pages);
        }
        param.cursor = Some(result.cursor);
        param.include_meta = Some(false);
        param.key_type = Some(result.key_type);
    }
    Err("field scan did not finish".into())
}

/// List / Stream / 按分数分页的 ZSet：第一页未结束，最后一页结束，顺序与写入一致。
pub fn assert_ordered_pages(
    pages: &[FieldPage],
    expected: &[String],
    pick: impl Fn(&serde_json::Value) -> Vec<String>,
) {
    assert!(
        pages.len() > 1 && !pages[0].finished,
        "first page should continue, pages={}",
        pages.len()
    );
    assert!(pages.last().is_some_and(|page| page.finished));
    let mut got = Vec::new();
    for page in pages {
        got.extend(pick(&page.value));
    }
    assert_eq!(got, expected);
}

/// 键 SCAN。COUNT 只是提示，小库可能一页就结束；拼起来必须正好是这些键，且不重复。
pub fn assert_key_scan(
    client: &dyn MeClient,
    pattern: &str,
    expected: &[Vec<u8>],
) -> Result<(), String> {
    let mut param = ScanParam {
        pattern: pattern.into(),
        scan_type: None,
        cursor: None,
        exact: false,
        count: 2,
    };
    let mut seen = BTreeSet::new();
    for _ in 0..64 {
        let result = client.scan(param.clone()).map_err(|e| e.to_string())?;
        for key in result.key_list {
            let bytes = key.to_bytes().to_vec();
            assert!(seen.insert(bytes), "duplicate key in scan");
        }
        if result.cursor.finished {
            let want: BTreeSet<Vec<u8>> = expected.iter().cloned().collect();
            assert_eq!(seen, want);
            return Ok(());
        }
        param.cursor = Some(result.cursor);
    }
    Err("key scan did not finish".into())
}

pub fn assert_exact_key(client: &dyn MeClient, pattern: &str, hits: usize) -> Result<(), String> {
    let result = client
        .scan(ScanParam {
            pattern: pattern.into(),
            scan_type: None,
            cursor: None,
            exact: true,
            count: 2,
        })
        .map_err(|e| e.to_string())?;
    assert!(result.cursor.finished);
    assert_eq!(result.key_list.len(), hits);
    if hits == 1 {
        assert_eq!(result.key_list[0].key, pattern);
    }
    Ok(())
}

/// 二进制键从 SCAN 回来时 `bytes` 必须还在，不能只剩 lossy 的 `key`。
pub fn assert_binary_key_returned(
    client: &dyn MeClient,
    pattern: &str,
    raw: &[u8],
) -> Result<(), String> {
    let mut cursor: Option<ScanCursor> = None;
    for _ in 0..64 {
        let result = client
            .scan(ScanParam {
                pattern: pattern.into(),
                scan_type: None,
                cursor: cursor.clone(),
                exact: false,
                count: 2,
            })
            .map_err(|e| e.to_string())?;
        if let Some(key) = result.key_list.iter().find(|key| key.to_bytes() == raw) {
            assert!(!key.bytes.is_empty());
            assert_eq!(key.bytes, raw);
            return Ok(());
        }
        if result.cursor.finished {
            return Err("binary key missing from scan".into());
        }
        cursor = Some(result.cursor);
    }
    Err("key scan did not finish".into())
}

/// toml 里的一段扩展连接。没有这段就不会出现在 `parse_live_conns` 的结果里。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveConn {
    pub name: String,
    pub endpoint: Endpoint,
    pub cluster: bool,
    pub ssl: bool,
    pub cert: String,
    pub tls_key: String,
    pub ca: String,
    pub ssh: bool,
    pub ssh_host: String,
    pub ssh_port: u16,
    pub login_type: String,
    pub ssh_username: String,
    pub ssh_password: String,
    pub pkfile: String,
    pub passphrase: String,
    pub proxy: bool,
    pub proxy_mode: String,
    pub proxy_type: String,
    pub proxy_host: String,
    pub proxy_port: u16,
    pub proxy_username: String,
    pub proxy_password: String,
    pub sentinel: bool,
    pub master_name: String,
    pub master_username: String,
    pub master_password: String,
    pub resp3: bool,
}

const LIVE_CONN_NAMES: &[&str] = &[
    "ssl",
    "ssl_mtls",
    "ssh_pwd",
    "ssh_key",
    "proxy_http",
    "proxy_https",
    "proxy_socks5",
    "proxy_socks5h",
    "proxy_auth",
    "proxy_system",
    "sentinel",
    "cluster_ssl",
    "cluster_ssh",
];

pub fn load_live_conns() -> Result<Vec<LiveConn>, String> {
    let Some(text) = toml_text_from(&local_toml_path(), &home_toml_path())? else {
        return Ok(Vec::new());
    };
    parse_live_conns(&text)
}

pub fn parse_live_conns(text: &str) -> Result<Vec<LiveConn>, String> {
    let value: toml::Value = toml::from_str(text).map_err(|e| format!("redis test toml: {e}"))?;
    let mut found = Vec::new();
    for name in LIVE_CONN_NAMES {
        if let Some(section) = value.get(*name) {
            found.push(parse_live_conn(name, section)?);
        }
    }
    Ok(found)
}

pub fn open_live(spec: &LiveConn) -> Result<Box<dyn MeClient>, String> {
    install_rustls();
    let conf = live_config(spec);
    let connect = Duration::from_secs(15);
    let command = Duration::from_secs(20);
    if spec.cluster {
        MeCluster::init(&conf, connect, command)
    } else {
        MeSingle::init(&conf, connect, command)
    }
    .map_err(|err| err.to_string())
}

pub fn live_config(spec: &LiveConn) -> ConnConfig {
    let mut conf = conn_config(&spec.endpoint, spec.cluster);
    conf.ssl = spec.ssl;
    conf.ssl_option = SslOption {
        key: spec.tls_key.clone(),
        cert: spec.cert.clone(),
        ca: spec.ca.clone(),
    };
    conf.ssh = spec.ssh;
    conf.ssh_option = SshOption {
        host: spec.ssh_host.clone(),
        port: spec.ssh_port,
        login_type: spec.login_type.clone(),
        username: spec.ssh_username.clone(),
        password: spec.ssh_password.clone(),
        pkfile: spec.pkfile.clone(),
        passphrase: spec.passphrase.clone(),
    };
    conf.proxy = spec.proxy;
    conf.proxy_option = ProxyOption {
        proxy_mode: spec.proxy_mode.clone(),
        proxy_type: spec.proxy_type.clone(),
        host: spec.proxy_host.clone(),
        port: spec.proxy_port,
        username: spec.proxy_username.clone(),
        password: spec.proxy_password.clone(),
    };
    conf.sentinel = spec.sentinel;
    conf.sentinel_option = SentinelOption {
        master_name: spec.master_name.clone(),
        master_username: spec.master_username.clone(),
        master_password: spec.master_password.clone(),
    };
    if spec.resp3 {
        conf.meta
            .insert("protocol".into(), ConnMetaValue::String("resp3".into()));
    }
    conf
}

fn parse_live_conn(name: &str, section: &toml::Value) -> Result<LiveConn, String> {
    let cluster = name == "cluster_ssl" || name == "cluster_ssh";
    let endpoint = if cluster {
        let nodes = cluster_from_section(section)?.nodes;
        nodes
            .into_iter()
            .next()
            .ok_or_else(|| format!("[{name}] nodes is empty"))?
    } else {
        parse_endpoint(section_str(section, "url", name)?)?
    };
    let mut spec = LiveConn {
        name: name.into(),
        endpoint,
        cluster,
        ssl: name == "ssl" || name == "ssl_mtls" || name == "cluster_ssl",
        cert: opt_str(section, "cert"),
        tls_key: opt_str(section, "key"),
        ca: opt_str(section, "ca"),
        ssh: name == "ssh_pwd" || name == "ssh_key" || name == "cluster_ssh",
        ssh_host: opt_str(section, "host"),
        ssh_port: opt_u16(section, "port").unwrap_or(22),
        login_type: opt_str(section, "login_type"),
        ssh_username: opt_str(section, "username"),
        ssh_password: opt_str(section, "password"),
        pkfile: opt_str(section, "pkfile"),
        passphrase: opt_str(section, "passphrase"),
        proxy: name.starts_with("proxy_"),
        proxy_mode: if name == "proxy_system" {
            "system".into()
        } else {
            "manual".into()
        },
        proxy_type: String::new(),
        proxy_host: opt_str(section, "host"),
        proxy_port: opt_u16(section, "port").unwrap_or(0),
        proxy_username: opt_str(section, "username"),
        proxy_password: opt_str(section, "password"),
        sentinel: name == "sentinel",
        master_name: opt_str(section, "master_name"),
        master_username: opt_str(section, "master_username"),
        master_password: opt_str(section, "master_password"),
        resp3: section
            .get("resp3")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
    };
    match name {
        "ssl_mtls" => {
            require_nonempty(name, "cert", &spec.cert)?;
            require_nonempty(name, "key", &spec.tls_key)?;
        }
        "ssh_pwd" => {
            spec.login_type = "pwd".into();
            require_nonempty(name, "host", &spec.ssh_host)?;
            require_nonempty(name, "username", &spec.ssh_username)?;
            require_nonempty(name, "password", &spec.ssh_password)?;
        }
        "ssh_key" => {
            spec.login_type = "pkfile".into();
            require_nonempty(name, "host", &spec.ssh_host)?;
            require_nonempty(name, "username", &spec.ssh_username)?;
            require_nonempty(name, "pkfile", &spec.pkfile)?;
        }
        "proxy_http" | "proxy_https" | "proxy_socks5" | "proxy_socks5h" => {
            spec.proxy_type = name.trim_start_matches("proxy_").into();
            require_nonempty(name, "host", &spec.proxy_host)?;
            if spec.proxy_port == 0 {
                return Err(format!("[{name}] port is required"));
            }
        }
        "proxy_auth" => {
            spec.proxy_type = opt_str(section, "proxy_type");
            require_nonempty(name, "proxy_type", &spec.proxy_type)?;
            require_nonempty(name, "host", &spec.proxy_host)?;
            require_nonempty(name, "username", &spec.proxy_username)?;
            require_nonempty(name, "password", &spec.proxy_password)?;
            if spec.proxy_port == 0 {
                return Err(format!("[{name}] port is required"));
            }
        }
        "proxy_system" => {}
        "sentinel" => require_nonempty(name, "master_name", &spec.master_name)?,
        "cluster_ssh" => {
            spec.ssh_password = opt_str(section, "ssh_password");
            require_nonempty(name, "host", &spec.ssh_host)?;
            require_nonempty(name, "username", &spec.ssh_username)?;
            if spec.pkfile.is_empty() {
                spec.login_type = "pwd".into();
                require_nonempty(name, "ssh_password", &spec.ssh_password)?;
            } else {
                spec.login_type = "pkfile".into();
            }
        }
        _ => {}
    }
    Ok(spec)
}

fn opt_str(section: &toml::Value, key: &str) -> String {
    section
        .get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn opt_u16(section: &toml::Value, key: &str) -> Option<u16> {
    section
        .get(key)
        .and_then(toml::Value::as_integer)
        .and_then(|n| u16::try_from(n).ok())
}

fn require_nonempty(section: &str, key: &str, value: &str) -> Result<(), String> {
    if value.is_empty() {
        Err(format!("[{section}] {key} is required"))
    } else {
        Ok(())
    }
}
