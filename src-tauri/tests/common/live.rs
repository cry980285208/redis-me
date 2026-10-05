//! 活测试的建连和翻页辅助。地址在 `conn.rs`。密码从环境变量读，不写进仓库。

use std::collections::BTreeSet;
use std::sync::Once;
use std::time::Duration;

use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use redis_me_lib::client::me_client::MeClient;
use redis_me_lib::client::me_cluster::MeCluster;
use redis_me_lib::client::me_single::MeSingle;
use redis_me_lib::model::{
    ConnConfig, FieldScanMeta, FieldScanParam, RedisKey, ScanCursor, ScanParam,
};

#[path = "conn.rs"]
mod conn;

/// 非 UTF-8 样例。读回时走 Base64，再解码比较。
pub const NON_UTF8: &[u8] = &[0xff, 0xfe, 0x00];

pub struct FieldPage {
    pub finished: bool,
    pub value: serde_json::Value,
}

/// 环境变量的值。没设或读失败时是空串，表示无密码。
pub fn env_secret(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

/// `single()` 为 `None` 时 `Ok(None)`。配了但建连失败是 `Err`。
pub fn single_client() -> Result<Option<Box<dyn MeClient>>, String> {
    install_rustls();
    let Some(conf) = conn::single() else {
        return Ok(None);
    };
    MeSingle::init(&conf, Duration::from_secs(5), Duration::from_secs(15))
        .map(Some)
        .map_err(|err| err.to_string())
}

/// `cluster()` 为 `None` 时 `Ok(None)`。种子就是这份配置，其余节点由集群自己发现。
pub fn cluster_client() -> Result<Option<Box<dyn MeClient>>, String> {
    install_rustls();
    let Some(conf) = conn::cluster() else {
        return Ok(None);
    };
    MeCluster::init(&conf, Duration::from_secs(15), Duration::from_secs(20))
        .map(Some)
        .map_err(|err| err.to_string())
}

/// 字段扫描的默认参数。用例只改自己关心的那几项。
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

/// 空的字段扫描元数据，给不关心 TTL、长度的断言用。
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

/// 测试里比较二进制时用的标准 Base64。
pub fn b64(bytes: &[u8]) -> String {
    BASE64_STANDARD.encode(bytes)
}

/// 把界面上的 Base64 解回原始字节。
pub fn decode_b64(text: &str) -> Result<Vec<u8>, String> {
    BASE64_STANDARD
        .decode(text)
        .map_err(|err| format!("base64: {err}"))
}

/// 服务端没有这个模块命令时跳过，而不是判失败。
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

/// 键 SCAN 要走完整个库才能确定没有漏键。COUNT 用 1000，和界面默认批量一致；
/// 用 2 时，库里已有几百个键，64 轮扫不完。
const KEY_SCAN_COUNT: u64 = 1000;

/// 键 SCAN。拼起来必须正好是这些键，且不重复。小库可能一页就结束。
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
        count: KEY_SCAN_COUNT,
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

/// `exact: true` 时只命中这一条；`hits` 为 0 表示不存在。
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
                count: KEY_SCAN_COUNT,
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

/// `conn.rs` 里列出的扩展连接。测试名不在里面就跳过。
pub fn live_conns() -> Vec<ConnConfig> {
    conn::live_conns()
}

/// 按一份 `ConnConfig` 建连。集群走集群客户端。
pub fn open_live(conf: &ConnConfig) -> Result<Box<dyn MeClient>, String> {
    install_rustls();
    let connect = Duration::from_secs(15);
    let command = Duration::from_secs(20);
    if conf.cluster {
        MeCluster::init(conf, connect, command)
    } else {
        MeSingle::init(conf, connect, command)
    }
    .map_err(|err| err.to_string())
}

// ------------------------------ 仅本文件使用 ------------------------------

/// 进程里只装一次 rustls provider。不装的话 TLS 测试会 panic。
fn install_rustls() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // 带 tls-rustls 的进程不装 provider，建连时会 panic
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}
