//! 集群往返。没有集群配置就跳过。

#![allow(dead_code)]

#[path = "common/redis.rs"]
mod redis;
#[path = "common/roundtrip.rs"]
mod roundtrip;

use redis_me_lib::client::client_trait::MeClient;

/// 没配集群就跳过。配了却建连失败则测试失败。
fn client() -> Option<Box<dyn MeClient>> {
    match redis::cluster_client() {
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
    roundtrip::chinese_and_binary_string(client.as_ref());
}

/// 集群键扫描要翻页，五把测试键都能收到。
#[test]
fn key_scan_collects_five() {
    let Some(client) = client() else { return };
    roundtrip::key_scan_collects_five(client.as_ref());
}

/// 集群上 Hash、List、Set、ZSet、Stream 的字段页能翻完。
#[test]
fn hash_list_set_zset_stream_pages() {
    let Some(client) = client() else { return };
    roundtrip::hash_list_set_zset_stream_pages(client.as_ref());
}

/// 集群上可选模块只在命令存在时才测。
#[test]
fn optional_modules_when_present() {
    let Some(client) = client() else { return };
    roundtrip::optional_modules_when_present(client.as_ref());
}

/// 同槽 RENAME 用原生命令；跨槽改成 DUMP、RESTORE 再删旧键。
#[test]
fn slot_and_cross_slot_rename() {
    let Some(client) = client() else { return };
    roundtrip::cluster_slot_and_rename(client.as_ref());
}
