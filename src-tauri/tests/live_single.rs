//! 单机往返。地址在 `common/conn.rs` 的 `single()`。连不上或断言失败则失败。
//!
//! 测这些（具体断言在 `common/check.rs`）：
//! - 中文键和值，以及非 UTF-8 用 Base64 读回原字节
//! - 键 SCAN 翻页，多页拼起来不丢不重
//! - Hash、List、Set、ZSet、Stream 的字段翻页；List、Stream 还核对顺序
//! - JSON、TimeSeries、Array、VectorSet：服务端没有对应命令就跳过
//!
//! 新增一种基础键类型或翻页规则时，在 `check.rs` 加断言，并在这里挂上。

#[path = "common/live.rs"]
#[allow(dead_code)]
mod live;
#[path = "common/check.rs"]
#[allow(dead_code)]
mod check;

use redis_me_lib::client::me_client::MeClient;

/// `single()` 返回 `None` 才跳过。当前配置总会去连，建连失败则测试失败。
fn client() -> Option<Box<dyn MeClient>> {
    match live::single_client() {
        Ok(None) => {
            eprintln!("skip: no redis single config");
            None
        }
        Ok(Some(client)) => Some(client),
        Err(err) => panic!("redis configured but not usable: {err}"),
    }
}

/// 中文和非 UTF-8 字符串能写进再读出。
#[test]
fn chinese_and_binary_string() {
    let Some(client) = client() else { return };
    check::chinese_and_binary_string(client.as_ref());
}

/// 键扫描要翻页，五把测试键都能收到。
#[test]
fn key_scan_collects_five() {
    let Some(client) = client() else { return };
    check::key_scan_collects_five(client.as_ref());
}

/// Hash、List、Set、ZSet、Stream 的字段页能翻完，顺序类型要看到未结束的第一页。
#[test]
fn hash_list_set_zset_stream_pages() {
    let Some(client) = client() else { return };
    check::hash_list_set_zset_stream_pages(client.as_ref());
}

/// JSON、TimeSeries、Array、Vector 只在服务端有对应模块时才测。
#[test]
fn optional_modules_when_present() {
    let Some(client) = client() else { return };
    check::optional_modules_when_present(client.as_ref());
}
