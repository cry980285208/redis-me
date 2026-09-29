use crate::client::ops::field_scan::{vgetattr_opt, ARRAY_INDEX_MAX};
use crate::support::error::AppError;
use crate::model::*;
use crate::cli::format::*;
use crate::support::util::*;
use anyhow::bail;
use parking_lot::MutexGuard;
use redis::{Commands, Value, ValueType};

/// 单键 → redis-cli 可执行命令行列表（全量读取，与键值页 fieldScan 分页无关）
pub fn key_as_command_lines(conn: &mut impl Commands, key: &RedisKey) -> AnyResult<Vec<String>> {
    let key_type: ValueType = conn.key_type(&key)?;
    if key_type == ValueType::None {
        bail!(AppError::KeyNotFound {
            key: vec8_to_display_string(key.to_bytes())
        });
    }

    let key_bytes = key.to_bytes();
    let lines = match key_type {
        ValueType::String => {
            let value: Vec<u8> = conn.get(&key)?;
            vec![format_set_command(key_bytes, &value)]
        }
        ValueType::Hash => {
            let pairs: Vec<(Vec<u8>, Vec<u8>)> = conn.hgetall(&key)?;
            format_hmset_command(key_bytes, &pairs)
                .map(|s| vec![s])
                .unwrap_or_default()
        }
        ValueType::List => {
            let items: Vec<Vec<u8>> = conn.lrange(&key, 0, -1)?;
            format_rpush_command(key_bytes, &items)
                .map(|s| vec![s])
                .unwrap_or_default()
        }
        ValueType::Set => {
            let members: Vec<Vec<u8>> = conn.smembers(&key)?;
            format_sadd_command(key_bytes, &members)
                .map(|s| vec![s])
                .unwrap_or_default()
        }
        ValueType::ZSet => {
            let pairs: Vec<(Vec<u8>, f64)> = conn.zrange_withscores(&key, 0, -1)?;
            format_zadd_command(key_bytes, &pairs)
                .map(|s| vec![s])
                .unwrap_or_default()
        }
        ValueType::Stream => {
            let raw: Value = redis::cmd("XRANGE")
                .arg(&key)
                .arg("-")
                .arg("+")
                .query(conn)?;
            let entries = parse_xrange_ordered(raw)?;
            entries
                .iter()
                .map(|(id, fields)| format_xadd_command(key_bytes, id, fields))
                .collect()
        }
        ValueType::JSON => {
            let json: Value = redis::cmd("JSON.GET").arg(&key).query(conn)?;
            match json {
                Value::Nil => vec![],
                Value::BulkString(b) if b.is_empty() => vec![],
                Value::BulkString(b) => vec![format_json_set_command(key_bytes, &b)],
                other => {
                    let b = redis_value_to_bulk_bytes(other);
                    if b.is_empty() {
                        vec![]
                    } else {
                        vec![format_json_set_command(key_bytes, &b)]
                    }
                }
            }
        }
        // Array：ARSCAN 全量 → ARMSET；见 is_array_type 升级注释
        _ if is_array_type(&key_type) => {
            let raw: Value = redis::cmd("ARSCAN")
                .arg(&key)
                .arg(0u64)
                .arg(ARRAY_INDEX_MAX)
                .query(conn)?;
            let pairs = parse_arscan_pairs(raw)?;
            format_armset_command(key_bytes, &pairs)
                .map(|s| vec![s])
                .unwrap_or_default()
        }
        // Vector Set：受控 VRANGE 批量（上限 1000）→ 多条 VADD（向量来自 VEMB，可能近似）
        ValueType::VectorSet => {
            const VSET_EXPORT_LIMIT: u64 = 1000;
            let mut lines = Vec::new();
            let mut start: Vec<u8> = b"-".to_vec();
            loop {
                let names: Vec<Vec<u8>> = match redis::cmd("VRANGE")
                    .arg(key)
                    .arg(&start)
                    .arg("+")
                    .arg(100)
                    .query(conn)
                {
                    Ok(names) => names,
                    Err(_) => break, // VRANGE 不支持，跳过导出
                };
                if names.is_empty() {
                    break;
                }
                for name in &names {
                    if lines.len() as u64 >= VSET_EXPORT_LIMIT {
                        break;
                    }
                    let Ok(nums) = conn.vemb::<_, _, Vec<f64>>(key, name) else {
                        continue;
                    };
                    if nums.is_empty() {
                        continue;
                    }
                    let attrs = vgetattr_opt(conn, key, name);
                    lines.push(format_vadd_command(
                        key_bytes,
                        &nums,
                        name,
                        attrs.as_deref(),
                    ));
                }
                if lines.len() as u64 >= VSET_EXPORT_LIMIT || names.len() < 100 {
                    break;
                }
                let mut next = vec![b'('];
                next.extend_from_slice(names.last().unwrap());
                start = next;
            }
            lines
        }
        // TimeSeries：TS.REVRANGE 分页导出 → 多条 TS.ADD（上限对齐 VectorSet）
        ValueType::TimeSeries => {
            const TS_EXPORT_LIMIT: u64 = 1000;
            let mut lines = Vec::new();
            let mut to = "+".to_string();
            loop {
                let remain = TS_EXPORT_LIMIT.saturating_sub(lines.len() as u64);
                if remain == 0 {
                    break;
                }
                let page = remain.min(100);
                let raw: Value = redis::cmd("TS.REVRANGE")
                    .arg(key)
                    .arg("-")
                    .arg(&to)
                    .arg("COUNT")
                    .arg(page)
                    .query(conn)?;
                let items = parse_ts_range_items(raw)?;
                if items.is_empty() {
                    break;
                }
                for item in &items {
                    if lines.len() as u64 >= TS_EXPORT_LIMIT {
                        break;
                    }
                    lines.push(format_ts_add_command(
                        key_bytes,
                        &item.key,
                        &item.value,
                    ));
                }
                if (items.len() as u64) < page || lines.len() as u64 >= TS_EXPORT_LIMIT {
                    break;
                }
                to = ts_timestamp_dec_one(&items.last().unwrap().key);
            }
            lines
        }
        other => bail!(AppError::KeyTypeUnsupported {
            value_type: ui_key_type(other)
        }),
    };
    Ok(lines)
}

/// 单键 → redis-cli 可执行命令（全量读取，与键值页 fieldScan 分页无关）
pub fn get_key_as_command0(
    mut conn: MutexGuard<impl Commands>,
    key: RedisKey,
) -> AnyResult<String> {
    Ok(key_as_command_lines(&mut conn, &key)?.join("\n"))
}

/// 表格单行 → redis-cli 可执行命令（Hash HSET / List RPUSH / Set SADD / ZSet ZADD / Stream XADD）
pub fn get_field_as_command0(
    mut conn: MutexGuard<impl Commands>,
    param: RedisFieldAsCommand,
) -> AnyResult<String> {
    let key: RedisKey = param.key;
    let key_type: ValueType = conn.key_type(&key)?;
    if key_type == ValueType::None {
        bail!(AppError::KeyNotFound {
            key: vec8_to_display_string(key.to_bytes())
        });
    }

    let key_bytes = key.to_bytes();
    let val_fmt = param.val_fmt.as_ref().cloned().unwrap_or_default();

    let line = match key_type {
        ValueType::Hash => {
            let field_bytes = parse_bytes(&param.field_key, &val_fmt)?;
            let value: Option<Vec<u8>> = conn.hget(&key, &field_bytes)?;
            let value_bytes = value.ok_or_else(|| AppError::FieldNotFound {
                hash_key: param.field_key.clone(),
            })?;
            format_hset_command(key_bytes, &field_bytes, &value_bytes)
        }
        ValueType::List => {
            let value: Option<Vec<u8>> = conn.lindex(&key, param.field_index)?;
            let value_bytes = value.ok_or_else(|| AppError::FieldNotFound {
                hash_key: param.field_index.to_string(),
            })?;
            format_rpush_command(key_bytes, std::slice::from_ref(&value_bytes)).ok_or_else(
                || AppError::Internal {
                    message: "empty list element".into(),
                },
            )?
        }
        ValueType::Set => {
            let member_bytes = parse_bytes(&param.field_value, &val_fmt)?;
            format_sadd_command(key_bytes, std::slice::from_ref(&member_bytes)).ok_or_else(
                || AppError::Internal {
                    message: "empty set member".into(),
                },
            )?
        }
        ValueType::ZSet => {
            let member_bytes = parse_bytes(&param.field_value, &val_fmt)?;
            let score: Option<f64> = conn.zscore(&key, &member_bytes)?;
            let score = score.ok_or_else(|| AppError::FieldNotFound {
                hash_key: param.field_value.clone(),
            })?;
            format_zadd_command(key_bytes, &[(member_bytes, score)]).ok_or_else(|| {
                AppError::Internal {
                    message: "empty zset member".into(),
                }
            })?
        }
        ValueType::Stream => {
            if param.stream_id.is_empty() {
                bail!(AppError::FieldNotFoundStream {
                    stream_id: param.stream_id
                });
            }
            let raw: Value = redis::cmd("XRANGE")
                .arg(&key)
                .arg(&param.stream_id)
                .arg(&param.stream_id)
                .query(&mut conn)?;
            let entries = parse_xrange_ordered(raw)?;
            let (id, fields) = entries
                .first()
                .ok_or_else(|| AppError::FieldNotFoundStream {
                    stream_id: param.stream_id.clone(),
                })?;
            format_xadd_command(key_bytes, id, fields)
        }
        // Array：单槽 ARSET；见 is_array_type 升级注释
        _ if is_array_type(&key_type) => {
            let idx = param.field_index as i64;
            if idx < 0 {
                bail!("invalid array index: {}", idx);
            }
            let value: Option<Vec<u8>> = redis::cmd("ARGET").arg(&key).arg(idx).query(&mut conn)?;
            let value_bytes = value.ok_or_else(|| AppError::FieldNotFound {
                hash_key: idx.to_string(),
            })?;
            format_arset_command(key_bytes, idx, &value_bytes)
        }
        // Vector Set：VEMB + 可选 VGETATTR → VADD … [SETATTR]（向量可能为近似值）
        ValueType::VectorSet => {
            let elem = parse_bytes(&param.field_key, &val_fmt)?;
            let nums: Vec<f64> = conn
                .vemb(&key, &elem)
                .map_err(|_| AppError::FieldNotFound {
                    hash_key: param.field_key.clone(),
                })?;
            if nums.is_empty() {
                bail!(AppError::FieldNotFound {
                    hash_key: param.field_key.clone(),
                });
            }
            let attrs = vgetattr_opt(&mut conn, &key, &elem);
            format_vadd_command(key_bytes, &nums, &elem, attrs.as_deref())
        }
        // TimeSeries：行内 timestamp/value 已是明文（表格传来），直接拼 TS.ADD
        ValueType::TimeSeries => {
            let ts = param.field_key.trim();
            let val = param.field_value.trim();
            if ts.is_empty() {
                bail!(AppError::FieldNotFound {
                    hash_key: param.field_key.clone(),
                });
            }
            if val.is_empty() {
                bail!(AppError::FieldNotFound {
                    hash_key: ts.to_string(),
                });
            }
            format_ts_add_command(key_bytes, ts, val)
        }
        other => bail!(AppError::KeyTypeUnsupported {
            value_type: ui_key_type(other)
        }),
    };
    Ok(line)
}
