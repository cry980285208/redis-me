use crate::client::ops::field_scan::handle_other_value_type;
use crate::model::*;
use crate::support::error::AppError;
use crate::support::util::*;
use anyhow::bail;
use parking_lot::MutexGuard;
use redis::streams::{StreamInfoConsumersReply, StreamInfoGroupsReply};
use redis::{Commands, FromRedisValue, Value, ValueType};

/// OBJECT ENCODING / IDLETIME / REFCOUNT / FREQ；IDLETIME/FREQ 受 maxmemory-policy 限制时写入 *_error
pub fn object_info0(
    mut conn: MutexGuard<impl Commands>,
    key: RedisKey,
) -> AnyResult<RedisObjectInfo> {
    let encoding: Option<String> = conn.object_encoding(&key)?;
    let refcount_raw: Option<usize> = conn.object_refcount(&key)?;
    let refcount = refcount_raw.map(|n| n as u64);

    // LFU 策略下 IDLETIME 报错；非 LFU 下 FREQ 报错 —— 记录原因供前端提示
    let (idle_time, idle_time_error) = match conn.object_idletime(&key) {
        Ok(v) => {
            let n: Option<usize> = v;
            (n.map(|x| x as u64), None)
        }
        Err(e) => (None, Some(e.to_string())),
    };
    let (freq, freq_error) = match conn.object_freq(&key) {
        Ok(v) => {
            let n: Option<usize> = v;
            (n.map(|x| x as u64), None)
        }
        Err(e) => (None, Some(e.to_string())),
    };

    Ok(RedisObjectInfo {
        encoding,
        idle_time,
        idle_time_error,
        refcount,
        freq,
        freq_error,
    })
}

/// Array ARINFO：元数据（默认不含 FULL）；RESP2 扁平键值对 / RESP3 Map。
pub fn ar_info0(
    mut conn: MutexGuard<impl Commands>,
    key: RedisKey,
) -> AnyResult<Vec<RedisArInfoItem>> {
    let key_type: ValueType = conn.key_type(&key)?;
    if !is_array_type(&key_type) {
        handle_other_value_type(&key_type, &key)?;
        unreachable!()
    }
    let raw: Value = redis::cmd("ARINFO").arg(&key).query(&mut conn)?;
    parse_info_kv_items(raw, "ARINFO")
}

/// TimeSeries TS.INFO：元数据；行结构同 ARINFO。labels/rules 等嵌套数组展平为可读字符串。
pub fn ts_info0(
    mut conn: MutexGuard<impl Commands>,
    key: RedisKey,
) -> AnyResult<Vec<RedisArInfoItem>> {
    let key_type: ValueType = conn.key_type(&key)?;
    if key_type != ValueType::TimeSeries {
        handle_other_value_type(&key_type, &key)?;
        unreachable!()
    }
    let raw: Value = redis::cmd("TS.INFO").arg(&key).query(&mut conn)?;
    parse_ts_info_items(raw)
}

/// ARINFO / VINFO 等扁平键值回复 → 保序 field/value 行
pub fn parse_info_kv_items(raw: Value, cmd: &str) -> AnyResult<Vec<RedisArInfoItem>> {
    match raw {
        Value::Nil => Ok(Vec::new()),
        Value::Map(map) => Ok(map
            .into_iter()
            .map(|(k, v)| RedisArInfoItem {
                field: redis_value_to_string(k, ""),
                value: redis_value_to_string(v, ""),
            })
            .collect()),
        Value::Array(arr) => {
            let mut items = Vec::with_capacity(arr.len() / 2);
            let mut i = 0;
            while i + 1 < arr.len() {
                items.push(RedisArInfoItem {
                    field: redis_value_to_string(arr[i].clone(), ""),
                    value: redis_value_to_string(arr[i + 1].clone(), ""),
                });
                i += 2;
            }
            Ok(items)
        }
        other => bail!(AppError::Internal {
            message: format!("unexpected {} reply: {:?}", cmd, other)
        }),
    }
}

/// Array ARLASTITEMS：最近插入的元素（REV 时最近优先）。
/// 官方 reply 允许 string | null；稀疏 ARMSET 键可能含空槽 null（与 ARINSERT/ARRING 场景不同）。
pub fn ar_last_items0(
    mut conn: MutexGuard<impl Commands>,
    param: RedisArLastItems,
) -> AnyResult<Vec<RedisArLastItemsItem>> {
    let key: RedisKey = param.key;
    let key_type: ValueType = conn.key_type(&key)?;
    if !is_array_type(&key_type) {
        handle_other_value_type(&key_type, &key)?;
        unreachable!()
    }
    let val_fmt = param.val_fmt.as_ref().cloned().unwrap_or_default();
    let count = if param.count == 0 { 10 } else { param.count };
    let mut cmd = redis::cmd("ARLASTITEMS");
    cmd.arg(&key).arg(count);
    if param.reverse {
        cmd.arg("REV");
    }
    let raw: Value = cmd.query(&mut conn)?;
    let arr = match raw {
        Value::Nil => Vec::new(),
        Value::Array(a) => a,
        other => bail!(AppError::Internal {
            message: format!("unexpected ARLASTITEMS reply: {:?}", other)
        }),
    };
    let mut items = Vec::with_capacity(arr.len());
    for (i, entry) in arr.into_iter().enumerate() {
        let value = match entry {
            Value::Nil => None,
            v => {
                let bytes: Vec<u8> = FromRedisValue::from_redis_value(v)
                    .map_err(|e| anyhow::anyhow!("ARLASTITEMS value parse: {}", e))?;
                Some(format_bytes(&bytes, &val_fmt))
            }
        };
        items.push(RedisArLastItemsItem {
            index: i as i64,
            value,
        });
    }
    Ok(items)
}

/// `TYPE`。键不存在时返回 `none`。
pub fn key_type0(mut conn: MutexGuard<impl Commands>, key: RedisKey) -> AnyResult<String> {
    // 简单字符串回复：key 的类型，如果 key 不存在则返回 none
    let key_type: ValueType = conn.key_type(&key)?;
    Ok(ui_key_type(key_type))
}

/// `XINFO GROUPS`：Stream 的消费组。
pub fn xinfo_groups0(
    mut conn: MutexGuard<impl Commands>,
    key: RedisKey,
) -> AnyResult<Vec<XInfoGroup>> {
    let reply: StreamInfoGroupsReply = conn.xinfo_groups(&key)?;
    Ok(reply.groups.into_iter().map(ui_xinfo_group).collect())
}

/// `XINFO CONSUMERS`：某个消费组里的消费者。
pub fn xinfo_consumers0(
    mut conn: MutexGuard<impl Commands>,
    key: RedisKey,
    group: String,
) -> AnyResult<Vec<XInfoConsumer>> {
    let reply: StreamInfoConsumersReply = conn.xinfo_consumers(&key, &group)?;
    Ok(reply.consumers.into_iter().map(ui_xinfo_consumer).collect())
}

/// `FLUSHDB`：清空当前库。
pub fn flush_db0(mut conn: MutexGuard<impl Commands>) -> AnyResult<()> {
    let _: () = conn.flushdb()?;
    Ok(())
}

/// `FLUSHALL`：清空全部库。
pub fn flush_all0(mut conn: MutexGuard<impl Commands>) -> AnyResult<()> {
    let _: () = conn.flushall()?;
    Ok(())
}

// ------------------------------ 仅本文件使用 ------------------------------

/// TS.INFO 嵌套值展平：label 对 `k=v`；规则等多元素用 `,`；多组用 `; `
fn format_ts_info_value(value: Value) -> String {
    match value {
        Value::Array(items)
            if !items.is_empty() && items.iter().all(|x| matches!(x, Value::Array(_))) =>
        {
            items
                .into_iter()
                .map(|item| match item {
                    Value::Array(pair) if pair.len() == 2 => {
                        format!(
                            "{}={}",
                            redis_value_to_string(pair[0].clone(), ""),
                            redis_value_to_string(pair[1].clone(), "")
                        )
                    }
                    Value::Array(parts) => parts
                        .into_iter()
                        .map(|p| redis_value_to_string(p, ""))
                        .collect::<Vec<_>>()
                        .join(","),
                    other => redis_value_to_string(other, ""),
                })
                .collect::<Vec<_>>()
                .join("; ")
        }
        Value::Array(items) => items
            .into_iter()
            .map(|x| redis_value_to_string(x, ""))
            .collect::<Vec<_>>()
            .join(", "),
        other => redis_value_to_string(other, ", "),
    }
}

/// `TS.INFO` 的扁平键值或 Map。嵌套的 labels、rules 先展平成可读字符串。
fn parse_ts_info_items(raw: Value) -> AnyResult<Vec<RedisArInfoItem>> {
    match raw {
        Value::Nil => Ok(Vec::new()),
        Value::Map(map) => Ok(map
            .into_iter()
            .map(|(k, v)| RedisArInfoItem {
                field: redis_value_to_string(k, ""),
                value: format_ts_info_value(v),
            })
            .collect()),
        Value::Array(arr) => {
            let mut items = Vec::with_capacity(arr.len() / 2);
            let mut i = 0;
            while i + 1 < arr.len() {
                items.push(RedisArInfoItem {
                    field: redis_value_to_string(arr[i].clone(), ""),
                    value: format_ts_info_value(arr[i + 1].clone()),
                });
                i += 2;
            }
            Ok(items)
        }
        other => bail!(AppError::Internal {
            message: format!("unexpected TS.INFO reply: {:?}", other)
        }),
    }
}

#[cfg(test)]
mod ts_info_parse_tests {
    use super::*;

    #[test]
    fn labels_pairs_flatten() {
        let raw = Value::Array(vec![
            Value::BulkString(b"totalSamples".to_vec()),
            Value::Int(2),
            Value::BulkString(b"labels".to_vec()),
            Value::Array(vec![
                Value::Array(vec![
                    Value::BulkString(b"device".to_vec()),
                    Value::BulkString(b"thermometer".to_vec()),
                ]),
                Value::Array(vec![
                    Value::BulkString(b"location".to_vec()),
                    Value::BulkString(b"lab".to_vec()),
                ]),
            ]),
            Value::BulkString(b"rules".to_vec()),
            Value::Array(vec![Value::Array(vec![
                Value::BulkString(b"dest".to_vec()),
                Value::Int(60000),
                Value::BulkString(b"avg".to_vec()),
            ])]),
        ]);
        let items = parse_ts_info_items(raw).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].field, "totalSamples");
        assert_eq!(items[0].value, "2");
        assert_eq!(items[1].field, "labels");
        assert_eq!(items[1].value, "device=thermometer; location=lab");
        assert_eq!(items[2].field, "rules");
        assert_eq!(items[2].value, "dest,60000,avg");
    }
}
