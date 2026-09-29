//! 单机往返。没配单机就跳过；配了却连不上或断言失败则失败。
//!
//! 键前缀 `redis-me:test:`，结束时只删除这些键。

#[path = "common/redis.rs"]
mod redis;

use redis::NON_UTF8;
use redis_me_lib::client::client_trait::MeClient;
use redis_me_lib::utils::model::{BytesFormat, RedisFieldAdd, RedisFieldValue, RedisKey};
use redis_me_lib::utils::util::AnyResult;
use std::sync::atomic::{AtomicU64, Ordering};

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

fn prefix() -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    format!("redis-me:test:{}:{n}:", std::process::id())
}

struct Trash<'a> {
    client: &'a dyn MeClient,
    keys: Vec<RedisKey>,
}

impl Trash<'_> {
    fn keep(&mut self, key: RedisKey) {
        self.keys.push(key);
    }
}

impl Drop for Trash<'_> {
    fn drop(&mut self) {
        for key in self.keys.drain(..) {
            let _ = self.client.del(key);
        }
    }
}

fn field(key: &str, value: &str, score: f64) -> RedisFieldValue {
    RedisFieldValue {
        field_key: key.into(),
        field_value: value.into(),
        field_score: score,
        field_ttl: -1,
        field_attrs: String::new(),
    }
}

fn new_add(key: RedisKey, key_type: &str) -> RedisFieldAdd {
    RedisFieldAdd {
        key,
        mode: "key".into(),
        key_type: key_type.into(),
        ttl: -1,
        value: String::new(),
        list_push_method: "rpush".into(),
        array_write_method: String::new(),
        vector: Vec::new(),
        attrs: String::new(),
        field_value_list: Vec::new(),
        stream_id: "*".into(),
        key_fmt: None,
        val_fmt: None,
    }
}

fn write(client: &dyn MeClient, add: RedisFieldAdd) -> RedisKey {
    client.field_add(add).expect("field_add")
}

fn string_of(page: &serde_json::Value) -> String {
    page.as_str().unwrap_or("").to_string()
}

fn list_values(page: &serde_json::Value) -> Vec<String> {
    page.as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .map(|item| item["value"].as_str().unwrap_or("").to_string())
        .collect()
}

fn stream_values(page: &serde_json::Value) -> Vec<String> {
    page.as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .map(|item| item["value"]["v"].as_str().unwrap_or("").to_string())
        .collect()
}

fn zset_values(page: &serde_json::Value) -> Vec<String> {
    page.as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .map(|item| item["value"].as_str().unwrap_or("").to_string())
        .collect()
}

#[test]
fn chinese_and_binary_string() {
    let Some(client) = client() else { return };
    let prefix = prefix();
    let mut trash = Trash {
        client: client.as_ref(),
        keys: Vec::new(),
    };

    let chinese = format!("{prefix}键");
    let mut add = new_add(RedisKey::from(chinese.clone()), "string");
    add.value = "值".into();
    trash.keep(write(client.as_ref(), add));
    let page = redis::field_pages(
        client.as_ref(),
        redis::field_scan_param(RedisKey::from(chinese.clone())),
    )
    .unwrap();
    assert_eq!(string_of(&page[0].value), "值");

    let mut add = new_add(RedisKey::from(format!("{prefix}bin-value")), "string");
    add.value = redis::b64(NON_UTF8);
    add.val_fmt = Some(BytesFormat::Base64);
    let key = write(client.as_ref(), add);
    trash.keep(key.clone());
    let mut param = redis::field_scan_param(key);
    param.bytes_format = Some(BytesFormat::Base64);
    let page = redis::field_pages(client.as_ref(), param).unwrap();
    assert_eq!(
        redis::decode_b64(&string_of(&page[0].value)).unwrap(),
        NON_UTF8
    );

    let mut raw = prefix.as_bytes().to_vec();
    raw.extend_from_slice(NON_UTF8);
    let binary = RedisKey::from(raw.clone());
    assert!(!binary.bytes.is_empty());
    let mut add = new_add(binary, "string");
    add.value = "值".into();
    let stored = write(client.as_ref(), add);
    assert_eq!(stored.bytes, raw);
    trash.keep(stored.clone());
    let page = redis::field_pages(client.as_ref(), redis::field_scan_param(stored)).unwrap();
    assert_eq!(string_of(&page[0].value), "值");
    redis::assert_binary_key_returned(client.as_ref(), &format!("{prefix}*"), &raw).unwrap();
    redis::assert_exact_key(client.as_ref(), &chinese, 1).unwrap();
    redis::assert_exact_key(client.as_ref(), &format!("{prefix}不存在"), 0).unwrap();
}

#[test]
fn key_scan_collects_five() {
    let Some(client) = client() else { return };
    let prefix = prefix();
    let mut trash = Trash {
        client: client.as_ref(),
        keys: Vec::new(),
    };
    let mut expected = Vec::new();
    for i in 0..5 {
        let name = format!("{prefix}{i}");
        expected.push(name.as_bytes().to_vec());
        let mut add = new_add(RedisKey::from(name), "string");
        add.value = "v".into();
        trash.keep(write(client.as_ref(), add));
    }
    redis::assert_key_scan(client.as_ref(), &format!("{prefix}*"), &expected).unwrap();
}

#[test]
fn hash_list_set_zset_stream_pages() {
    let Some(client) = client() else { return };
    let prefix = prefix();
    let mut trash = Trash {
        client: client.as_ref(),
        keys: Vec::new(),
    };

    let hash_key = format!("{prefix}hash");
    let mut add = new_add(RedisKey::from(hash_key.clone()), "hash");
    add.val_fmt = Some(BytesFormat::Base64);
    add.field_value_list = (0..5)
        .map(|i| {
            field(
                &redis::b64(format!("字段{i}").as_bytes()),
                &redis::b64(format!("值{i}").as_bytes()),
                0.0,
            )
        })
        .collect();
    add.field_value_list
        .push(field(&redis::b64(NON_UTF8), &redis::b64(NON_UTF8), 0.0));
    trash.keep(write(client.as_ref(), add));
    let mut param = redis::field_scan_param(RedisKey::from(hash_key.clone()));
    param.bytes_format = Some(BytesFormat::Base64);
    let pages = redis::field_pages(client.as_ref(), param).unwrap();
    let mut pairs = Vec::new();
    for page in &pages {
        for item in page.value.as_array().unwrap() {
            let k = redis::decode_b64(item["key"].as_str().unwrap()).unwrap();
            let v = redis::decode_b64(item["value"].as_str().unwrap()).unwrap();
            pairs.push((k, v));
        }
    }
    pairs.sort();
    let mut want: Vec<(Vec<u8>, Vec<u8>)> = (0..5)
        .map(|i| {
            (
                format!("字段{i}").into_bytes(),
                format!("值{i}").into_bytes(),
            )
        })
        .collect();
    want.push((NON_UTF8.to_vec(), NON_UTF8.to_vec()));
    want.sort();
    assert_eq!(pairs, want);

    let mut exact = redis::field_scan_param(RedisKey::from(hash_key));
    exact.exact = true;
    exact.pattern = "字段0".into();
    let hit = redis::field_pages(client.as_ref(), exact).unwrap();
    assert_eq!(hit.len(), 1);
    assert_eq!(hit[0].value[0]["key"], "字段0");
    assert_eq!(hit[0].value[0]["value"], "值0");
    let mut miss = redis::field_scan_param(RedisKey::from(format!("{prefix}hash")));
    miss.exact = true;
    miss.pattern = "没有这个字段".into();
    let miss = redis::field_pages(client.as_ref(), miss).unwrap();
    assert_eq!(miss[0].value.as_array().unwrap().len(), 0);

    let list_values_expected: Vec<String> = (0..5).map(|i| format!("值{i}")).collect();
    let mut add = new_add(RedisKey::from(format!("{prefix}list")), "list");
    add.field_value_list = list_values_expected
        .iter()
        .map(|v| field("", v, 0.0))
        .collect();
    let key = write(client.as_ref(), add);
    trash.keep(key.clone());
    let pages = redis::field_pages(client.as_ref(), redis::field_scan_param(key)).unwrap();
    redis::assert_ordered_pages(&pages, &list_values_expected, list_values);

    let mut add = new_add(RedisKey::from(format!("{prefix}set")), "set");
    add.field_value_list = (0..5).map(|i| field("", &format!("值{i}"), 0.0)).collect();
    let key = write(client.as_ref(), add);
    trash.keep(key.clone());
    let pages = redis::field_pages(client.as_ref(), redis::field_scan_param(key)).unwrap();
    let mut got: Vec<String> = pages
        .iter()
        .flat_map(|page| {
            page.value
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item.as_str().unwrap().to_string())
        })
        .collect();
    got.sort();
    let mut want = list_values_expected.clone();
    want.sort();
    assert_eq!(got, want);

    let mut add = new_add(RedisKey::from(format!("{prefix}zset")), "zset");
    add.field_value_list = (0..5)
        .map(|i| field("", &format!("值{i}"), (i + 1) as f64))
        .collect();
    let key = write(client.as_ref(), add);
    trash.keep(key.clone());
    let mut param = redis::field_scan_param(key);
    let mut meta = redis::empty_meta();
    meta.zset_min_score = Some("-inf".into());
    param.meta = Some(meta);
    let pages = redis::field_pages(client.as_ref(), param).unwrap();
    redis::assert_ordered_pages(&pages, &list_values_expected, zset_values);

    let stream_key = format!("{prefix}stream");
    for (i, value) in list_values_expected.iter().enumerate() {
        let mut add = new_add(RedisKey::from(stream_key.clone()), "stream");
        if i > 0 {
            add.mode = "field".into();
        }
        add.field_value_list = vec![field("v", value, 0.0)];
        let stored = write(client.as_ref(), add);
        if i == 0 {
            trash.keep(stored);
        }
    }
    let mut param = redis::field_scan_param(RedisKey::from(stream_key));
    let mut meta = redis::empty_meta();
    meta.stream_desc = Some(false);
    param.meta = Some(meta);
    let pages = redis::field_pages(client.as_ref(), param).unwrap();
    redis::assert_ordered_pages(&pages, &list_values_expected, stream_values);
}

#[test]
fn optional_modules_when_present() {
    let Some(client) = client() else { return };
    let prefix = prefix();
    let mut trash = Trash {
        client: client.as_ref(),
        keys: Vec::new(),
    };
    optional(client.as_ref(), &mut trash, &prefix, "json", |key| {
        let mut add = new_add(key, "json");
        add.value = r#"{"字":"值"}"#.into();
        Ok(add)
    });
    optional(client.as_ref(), &mut trash, &prefix, "array", |key| {
        let mut add = new_add(key, "array");
        add.field_value_list = vec![field("0", &redis::b64("值".as_bytes()), 0.0)];
        add.field_value_list
            .push(field("1", &redis::b64(NON_UTF8), 0.0));
        add.val_fmt = Some(BytesFormat::Base64);
        Ok(add)
    });
    optional(client.as_ref(), &mut trash, &prefix, "vectorset", |key| {
        let mut add = new_add(key, "vectorset");
        add.vector = vec![1.0, 0.0];
        add.field_value_list = vec![field(&redis::b64("元素".as_bytes()), "", 0.0)];
        add.val_fmt = Some(BytesFormat::Base64);
        Ok(add)
    });
    optional(client.as_ref(), &mut trash, &prefix, "timeseries", |key| {
        let mut add = new_add(key, "timeseries");
        add.field_value_list = vec![field("1000", "1.5", 0.0)];
        Ok(add)
    });
}

fn optional(
    client: &dyn MeClient,
    trash: &mut Trash<'_>,
    prefix: &str,
    name: &str,
    build: impl FnOnce(RedisKey) -> AnyResult<RedisFieldAdd>,
) {
    let key = RedisKey::from(format!("{prefix}{name}"));
    let add = match build(key.clone()) {
        Ok(add) => add,
        Err(err) => panic!("{name}: {err}"),
    };
    let stored = match client.field_add(add) {
        Ok(stored) => stored,
        Err(err) if redis::unknown_command(&*err) => {
            eprintln!("skip: {name}: {err}");
            return;
        }
        Err(err) => {
            let _ = client.del(key);
            panic!("{name}: {err}");
        }
    };
    trash.keep(stored.clone());
    let mut param = redis::field_scan_param(stored);
    if name != "json" && name != "timeseries" {
        param.bytes_format = Some(BytesFormat::Base64);
    }
    let pages = redis::field_pages(client, param).unwrap_or_else(|err| panic!("{name}: {err}"));
    match name {
        "json" => assert_eq!(pages[0].value["字"], "值"),
        "array" => {
            let mut values: Vec<Vec<u8>> = pages
                .iter()
                .flat_map(|page| page.value.as_array().unwrap().iter())
                .map(|item| redis::decode_b64(item["value"].as_str().unwrap()).unwrap())
                .collect();
            values.sort();
            let mut want = vec!["值".as_bytes().to_vec(), NON_UTF8.to_vec()];
            want.sort();
            assert_eq!(values, want);
        }
        "vectorset" => {
            let name = pages[0].value[0]["name"].as_str().unwrap();
            assert_eq!(redis::decode_b64(name).unwrap(), "元素".as_bytes());
        }
        "timeseries" => {
            let hit = pages.iter().any(|page| {
                page.value
                    .as_array()
                    .is_some_and(|items| items.iter().any(|item| item["value"] == "1.5"))
            });
            assert!(hit, "timeseries sample missing");
        }
        _ => {}
    }
}
