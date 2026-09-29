//! 单机往返。没配单机就跳过；配了却连不上或断言失败则失败。

#![allow(dead_code)]

#[path = "common/redis.rs"]
mod redis;
#[path = "common/roundtrip.rs"]
mod roundtrip;

use redis_me_lib::client::me_client::MeClient;

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

#[test]
fn chinese_and_binary_string() {
    let Some(client) = client() else { return };
    roundtrip::chinese_and_binary_string(client.as_ref());
}

#[test]
fn key_scan_collects_five() {
    let Some(client) = client() else { return };
    roundtrip::key_scan_collects_five(client.as_ref());
}

#[test]
fn hash_list_set_zset_stream_pages() {
    let Some(client) = client() else { return };
    roundtrip::hash_list_set_zset_stream_pages(client.as_ref());
}

#[test]
fn optional_modules_when_present() {
    let Some(client) = client() else { return };
    roundtrip::optional_modules_when_present(client.as_ref());
}
