//! 集群往返。地址在 `common/conn.rs` 的 `cluster()`。连不上或断言失败则失败。
//!
//! 和单机同一套：中文与非 UTF-8、键扫描翻页、Hash / List / Set / ZSet / Stream 字段翻页，
//! 以及有模块才测的 JSON、TimeSeries、Array、VectorSet。
//! 集群另测：槽位能查到，跨 slot 的 RENAME 用 DUMP、RESTORE 再删旧键。
//!
//! 新增集群特有行为时加在本文件；单机和集群共用的往返加在 `common/check.rs`。

#[path = "common/check.rs"]
#[allow(dead_code)]
mod check;
#[path = "common/live.rs"]
#[allow(dead_code)]
mod live;

use redis_me_lib::client::me_client::MeClient;

/// `cluster()` 返回 `None` 才跳过。当前配置总会去连，建连失败则测试失败。
fn client() -> Option<Box<dyn MeClient>> {
    match live::cluster_client() {
        Ok(None) => {
            eprintln!("skip: no redis cluster config");
            None
        }
        Ok(Some(client)) => Some(client),
        Err(err) => panic!("redis cluster configured but not usable: {err}"),
    }
}

/// 集群上中文和非 UTF-8 字符串能写进再读出。
#[test]
fn chinese_and_binary_string() {
    let Some(client) = client() else { return };
    check::chinese_and_binary_string(client.as_ref());
}

/// 集群键扫描要翻页，五把测试键都能收到。
#[test]
fn key_scan_collects_five() {
    let Some(client) = client() else { return };
    check::key_scan_collects_five(client.as_ref());
}

/// 集群上 Hash、List、Set、ZSet、Stream 的字段页能翻完。
#[test]
fn hash_list_set_zset_stream_pages() {
    let Some(client) = client() else { return };
    check::hash_list_set_zset_stream_pages(client.as_ref());
}

/// 集群上可选模块只在命令存在时才测。
#[test]
fn optional_modules_when_present() {
    let Some(client) = client() else { return };
    check::optional_modules_when_present(client.as_ref());
}

/// 同槽 RENAME 用原生命令；跨槽改成 DUMP、RESTORE 再删旧键。
#[test]
fn slot_and_cross_slot_rename() {
    let Some(client) = client() else { return };
    check::cluster_slot_and_rename(client.as_ref());
}
