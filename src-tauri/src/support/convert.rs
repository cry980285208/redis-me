//! Redis 回复转成某一种界面行。不发命令。
//!
//! 函数要认识 `ValueType`、某种扫描回复，或产出 `RedisHashItem` 这类行，就放这里。
//! 字节和 wire 字符串、路径、随机数、超时仍在 `util`。

use crate::model::*;
use crate::support::error::AppError;
use crate::support::util::{
    AnyResult, ME_JSON_TYPE_NAME, ME_TIMESERIES_TYPE_NAME, REDIS_JSON_TYPE_NAME,
    REDIS_TIMESERIES_TYPE_NAME, format_bytes, redis_value_to_bulk_bytes, redis_value_to_string,
};
use anyhow::bail;
use log::info;
use redis::streams::{StreamId, StreamInfoConsumer, StreamInfoGroup, StreamRangeReply};
use redis::{FromRedisValue, Value, ValueType};
use std::collections::{HashMap, HashSet};

type XRangeField = (Vec<u8>, Vec<u8>);
type XRangeEntry = (Vec<u8>, Vec<XRangeField>);

/// `ValueType` 转成界面上的小写类型名（json / timeseries / array 等）。
pub fn ui_key_type(key_type: ValueType) -> String {
    let key_type: String = key_type.into();
    ui_key_type_str(&key_type)
}

/// 界面类型名或 `TYPE` 回复转回 `ValueType`。认不出的名字原样放进 `Unknown`。
pub fn to_key_type(key_type: &str) -> ValueType {
    match key_type {
        ME_JSON_TYPE_NAME => ValueType::JSON,
        // 前端 KEY_TYPE_LIST / SCAN 用 timeseries；TYPE 原始名也可经 From 落到 TimeSeries
        s if s.eq_ignore_ascii_case(ME_TIMESERIES_TYPE_NAME) => ValueType::TimeSeries,
        // 规范化为小写 "array"，与 TYPE 回复一致，便于 is_array_type 识别
        s if s.eq_ignore_ascii_case("array") => ValueType::Unknown("array".into()),
        _ => key_type.into(),
    }
}

/// TimeSeries 倒序续页：上一页最小 timestamp 减 1（`TS.REVRANGE` 下一页 `to`）。
/// 用十进制字符串解析为 i128，避免 JS Number / i64 边界问题；非法或 ≤0 时回 `"0"`。
pub fn ts_timestamp_dec_one(ts: &str) -> String {
    let s = ts.trim();
    if s.is_empty() {
        return "0".into();
    }
    match s.parse::<i128>() {
        Ok(n) if n > 0 => (n - 1).to_string(),
        Ok(_) => "0".into(),
        Err(_) => "0".into(),
    }
}

/// TimeSeries 正序续页：上一页最大 timestamp 加 1（`TS.RANGE` 下一页 `from`）。
pub fn ts_timestamp_inc_one(ts: &str) -> String {
    let s = ts.trim();
    if s.is_empty() {
        return "0".into();
    }
    match s.parse::<i128>() {
        Ok(n) => n.saturating_add(1).to_string(),
        Err(_) => "0".into(),
    }
}

/// Redis 8.8 Array 类型判断。
///
/// redis-rs 当前尚无正式 `ValueType::Array`，`TYPE` / `into()` 会落到
/// `ValueType::Unknown("array")`。所有 Array 读写分支必须经此函数，勿散落匹配字符串。
///
/// **升级注意**：若日后 redis-rs 增加 `ValueType::Array`（或 Unknown 字符串变化），
/// 必须同步扩展本函数，否则会落入 `KeyTypeUnknown` / 静默走错分支。
pub fn is_array_type(t: &ValueType) -> bool {
    matches!(t, ValueType::Unknown(s) if s.eq_ignore_ascii_case("array"))
}

/// 解析 Array 索引（十进制非负）；供 ARSET/ARGET/ARDEL 等使用
pub fn parse_array_index(s: &str) -> AnyResult<i64> {
    let idx = s
        .trim()
        .parse::<i64>()
        .map_err(|_| anyhow::anyhow!("invalid array index: {}", s))?;
    if idx < 0 {
        bail!("invalid array index: {}", s);
    }
    Ok(idx)
}

/// `XINFO GROUPS` 的 redis-rs 结构转成 IPC 的 `XInfoGroup`。
pub fn ui_xinfo_group(group: StreamInfoGroup) -> XInfoGroup {
    XInfoGroup {
        name: group.name,
        consumers: group.consumers,
        pending: group.pending,
        last_delivered_id: group.last_delivered_id,
        entries_read: group.entries_read,
        lag: group.lag,
    }
}

/// `XINFO CONSUMERS` 的 redis-rs 结构转成 IPC 的 `XInfoConsumer`。
pub fn ui_xinfo_consumer(consumer: StreamInfoConsumer) -> XInfoConsumer {
    XInfoConsumer {
        name: consumer.name,
        pending: consumer.pending,
        idle: consumer.idle,
    }
}

// 辅助函数
pub fn tuple_to_key_size(keys: Vec<(Vec<u8>, u64, String)>) -> Vec<RedisKeySize> {
    let mut key_list: Vec<RedisKeySize> = keys
        .into_iter()
        .map(|(key, size, key_type)| RedisKeySize::from((key, size, ui_key_type_str(&key_type))))
        .collect();
    key_list.sort_by_key(|x| x.size);
    key_list.reverse();
    key_list
}

/// 扫描得到的键字节转成 `RedisKey`。合法 UTF-8 不重复带 `bytes`。
pub fn ui_key_list(keys: Vec<Vec<u8>>) -> Vec<RedisKey> {
    // UTF-8 键省略 bytes，见 RedisKey::from(Vec<u8>)
    keys.into_iter().map(RedisKey::from).collect()
}

/// List 一页转成带起始下标的 `{index, value}`。
pub fn ui_list_items(
    start_index: i64,
    value: &[Vec<u8>],
    format: &BytesFormat,
) -> Vec<crate::model::RedisListItem> {
    value
        .iter()
        .enumerate()
        .map(|(i, v)| crate::model::RedisListItem {
            index: start_index + i as i64,
            value: format_bytes(v, format),
        })
        .collect()
}

/// 解析 ARSCAN 一对索引+原始 bytes。
/// Redis 8.8 实际为嵌套 `[[idx, val], ...]`（见命令示例）；亦兼容扁平 `[idx, val, ...]`。
pub fn parse_arscan_pairs(raw: Value) -> AnyResult<Vec<(i64, Vec<u8>)>> {
    let arr = match raw {
        Value::Nil => return Ok(Vec::new()),
        Value::Array(a) => a,
        other => bail!(AppError::Internal {
            message: format!("unexpected ARSCAN reply: {:?}", other)
        }),
    };
    if arr.is_empty() {
        return Ok(Vec::new());
    }

    let parse_one = |idx_v: &Value, val_v: &Value| -> AnyResult<(i64, Vec<u8>)> {
        let idx: i64 = FromRedisValue::from_redis_value_ref(idx_v)
            .map_err(|e| anyhow::anyhow!("ARSCAN index parse: {}", e))?;
        let val_bytes: Vec<u8> = FromRedisValue::from_redis_value_ref(val_v)
            .map_err(|e| anyhow::anyhow!("ARSCAN value parse: {}", e))?;
        Ok((idx, val_bytes))
    };

    // 嵌套：每个元素是 [idx, val]
    if matches!(arr.first(), Some(Value::Array(_))) {
        let mut pairs = Vec::with_capacity(arr.len());
        for entry in &arr {
            match entry {
                Value::Array(pair) if pair.len() >= 2 => {
                    pairs.push(parse_one(&pair[0], &pair[1])?);
                }
                other => bail!(AppError::Internal {
                    message: format!("unexpected ARSCAN pair: {:?}", other)
                }),
            }
        }
        return Ok(pairs);
    }

    // 扁平：idx1, val1, idx2, val2, ...
    let mut pairs = Vec::with_capacity(arr.len() / 2);
    let mut i = 0;
    while i + 1 < arr.len() {
        pairs.push(parse_one(&arr[i], &arr[i + 1])?);
        i += 2;
    }
    Ok(pairs)
}

/// ARSCAN → List 同行形状 `{index, value}`（IPC 展示用）
pub fn ui_array_items_from_arscan(
    raw: Value,
    format: &BytesFormat,
) -> AnyResult<Vec<crate::model::RedisListItem>> {
    Ok(parse_arscan_pairs(raw)?
        .into_iter()
        .map(|(index, bytes)| crate::model::RedisListItem {
            index,
            value: format_bytes(&bytes, format),
        })
        .collect())
}

/// Hash 字段对转成界面行。这里不填字段 TTL。
pub fn ui_hash_value(value: &[(Vec<u8>, Vec<u8>)], format: &BytesFormat) -> Vec<RedisHashItem> {
    value
        .iter()
        .map(|(key, value)| {
            let key: String = format_bytes(key, format);
            let value: String = format_bytes(value, format);
            RedisHashItem {
                key,
                value,
                ttl: None,
            }
        })
        .collect()
}

/// Set 成员转成界面字符串。顺序不稳定，和 Redis `SMEMBERS` 一样。
pub fn ui_set_value(value: HashSet<Vec<u8>>, format: &BytesFormat) -> Vec<String> {
    value
        .into_iter()
        .map(|v| format_bytes(&v, format))
        .collect()
}

/// ZSet 成员和分数转成界面行，顺序保持 Redis 返回的顺序。
pub fn ui_zset_value(value: Vec<(Vec<u8>, f64)>, format: &BytesFormat) -> Vec<RedisZetItem> {
    value
        .into_iter()
        .map(|(value, score)| RedisZetItem {
            value: format_bytes(&value, format),
            score,
        })
        .collect()
}

/// Stream 区间回复转成 `{id, value}`。字段值用换行拼成一段文本。
pub fn ui_stream_value(reply: StreamRangeReply) -> Vec<RedisStreamItem> {
    reply
        .ids
        .into_iter()
        .map(|sid| {
            let StreamId { id, map, .. } = sid;
            RedisStreamItem {
                id,
                value: ui_stream_id(map),
            }
        })
        .collect()
}

/// `XRANGE` 原始数组回复 → 保序 entry（id + field-value 对），避免 `HashMap` 打乱顺序
pub fn parse_xrange_ordered(raw: Value) -> AnyResult<Vec<XRangeEntry>> {
    let entries = match raw {
        Value::Array(arr) => arr,
        _ => bail!(AppError::Internal {
            message: "XRANGE expected array".into()
        }),
    };
    let mut result = Vec::with_capacity(entries.len());
    for entry in entries {
        let parts = match entry {
            Value::Array(p) if p.len() >= 2 => p,
            _ => continue,
        };
        let id = redis_value_to_bulk_bytes(parts[0].clone());
        // 字段列表：实测 RESP3 下 Redis 核心命令仍返回扁平数组 [f1, v1, ...]；
        // Map 分支为防御性兼容（RESP3 规范允许 Map 回复，其他服务端实现可能返回 Map）
        let fields: Vec<(Vec<u8>, Vec<u8>)> = match &parts[1] {
            Value::Array(f) => f
                .chunks(2)
                .filter(|chunk| chunk.len() == 2)
                .map(|chunk| {
                    (
                        redis_value_to_bulk_bytes(chunk[0].clone()),
                        redis_value_to_bulk_bytes(chunk[1].clone()),
                    )
                })
                .collect(),
            Value::Map(m) => {
                // RESP3 Map 形态确认点：日志出现即说明兼容分支生效
                info!("RESP3: XRANGE 字段列表以 Map 返回，按 Map 保序解析");
                m.iter()
                    .map(|(k, v)| {
                        (
                            redis_value_to_bulk_bytes(k.clone()),
                            redis_value_to_bulk_bytes(v.clone()),
                        )
                    })
                    .collect()
            }
            _ => continue,
        };
        result.push((id, fields));
    }
    Ok(result)
}

/// 解析 `TS.REVRANGE` / `TS.RANGE` 回复为样本行（`[[ts, value], ...]`）
pub fn parse_ts_range_items(raw: Value) -> AnyResult<Vec<RedisTimeSeriesItem>> {
    match raw {
        Value::Nil => Ok(Vec::new()),
        Value::Array(rows) => {
            let mut out = Vec::with_capacity(rows.len());
            for row in rows {
                match row {
                    Value::Array(pair) if pair.len() >= 2 => {
                        out.push(RedisTimeSeriesItem {
                            key: redis_scalar_to_plain(&pair[0]),
                            value: redis_scalar_to_plain(&pair[1]),
                        });
                    }
                    other => bail!("unexpected TS.RANGE sample: {:?}", other),
                }
            }
            Ok(out)
        }
        other => bail!("unexpected TS.RANGE reply: {:?}", other),
    }
}

/// 从 `TS.INFO` 扁平键值中取 `totalSamples`；解析失败返回 None
pub fn ts_info_total_samples(raw: &Value) -> Option<u64> {
    let pairs: Vec<(String, String)> = match raw {
        Value::Array(arr) => {
            let mut pairs = Vec::new();
            let mut i = 0;
            while i + 1 < arr.len() {
                pairs.push((
                    redis_scalar_to_plain(&arr[i]),
                    redis_scalar_to_plain(&arr[i + 1]),
                ));
                i += 2;
            }
            pairs
        }
        Value::Map(map) => map
            .iter()
            .map(|(k, v)| (redis_scalar_to_plain(k), redis_scalar_to_plain(v)))
            .collect(),
        _ => return None,
    };
    pairs
        .into_iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("totalSamples"))
        .and_then(|(_, v)| v.parse::<u64>().ok())
}

// ------------------------------ 仅本文件使用 ------------------------------

/// `TYPE` 等返回的原始类型名（含模块名如 ReJSON-RL / TSDB-TYPE）统一为与 `ui_key_type` 一致的展示名
fn ui_key_type_str(key_type: &str) -> String {
    if key_type == REDIS_JSON_TYPE_NAME {
        ME_JSON_TYPE_NAME.to_string()
    } else if key_type == REDIS_TIMESERIES_TYPE_NAME {
        ME_TIMESERIES_TYPE_NAME.to_string()
    } else {
        key_type.to_string()
    }
}

/// 一条 Stream 消息的字段表转成字符串。嵌套值用换行展开。
fn ui_stream_id(stream_id: HashMap<String, Value>) -> HashMap<String, String> {
    stream_id
        .into_iter()
        .map(|(k, v)| (k, redis_value_to_string(v, "\n")))
        .collect()
}

/// 将 Redis Value 标量为十进制或明文字符串。TimeSeries 的时间戳和值用这个。
fn redis_scalar_to_plain(v: &Value) -> String {
    redis_value_to_string(v.clone(), "")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 键列表里能当文本的项同样省略 bytes。
    #[test]
    fn test_ui_key_list_omits_utf8_bytes() {
        let list = ui_key_list(vec![b"a".to_vec(), vec![0xff]]);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].key, "a");
        assert!(list[0].bytes.is_empty());
        assert_eq!(list[1].bytes, vec![0xff]);
    }

    /// XRANGE 回复按条目顺序收成 id 和字段。
    #[test]
    fn test_parse_xrange_ordered() {
        let raw = Value::Array(vec![Value::Array(vec![
            Value::SimpleString("1-0".into()),
            Value::Array(vec![
                Value::SimpleString("f2".into()),
                Value::BulkString(b"v2".to_vec()),
                Value::SimpleString("f1".into()),
                Value::BulkString(b"v1".to_vec()),
            ]),
        ])]);
        let entries = parse_xrange_ordered(raw).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, b"1-0");
        assert_eq!(
            entries[0].1,
            vec![
                (b"f2".to_vec(), b"v2".to_vec()),
                (b"f1".to_vec(), b"v1".to_vec()),
            ]
        );

        // RESP3：字段部分为 Map（保序）
        let raw3 = Value::Array(vec![Value::Array(vec![
            Value::SimpleString("1-0".into()),
            Value::Map(vec![
                (
                    Value::SimpleString("f2".into()),
                    Value::BulkString(b"v2".to_vec()),
                ),
                (
                    Value::SimpleString("f1".into()),
                    Value::BulkString(b"v1".to_vec()),
                ),
            ]),
        ])]);
        let entries = parse_xrange_ordered(raw3).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, b"1-0");
        assert_eq!(
            entries[0].1,
            vec![
                (b"f2".to_vec(), b"v2".to_vec()),
                (b"f1".to_vec(), b"v1".to_vec()),
            ]
        );
    }

    /// 倒序续页把时间戳减 1；空、0 和非法都落成 0。
    #[test]
    fn test_ts_timestamp_dec_one() {
        assert_eq!(ts_timestamp_dec_one("100"), "99");
        assert_eq!(ts_timestamp_dec_one("1"), "0");
        assert_eq!(ts_timestamp_dec_one("0"), "0");
        assert_eq!(ts_timestamp_dec_one(""), "0");
        assert_eq!(ts_timestamp_dec_one("abc"), "0");
        assert_eq!(ts_timestamp_dec_one(" 42 "), "41");
    }

    /// 正序续页把时间戳加 1；空和非法落成 0。
    #[test]
    fn test_ts_timestamp_inc_one() {
        assert_eq!(ts_timestamp_inc_one("100"), "101");
        assert_eq!(ts_timestamp_inc_one("0"), "1");
        assert_eq!(ts_timestamp_inc_one(""), "0");
        assert_eq!(ts_timestamp_inc_one("abc"), "0");
        assert_eq!(ts_timestamp_inc_one(" 42 "), "43");
    }

    /// TS.RANGE 的样本行收成时间戳和值。
    #[test]
    fn test_parse_ts_range_items() {
        let raw = Value::Array(vec![
            Value::Array(vec![Value::Int(1000), Value::BulkString(b"1.5".to_vec())]),
            Value::Array(vec![Value::Int(900), Value::Double(2.0)]),
        ]);
        let items = parse_ts_range_items(raw).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].key, "1000");
        assert_eq!(items[0].value, "1.5");
        assert_eq!(items[1].key, "900");
        assert_eq!(items[1].value, "2");
    }

    /// 模块类型名和界面名双向对应；Array 仍走 Unknown，索引不能是负数。
    #[test]
    fn module_types_and_array_index() {
        assert_eq!(ui_key_type_str(REDIS_JSON_TYPE_NAME), ME_JSON_TYPE_NAME);
        assert_eq!(
            ui_key_type_str(REDIS_TIMESERIES_TYPE_NAME),
            ME_TIMESERIES_TYPE_NAME
        );
        assert_eq!(ui_key_type_str("hash"), "hash");
        assert!(matches!(to_key_type("json"), ValueType::JSON));
        assert!(matches!(to_key_type("TimeSeries"), ValueType::TimeSeries));
        assert!(is_array_type(&to_key_type("Array")));
        assert!(!is_array_type(&to_key_type("hash")));
        assert_eq!(parse_array_index(" 12 ").unwrap(), 12);
        assert!(parse_array_index("-1").is_err());
        assert!(parse_array_index("1.5").is_err());
    }

    /// ARSCAN 嵌套对和扁平对都能收成索引加字节；空回复是空列表。
    #[test]
    fn arscan_nested_and_flat_pairs() {
        assert!(parse_arscan_pairs(Value::Nil).unwrap().is_empty());
        let nested = Value::Array(vec![Value::Array(vec![
            Value::Int(3),
            Value::BulkString(b"ab".to_vec()),
        ])]);
        assert_eq!(
            parse_arscan_pairs(nested).unwrap(),
            vec![(3, b"ab".to_vec())]
        );
        let flat = Value::Array(vec![
            Value::Int(1),
            Value::BulkString(vec![0xff]),
            Value::Int(2),
        ]);
        let pairs = parse_arscan_pairs(flat).unwrap();
        assert_eq!(pairs, vec![(1, vec![0xff])]);
    }

    /// 界面行保留下标和字段顺序；内存结果按字节从大到小，模块类型名换成界面名。
    #[test]
    fn ui_rows_keep_order_and_sort_sizes() {
        let list = ui_list_items(3, &[b"a".to_vec(), vec![0xff]], &BytesFormat::UTF8);
        assert_eq!(list[0].index, 3);
        assert_eq!(list[0].value, "a");
        assert_eq!(list[1].index, 4);
        assert_ne!(list[1].value, "a");

        let hash = ui_hash_value(&[(b"f".to_vec(), vec![0xff])], &BytesFormat::Base64);
        assert_eq!(hash[0].key, "Zg==");
        assert!(hash[0].ttl.is_none());

        let zset = ui_zset_value(
            vec![(b"m1".to_vec(), 1.0), (b"m2".to_vec(), 2.0)],
            &BytesFormat::UTF8,
        );
        assert_eq!(zset[0].value, "m1");
        assert_eq!(zset[1].score, 2.0);

        let sizes = tuple_to_key_size(vec![
            (b"small".to_vec(), 1, "string".into()),
            (b"big".to_vec(), 9, REDIS_JSON_TYPE_NAME.into()),
        ]);
        assert_eq!(sizes[0].key, "big");
        assert_eq!(sizes[0].key_type, ME_JSON_TYPE_NAME);
        assert_eq!(sizes[1].size, 1);
    }

    /// ARSCAN 收成带索引的界面行，值按当前 wire 格式编码。
    #[test]
    fn array_items_from_arscan() {
        let raw = Value::Array(vec![Value::Array(vec![
            Value::Int(2),
            Value::BulkString(b"ab".to_vec()),
        ])]);
        let items = ui_array_items_from_arscan(raw, &BytesFormat::UTF8).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].index, 2);
        assert_eq!(items[0].value, "ab");
    }

    /// totalSamples 大小写不敏感；没有这个字段时返回 None。
    #[test]
    fn ts_info_total_samples_is_optional() {
        let raw = Value::Array(vec![
            Value::BulkString(b"totalsamples".to_vec()),
            Value::Int(6),
        ]);
        assert_eq!(ts_info_total_samples(&raw), Some(6));
        assert_eq!(ts_info_total_samples(&Value::Array(vec![])), None);
    }
}
