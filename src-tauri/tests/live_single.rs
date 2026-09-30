//! 单机往返。没配单机就跳过；配了却连不上或断言失败则失败。

#![allow(dead_code)]

#[path = "common/redis.rs"]
mod redis;
#[path = "common/roundtrip.rs"]
mod roundtrip;

use redis_me_lib::client::client_trait::MeClient;

fn client() -> Option<Box<dyn MeClient>> {
    match redis::single_client() {
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
    roundtrip::chinese_and_binary_string(client.as_ref());
}

/// 键扫描要翻页，五把测试键都能收到。
#[test]
fn key_scan_collects_five() {
    let Some(client) = client() else { return };
    roundtrip::key_scan_collects_five(client.as_ref());
}

/// Hash、List、Set、ZSet、Stream 的字段页能翻完，顺序类型要看到未结束的第一页。
#[test]
fn hash_list_set_zset_stream_pages() {
    let Some(client) = client() else { return };
    roundtrip::hash_list_set_zset_stream_pages(client.as_ref());
}

/// JSON、TimeSeries、Array、Vector 只在服务端有对应模块时才测。
#[test]
fn optional_modules_when_present() {
    let Some(client) = client() else { return };
    roundtrip::optional_modules_when_present(client.as_ref());
}
