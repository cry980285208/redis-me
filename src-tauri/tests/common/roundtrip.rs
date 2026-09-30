//! 单机和集群共用的往返。调用方负责建连；这里只写键、断言、删除。

use crate::redis;
use redis_me_lib::client::me_client::MeClient;
use redis_me_lib::model::{BytesFormat, RedisFieldAdd, RedisFieldValue, RedisKey};
use redis_me_lib::support::util::AnyResult;
use std::sync::atomic::{AtomicU64, Ordering};

struct Trash<'a> {
    client: &'a dyn MeClient,
    keys: Vec<RedisKey>,
}

impl Trash<'_> {
    /// 记下要在结束时删掉的键。
    fn keep(&mut self, key: RedisKey) {
        self.keys.push(key);
    }
}

impl Drop for Trash<'_> {
    /// 只删除这个用例写下的键。
    fn drop(&mut self) {
        for key in self.keys.drain(..) {
            let _ = self.client.del(key);
        }
    }
}

/// 扩展连接用：一条中文 String，一段非 UTF-8 用 Base64 读回。
pub fn string_roundtrip(client: &dyn MeClient) {
    let prefix = prefix();
    let mut trash = Trash {
        client,
        keys: Vec::new(),
    };
    let chinese = format!("{prefix}键");
    let mut add = new_add(RedisKey::from(chinese), "string");
    add.value = "值".into();
    let key = write(client, add);
    trash.keep(key.clone());
    let page = redis::field_pages(client, redis::field_scan_param(key)).unwrap();
    assert_eq!(string_of(&page[0].value), "值");

    let raw = [0xff, 0xfe];
    let mut add = new_add(RedisKey::from(format!("{prefix}bin")), "string");
    add.value = redis::b64(&raw);
    add.val_fmt = Some(BytesFormat::Base64);
    let key = write(client, add);
    trash.keep(key.clone());
    let mut param = redis::field_scan_param(key);
    param.bytes_format = Some(BytesFormat::Base64);
    let page = redis::field_pages(client, param).unwrap();
    assert_eq!(redis::decode_b64(&string_of(&page[0].value)).unwrap(), raw);
}

/// 键名、String、Hash、List 都含中文；二进制键用 Base64 读回原字节。
pub fn chinese_and_binary_string(client: &dyn MeClient) {
    let prefix = prefix();
    let mut trash = Trash {
        client,
        keys: Vec::new(),
    };

    let chinese = format!("{prefix}键");
    let mut add = new_add(RedisKey::from(chinese.clone()), "string");
    add.value = "值".into();
    trash.keep(write(client, add));
    let page = redis::field_pages(
        client,
        redis::field_scan_param(RedisKey::from(chinese.clone())),
    )
    .unwrap();
    assert_eq!(string_of(&page[0].value), "值");

    let mut add = new_add(RedisKey::from(format!("{prefix}bin-value")), "string");
    add.value = redis::b64(redis::NON_UTF8);
    add.val_fmt = Some(BytesFormat::Base64);
    let key = write(client, add);
    trash.keep(key.clone());
    let mut param = redis::field_scan_param(key);
    param.bytes_format = Some(BytesFormat::Base64);
    let page = redis::field_pages(client, param).unwrap();
    assert_eq!(
        redis::decode_b64(&string_of(&page[0].value)).unwrap(),
        redis::NON_UTF8
    );

    let mut raw = prefix.as_bytes().to_vec();
    raw.extend_from_slice(redis::NON_UTF8);
    let binary = RedisKey::from(raw.clone());
    assert!(!binary.bytes.is_empty());
    let mut add = new_add(binary, "string");
    add.value = "值".into();
    let stored = write(client, add);
    assert_eq!(stored.bytes, raw);
    trash.keep(stored.clone());
    let page = redis::field_pages(client, redis::field_scan_param(stored)).unwrap();
    assert_eq!(string_of(&page[0].value), "值");
    redis::assert_binary_key_returned(client, &format!("{prefix}*"), &raw).unwrap();
    redis::assert_exact_key(client, &chinese, 1).unwrap();
    redis::assert_exact_key(client, &format!("{prefix}不存在"), 0).unwrap();
}

/// 同一前缀写 5 个键，`count = 2`，翻页拼起来正好是这 5 个。
pub fn key_scan_collects_five(client: &dyn MeClient) {
    let prefix = prefix();
    let mut trash = Trash {
        client,
        keys: Vec::new(),
    };
    let mut expected = Vec::new();
    for i in 0..5 {
        let name = format!("{prefix}{i}");
        expected.push(name.as_bytes().to_vec());
        let mut add = new_add(RedisKey::from(name), "string");
        add.value = "v".into();
        trash.keep(write(client, add));
    }
    redis::assert_key_scan(client, &format!("{prefix}*"), &expected).unwrap();
}

/// Hash / List / Set / ZSet / Stream 各 5 个元素，按返回游标翻页，List 和 Stream 还核对顺序。
pub fn hash_list_set_zset_stream_pages(client: &dyn MeClient) {
    let prefix = prefix();
    let mut trash = Trash {
        client,
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
    add.field_value_list.push(field(
        &redis::b64(redis::NON_UTF8),
        &redis::b64(redis::NON_UTF8),
        0.0,
    ));
    trash.keep(write(client, add));
    let mut param = redis::field_scan_param(RedisKey::from(hash_key.clone()));
    param.bytes_format = Some(BytesFormat::Base64);
    let pages = redis::field_pages(client, param).unwrap();
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
    want.push((redis::NON_UTF8.to_vec(), redis::NON_UTF8.to_vec()));
    want.sort();
    assert_eq!(pairs, want);

    let mut exact = redis::field_scan_param(RedisKey::from(hash_key));
    exact.exact = true;
    exact.pattern = "字段0".into();
    let hit = redis::field_pages(client, exact).unwrap();
    assert_eq!(hit.len(), 1);
    assert_eq!(hit[0].value[0]["key"], "字段0");
    assert_eq!(hit[0].value[0]["value"], "值0");
    let mut miss = redis::field_scan_param(RedisKey::from(format!("{prefix}hash")));
    miss.exact = true;
    miss.pattern = "没有这个字段".into();
    let miss = redis::field_pages(client, miss).unwrap();
    assert_eq!(miss[0].value.as_array().unwrap().len(), 0);

    let list_values_expected: Vec<String> = (0..5).map(|i| format!("值{i}")).collect();
    let mut add = new_add(RedisKey::from(format!("{prefix}list")), "list");
    add.field_value_list = list_values_expected
        .iter()
        .map(|v| field("", v, 0.0))
        .collect();
    let key = write(client, add);
    trash.keep(key.clone());
    let pages = redis::field_pages(client, redis::field_scan_param(key)).unwrap();
    redis::assert_ordered_pages(&pages, &list_values_expected, list_values);

    let mut add = new_add(RedisKey::from(format!("{prefix}set")), "set");
    add.field_value_list = (0..5).map(|i| field("", &format!("值{i}"), 0.0)).collect();
    let key = write(client, add);
    trash.keep(key.clone());
    let pages = redis::field_pages(client, redis::field_scan_param(key)).unwrap();
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
    let mut want_set = list_values_expected.clone();
    want_set.sort();
    assert_eq!(got, want_set);

    let mut add = new_add(RedisKey::from(format!("{prefix}zset")), "zset");
    add.field_value_list = (0..5)
        .map(|i| field("", &format!("值{i}"), (i + 1) as f64))
        .collect();
    let key = write(client, add);
    trash.keep(key.clone());
    let mut param = redis::field_scan_param(key);
    let mut meta = redis::empty_meta();
    meta.zset_min_score = Some("-inf".into());
    param.meta = Some(meta);
    let pages = redis::field_pages(client, param).unwrap();
    redis::assert_ordered_pages(&pages, &list_values_expected, zset_values);

    let stream_key = format!("{prefix}stream");
    for (i, value) in list_values_expected.iter().enumerate() {
        let mut add = new_add(RedisKey::from(stream_key.clone()), "stream");
        if i > 0 {
            add.mode = "field".into();
        }
        add.field_value_list = vec![field("v", value, 0.0)];
        let stored = write(client, add);
        if i == 0 {
            trash.keep(stored);
        }
    }
    let mut param = redis::field_scan_param(RedisKey::from(stream_key));
    let mut meta = redis::empty_meta();
    meta.stream_desc = Some(false);
    param.meta = Some(meta);
    let pages = redis::field_pages(client, param).unwrap();
    redis::assert_ordered_pages(&pages, &list_values_expected, stream_values);
}

/// JSON、Array、VectorSet、TimeSeries。没有对应命令就跳过。
pub fn optional_modules_when_present(client: &dyn MeClient) {
    let prefix = prefix();
    let mut trash = Trash {
        client,
        keys: Vec::new(),
    };
    optional(client, &mut trash, &prefix, "json", |key| {
        let mut add = new_add(key, "json");
        add.value = r#"{"字":"值"}"#.into();
        Ok(add)
    });
    optional(client, &mut trash, &prefix, "array", |key| {
        let mut add = new_add(key, "array");
        add.field_value_list = vec![field("0", &redis::b64("值".as_bytes()), 0.0)];
        add.field_value_list
            .push(field("1", &redis::b64(redis::NON_UTF8), 0.0));
        add.val_fmt = Some(BytesFormat::Base64);
        Ok(add)
    });
    optional(client, &mut trash, &prefix, "vectorset", |key| {
        let mut add = new_add(key, "vectorset");
        add.vector = vec![1.0, 0.0];
        add.field_value_list = vec![field(&redis::b64("元素".as_bytes()), "", 0.0)];
        add.val_fmt = Some(BytesFormat::Base64);
        Ok(add)
    });
    optional(client, &mut trash, &prefix, "timeseries", |key| {
        let mut add = new_add(key, "timeseries");
        add.field_value_list = vec![field("1000", "1.5", 0.0)];
        Ok(add)
    });
}

/// hash tag 把两个键钉在不同 slot，再跨 slot 重命名。
pub fn cluster_slot_and_rename(client: &dyn MeClient) {
    let prefix = prefix();
    let key_a = format!("{{a}}{prefix}from");
    let key_b = format!("{{b}}{prefix}to");
    let slot_a = client
        .key_slot(RedisKey::from(key_a.as_str()))
        .expect("key_slot");
    let slot_b = client
        .key_slot(RedisKey::from(key_b.as_str()))
        .expect("key_slot");
    assert_ne!(
        slot_a, slot_b,
        "hash tags {{a}} and {{b}} landed in the same slot"
    );
    let nodes = client
        .key_node(RedisKey::from(key_a.as_str()))
        .expect("key_node");
    assert!(!nodes.is_empty());

    let mut trash = Trash {
        client,
        keys: Vec::new(),
    };
    let mut add = new_add(RedisKey::from(key_a.as_str()), "string");
    add.value = "值".into();
    trash.keep(write(client, add));
    let renamed = client
        .rename(
            RedisKey::from(key_a.as_str()),
            RedisKey::from(key_b.as_str()),
        )
        .expect("cross-slot rename");
    trash.keep(renamed.clone());
    let page = redis::field_pages(client, redis::field_scan_param(renamed)).unwrap();
    assert_eq!(string_of(&page[0].value), "值");
}

// ------------------------------ 仅本文件使用 ------------------------------

/// 每个用例一把键前缀，避免 `cargo test` 并行时互相删键。
fn prefix() -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    format!("redis-me:test:{}:{n}:", std::process::id())
}

/// 一条字段。TTL 用 -1，表示不单独设置字段过期。
fn field(key: &str, value: &str, score: f64) -> RedisFieldValue {
    RedisFieldValue {
        field_key: key.into(),
        field_value: value.into(),
        field_score: score,
        field_ttl: -1,
        field_attrs: String::new(),
    }
}

/// 新建键的参数骨架。调用方再填值和字段。
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

/// 写入并返回键。失败直接让测试挂掉。
fn write(client: &dyn MeClient, add: RedisFieldAdd) -> RedisKey {
    client.field_add(add).expect("field_add")
}

/// String 页的值。不是字符串就当成空，断言会失败。
fn string_of(page: &serde_json::Value) -> String {
    page.as_str().unwrap_or("").to_string()
}

/// List 一页里的元素文本，按返回顺序。
fn list_values(page: &serde_json::Value) -> Vec<String> {
    page.as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .map(|item| item["value"].as_str().unwrap_or("").to_string())
        .collect()
}

/// Stream 一页里字段 `v` 的文本，用来核对顺序。
fn stream_values(page: &serde_json::Value) -> Vec<String> {
    page.as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .map(|item| item["value"]["v"].as_str().unwrap_or("").to_string())
        .collect()
}

/// ZSet 一页里的成员文本。
fn zset_values(page: &serde_json::Value) -> Vec<String> {
    page.as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .map(|item| item["value"].as_str().unwrap_or("").to_string())
        .collect()
}

/// 可选模块：unknown command 就跳过，能创建再做中文和非 UTF-8 往返。
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
            let mut want = vec!["值".as_bytes().to_vec(), redis::NON_UTF8.to_vec()];
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
