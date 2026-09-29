//! 本机 Redis 测试配置。
//!
//! 单机、集群各自只认一个来源：环境变量优先，否则读 toml。
//! 两个 toml 不合并：`src-tauri/redis-test.local.toml` 存在就用它，否则用家目录那份。
//! 扩展连接段在后续步骤读取；这里只解析单机和集群。

use std::fs;
use std::path::{Path, PathBuf};

use url::Url;

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
