use crate::api_model;
use crate::client::state::MeBase;
use crate::support::util::redis_value_to_string;
use redis::{ConnectionLike, Value};
use serde::{Deserialize, Serialize};
use specta::Type;

// 服务器能力（connect 时检测并返回）
api_model!(
    #[derive(Default)]
    ServerCapabilities {
        version: String,
        is_valkey: bool,
        info_supported: bool,
        acl_supported: bool,
        acl_dryrun_supported: bool,
        acl_selector_supported: bool,
        httl_supported: bool,
        /// 集群模式是否支持编号数据库（Valkey 9+）
        cluster_db_supported: bool,
        /// 当前连接能执行 FT._LIST。未装 RedisSearch 时不展示搜索页。
        redis_search_supported: bool,
    }
);

/// 检测服务器能力：优先通过 INFO SERVER 解析版本号，失败时 fallback 到 HTTL 命令探测
pub fn detect_server_capabilities(
    conn: &mut impl ConnectionLike,
    base: &mut MeBase,
    is_cluster: bool,
) {
    // Value接收是为了适配单机（返回String）和集群（返回Map）场景
    if let Ok(value) = redis::cmd("info").arg("server").query::<Value>(conn) {
        let info = redis_value_to_string(value, "\n");
        let (version, is_valkey) = parse_server_version(&info);
        base.capabilities = detect_capabilities(&version, is_valkey, is_cluster);
        log::info!("服务版本: {} (is_valkey={})", version, is_valkey);
    } else {
        log::info!("INFO SERVER 不可用，尝试 HTTL 命令探测字段级 TTL 支持");
        base.capabilities.info_supported = false;
        base.capabilities.httl_supported = detect_httl_by_command(conn);
        base.capabilities.cluster_db_supported = false;
    }
    base.capabilities.redis_search_supported = detect_redis_search(conn);
    log::info!("服务能力: {:?}", base.capabilities);
}

// ------------------------------ 仅本文件使用 ------------------------------

/// 从 INFO 输出中解析服务器版本
/// 返回 (版本号, 是否为 Valkey)
fn parse_server_version(info: &str) -> (String, bool) {
    let mut valkey_version = None;
    let mut redis_version = None;
    for line in info.lines() {
        if let Some(v) = line.strip_prefix("valkey_version:") {
            valkey_version = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("redis_version:") {
            redis_version = Some(v.trim().to_string());
        }
    }
    let is_valkey = valkey_version.is_some();
    let version = valkey_version.or(redis_version).unwrap_or_default();
    (version, is_valkey)
}

/// 根据版本号检测服务能力
fn detect_capabilities(version: &str, is_valkey: bool, is_cluster: bool) -> ServerCapabilities {
    let mut parts = version.split('.');
    let major = parts
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);
    let minor = parts
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);

    ServerCapabilities {
        version: version.to_string(),
        is_valkey,
        // INFO 命令执行成功即支持
        info_supported: true,
        // ACL 自 Redis/Valkey 6.0 起支持；用于 Info 入口与 ACL 页
        acl_supported: major >= 6,
        // ACL DRYRUN 自 Redis/Valkey 7.0 起支持
        acl_dryrun_supported: major >= 7,
        // ACL selectors 自 Redis/Valkey 7.2 起支持
        acl_selector_supported: major > 7 || (major == 7 && minor >= 2),
        // Hash 字段级 TTL 自 Redis/Valkey 7.4 起支持
        httl_supported: major > 7 || (major == 7 && minor >= 4),
        // Valkey 9+ 集群模式编号数据库（Redis OSS 集群不支持）
        cluster_db_supported: is_cluster && is_valkey && major >= 9,
        // 是否装了 RedisSearch 要发命令看，不能从版本号推断
        redis_search_supported: false,
    }
}

/// `FT._LIST` 能执行才算启用。未知命令是没装模块，其它错误同样不展示搜索页。
fn detect_redis_search(conn: &mut impl ConnectionLike) -> bool {
    match redis::cmd("FT._LIST").query::<Value>(conn) {
        Ok(_) => true,
        Err(e) => {
            log::info!("FT._LIST 不可用，搜索页不展示: {e}");
            false
        }
    }
}

/// 通过实际执行 HTTL 命令探测服务器是否支持字段级 TTL
/// 用于 INFO 命令不可用（如 ACL 限制）时的 fallback 探测
fn detect_httl_by_command(conn: &mut impl ConnectionLike) -> bool {
    let result: redis::RedisResult<Vec<i64>> = redis::cmd("HTTL")
        .arg("nonexistent_key_for_probe")
        .arg("FIELDS")
        .arg("1")
        .arg("_probe_field_")
        .query(conn);

    result.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Valkey 行优先于 redis_version；两边都没有时版本为空且不是 Valkey。
    #[test]
    fn version_prefers_valkey_line() {
        let info = "redis_version:7.2.4\nvalkey_version:8.1.0\n";
        assert_eq!(parse_server_version(info), ("8.1.0".into(), true));
        assert_eq!(
            parse_server_version("redis_version:6.2.0\n"),
            ("6.2.0".into(), false)
        );
        assert_eq!(
            parse_server_version("no version here"),
            (String::new(), false)
        );
    }

    /// ACL、DRYRUN、选择器、字段 TTL 和集群库号按主次版本打开，空版本全部关掉。
    #[test]
    fn capability_flags_follow_version_boundaries() {
        let caps = detect_capabilities("5.0.14", false, false);
        assert!(!caps.acl_supported);
        assert!(!caps.acl_dryrun_supported);
        assert!(!caps.acl_selector_supported);
        assert!(!caps.httl_supported);

        let caps = detect_capabilities("6.2.0", false, false);
        assert!(caps.acl_supported);
        assert!(!caps.acl_dryrun_supported);

        let caps = detect_capabilities("7.0.0", false, false);
        assert!(caps.acl_dryrun_supported);
        assert!(!caps.acl_selector_supported);
        assert!(!caps.httl_supported);

        let caps = detect_capabilities("7.2.5", false, false);
        assert!(caps.acl_selector_supported);
        assert!(!caps.httl_supported);

        let caps = detect_capabilities("7.4.0", false, true);
        assert!(caps.httl_supported);
        assert!(!caps.cluster_db_supported);

        let caps = detect_capabilities("9.0.0", true, true);
        assert!(caps.is_valkey);
        assert!(caps.cluster_db_supported);
        assert!(caps.info_supported);

        let caps = detect_capabilities("9.0.0", true, false);
        assert!(!caps.cluster_db_supported);

        let caps = detect_capabilities("", false, true);
        assert!(!caps.acl_supported);
        assert!(!caps.cluster_db_supported);
    }
}
