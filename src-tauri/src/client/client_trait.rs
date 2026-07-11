use crate::utils::command_log::CommandLogger;
use crate::utils::conn::set_client_name;
use crate::utils::error::AppError;
use crate::utils::model::*;
use crate::utils::redis_cli_format::*;
use crate::utils::util::*;
use Ordering::Relaxed;
use anyhow::{Context, bail};
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use chrono::Local;
use log::{info, warn};
use parking_lot::MutexGuard;
use redis::acl::Rule;
use redis::streams::{StreamInfoConsumersReply, StreamInfoGroupsReply, StreamRangeReply};
use redis::{
    Cmd, Commands, Connection, CopyOptions, ExpireOption, FromRedisValue, IntegerReplyOrNoOp,
    JsonCommands, Msg, SetExpiry, SetOptions, Value, ValueType, from_redis_value,
};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::thread::JoinHandle;
use tauri::{AppHandle, Emitter};

// RedisME服务接口
pub trait MeClient: Send + Sync {
    fn base(&self) -> &MeBase;

    fn name(&self) -> String {
        self.base().conf.name.clone()
    }

    fn db_list(&self) -> AnyResult<Vec<RedisDB>>;

    fn select_db(&self, db: u16) -> AnyResult<()>;

    fn info(&self, node: Option<String>) -> AnyResult<RedisInfo>;

    fn info_list(&self) -> AnyResult<Vec<RedisInfo>>;

    fn chart(&self, node: Option<String>) -> AnyResult<RedisChart> {
        info_to_chart(self.info(node)?)
    }

    fn chart_list(&self) -> AnyResult<Vec<RedisChart>> {
        let info_list = self.info_list()?;
        info_list.into_iter().map(info_to_chart).collect()
    }

    fn node_list(&self) -> AnyResult<Vec<RedisNode>>;

    fn scan(&self, param: ScanParam) -> AnyResult<ScanResult>;

    fn field_scan(&self, param: FieldScanParam) -> AnyResult<FieldScanResult>;

    fn ttl(&self, key: RedisKey, ttl: i64) -> AnyResult<()>;

    fn set(&self, param: RedisSetParam) -> AnyResult<()>;

    fn del(&self, key: RedisKey) -> AnyResult<()>;

    fn rename(&self, key: RedisKey, new_key: RedisKey) -> AnyResult<RedisKey>;

    fn copy(&self, param: RedisCopyParam) -> AnyResult<RedisKey>;

    fn field_add(&self, param: RedisFieldAdd) -> AnyResult<RedisKey>;

    fn field_set(&self, param: RedisFieldSet) -> AnyResult<()>;

    fn field_get(&self, param: RedisFieldGet) -> AnyResult<RedisFieldValue>;

    fn hash_keys(&self, param: RedisHashKeys) -> AnyResult<Vec<String>>;

    fn hash_values(&self, param: RedisHashKeys) -> AnyResult<Vec<String>>;

    fn field_pop(&self, param: RedisPop) -> AnyResult<String>;

    fn field_del(&self, param: RedisFieldDel) -> AnyResult<()>;

    fn execute_command(&self, param: RedisCommand) -> AnyResult<String>;

    fn config_get(&self, pattern: &str, node: Option<String>)
    -> AnyResult<HashMap<String, String>>;

    fn config_set(&self, key: &str, value: &str, node: Option<String>) -> AnyResult<()>;

    fn slow_log(&self, count: Option<u64>, node: Option<String>) -> AnyResult<Vec<RedisSlowLog>>;

    fn memory_usage(&self, param: RedisMemoryParam) -> AnyResult<Vec<RedisKeySize>>;

    fn client_list(
        &self,
        node: Option<String>,
        client_type: Option<String>,
    ) -> AnyResult<Vec<RedisClientInfo>>;

    fn publish(&self, channel: &str, message: &str, msg_fmt: Option<BytesFormat>) -> AnyResult<()>;

    fn subscribe(&self, channel: Option<String>) -> AnyResult<()>;
    fn subscribe_stop(&self) -> AnyResult<()>;

    fn monitor(&self, node: &str) -> AnyResult<()>;
    fn monitor_stop(&self) -> AnyResult<()>;

    fn batch_del(&self, param: RedisBatchKey) -> AnyResult<()>;
    fn batch_ttl(&self, param: RedisBatchTtl) -> AnyResult<()>;
    fn export_csv(&self, param: RedisExportCsv) -> AnyResult<()>;
    fn import_csv(&self, param: RedisImportCsv) -> AnyResult<()>;
    fn import_cmd(&self, file: String) -> AnyResult<()>;

    fn mock_data(&self, count: u64) -> AnyResult<()>;
    fn key_type(&self, key: RedisKey) -> AnyResult<String>;
    fn get_key_as_command(&self, key: RedisKey) -> AnyResult<String>;
    fn get_field_as_command(&self, param: RedisFieldAsCommand) -> AnyResult<String>;
    fn xinfo_groups(&self, key: RedisKey) -> AnyResult<Vec<XInfoGroup>>;
    fn xinfo_consumers(&self, key: RedisKey, group: String) -> AnyResult<Vec<XInfoConsumer>>;
    fn key_slot(&self, key: RedisKey) -> AnyResult<u64>;
    fn key_node(&self, key: RedisKey) -> AnyResult<Vec<RedisNode>>;
    fn flush_db(&self) -> AnyResult<()>;
    fn flush_all(&self) -> AnyResult<()>;

    fn acl_users(&self) -> AnyResult<Vec<String>>;
    fn acl_list_users(&self) -> AnyResult<Vec<AclUserDetail>>;
    fn acl_getuser(&self, username: &str) -> AnyResult<AclUserDetail>;
    fn acl_setuser(&self, param: AclSetuserParam) -> AnyResult<()>;
    fn acl_deluser(&self, usernames: Vec<String>) -> AnyResult<usize>;
    fn acl_whoami(&self) -> AnyResult<String>;
    fn acl_cat(&self, category: Option<String>) -> AnyResult<Vec<String>>;
    fn acl_genpass(&self, bits: Option<i64>) -> AnyResult<String>;
    fn acl_save(&self) -> AnyResult<()>;
    fn acl_load(&self) -> AnyResult<()>;
    fn acl_log(&self, count: Option<u64>) -> AnyResult<Vec<AclLogEntry>>;
    fn acl_log_reset(&self) -> AnyResult<()>;
    fn acl_dryrun(&self, username: String, command: String) -> AnyResult<String>;

    fn command_logs(&self, limit: Option<u64>) -> AnyResult<Vec<CommandLogEntry>> {
        Ok(self.base().command_logger.query(limit))
    }

    fn command_logs_clear(&self) -> AnyResult<()> {
        self.base().command_logger.clear();
        Ok(())
    }
}

// 通用实现: 由于Connection动态兼容问题，无法写在接口里面，因此写在方法中

pub fn scan_0_batch_count(pattern: &str) -> u64 {
    // 空白或单字母查询，SCAN 的 COUNT 参数（每次扫描的 bucket 数量）使用 1000；否则使用 10000
    if pattern.replace("*", "").chars().count() <= 1 {
        1000
    } else {
        10000
    }
}

/** fieldScan 单次 HSCAN/SSCAN/ZSCAN/LRANGE 的 COUNT，来自 settings.fieldScanCount */
pub fn field_scan_batch_count(count: u64) -> u64 {
    if count == 0 { 20 } else { count }
}

fn field_scan_include_meta(param: &FieldScanParam) -> bool {
    param.include_meta.unwrap_or(true)
}

fn field_scan_include_field_ttl(param: &FieldScanParam, httl_supported: bool) -> bool {
    resolve_include_field_ttl(param.include_field_ttl, httl_supported)
}

/// 是否执行 HTTL/HEXPIRE：须同时满足服务端能力与调用方 opt（默认 false）
fn resolve_include_field_ttl(opt: Option<bool>, httl_supported: bool) -> bool {
    httl_supported && opt.unwrap_or(false)
}

/// HSET 前读取 Hash 字段剩余过期秒数；无字段级 TTL 或已永久则返回 None
fn hash_field_ttl_to_preserve(
    conn: &mut impl Commands,
    key: &RedisKey,
    field: &[u8],
    httl_supported: bool,
) -> AnyResult<Option<i64>> {
    if !httl_supported {
        return Ok(None);
    }
    let ttl_values = conn.httl::<_, _, Vec<IntegerReplyOrNoOp>>(key, &[field])?;
    Ok(match ttl_values.first() {
        Some(IntegerReplyOrNoOp::IntegerReply(ttl)) if *ttl > 0 => Some(*ttl as i64),
        _ => None,
    })
}

fn resolve_field_scan_key_type(
    conn: &mut MutexGuard<impl Commands>,
    key: &RedisKey,
    param: &FieldScanParam,
) -> AnyResult<ValueType> {
    if field_scan_include_meta(param) {
        Ok(conn.key_type(key)?)
    } else if let Some(ref t) = param.key_type {
        Ok(to_key_type(t))
    } else {
        Ok(conn.key_type(key)?)
    }
}

/// 完全匹配时用 EXISTS 判断键是否存在；否则返回 None 走 SCAN
/// 注意：EXISTS 路径不校验 scan_type，精确查完整键名时更符合实际使用场景
pub fn scan_0_exact<C: redis::ConnectionLike>(
    conn: &mut C,
    pattern: &str,
    exact: bool,
) -> AnyResult<Option<ScanResult>> {
    if !exact {
        return Ok(None);
    }
    let exists: bool = redis::cmd("EXISTS").arg(pattern).query(conn)?;
    let key_list = if exists {
        vec![RedisKey::from(pattern)]
    } else {
        vec![]
    };
    Ok(Some(ScanResult {
        cursor: ScanCursor {
            finished: true,
            ..Default::default()
        },
        key_list,
    }))
}

pub fn scan_1_cmd(cursor: u64, pattern: &str, batch_count: u64, scan_type: Option<String>) -> Cmd {
    // SCAN cursor [MATCH pattern] [COUNT count] [TYPE type]
    let mut cmd = redis::cmd("scan");
    cmd.arg(cursor)
        .arg("match")
        .arg(pattern)
        .arg("count")
        .arg(batch_count);

    if let Some(mut scan_type) = scan_type
        && !scan_type.is_empty()
    {
        if scan_type == ME_JSON_TYPE_NAME {
            scan_type = REDIS_JSON_TYPE_NAME.to_string();
        }
        cmd.arg("type").arg(scan_type);
    }
    cmd
}

pub fn field_scan_0_exact(
    conn: &mut impl Commands,
    key: &RedisKey,
    key_type: &ValueType,
    param: &FieldScanParam,
    bytes_format: &BytesFormat,
    include_field_ttl: bool,
) -> AnyResult<Option<(serde_json::Value, ScanCursor)>> {
    if !param.exact {
        return Ok(None);
    }
    let cc = ScanCursor {
        finished: true,
        ..Default::default()
    };
    let member = &param.pattern;
    let json = match key_type {
        ValueType::Hash => {
            let value: Option<Vec<u8>> = conn.hget(key, member)?;
            let mut items = match value {
                Some(bytes) => ui_hash_value(&[(member.as_bytes().to_vec(), bytes)], bytes_format),
                None => Vec::new(),
            };
            if include_field_ttl && !items.is_empty() {
                let field_bytes = member.as_bytes().to_vec();
                if let Ok(ttl_values) =
                    conn.httl::<_, _, Vec<IntegerReplyOrNoOp>>(key, &[&field_bytes])
                {
                    if let (Some(item), Some(ttl_reply)) = (items.first_mut(), ttl_values.first())
                    {
                        item.ttl = match ttl_reply {
                            IntegerReplyOrNoOp::IntegerReply(ttl) => Some(*ttl as i64),
                            IntegerReplyOrNoOp::NotExists => Some(-2),
                            IntegerReplyOrNoOp::ExistsButNotRelevant => Some(-1),
                            _ => None,
                        };
                    }
                }
            }
            serde_json::to_value(items)?
        }
        ValueType::Set => {
            let exists: bool = conn.sismember(key, member)?;
            let set = if exists {
                ui_set_value(
                    HashSet::from([member.as_bytes().to_vec()]),
                    bytes_format,
                )
            } else {
                Vec::new()
            };
            serde_json::to_value(set)?
        }
        ValueType::ZSet => {
            let score: Option<f64> = conn.zscore(key, member)?;
            let zset = score
                .map(|score| {
                    ui_zset_value(vec![(member.as_bytes().to_vec(), score)], bytes_format)
                })
                .unwrap_or_default();
            serde_json::to_value(zset)?
        }
        _ => return Ok(None),
    };
    Ok(Some((json, cc)))
}

pub fn field_scan0(
    mut conn: MutexGuard<impl Commands>,
    param: FieldScanParam,
    httl_supported: bool,
) -> AnyResult<FieldScanResult> {
    let bytes_format = param.bytes_format.as_ref().cloned().unwrap_or_default();
    let include_field_ttl = field_scan_include_field_ttl(&param, httl_supported);

    // String, Json, List, Stream 直接获取；Hash/Set/ZSet 走 exact 或 *SCAN
    let (mut value, key_type, mut cc, length, value_truncated) =
        field_scan_0_get(&mut conn, &param, &bytes_format)?;
    if value.is_none() {
        if let Some((exact_value, exact_cc)) = field_scan_0_exact(
            &mut conn,
            &param.key,
            &key_type,
            &param,
            &bytes_format,
            include_field_ttl,
        )? {
            value = Some(exact_value);
            cc = exact_cc;
        } else {
            // 每次 API 只执行一轮 HSCAN/SSCAN/ZSCAN，循环由前端控制；COUNT 用 fieldScanCount，非键扫描 batch
            let batch_count = field_scan_batch_count(param.count);
            let cmd = field_scan_1_cmd(
                &key_type,
                &param.key,
                cc.now_cursor,
                &param.pattern,
                batch_count,
            )?;
            let (next_cursor, new_value): (u64, Value) = cmd.query(&mut conn)?;
            let mut scan_value = FieldScanValue::default();
            field_scan_2_value(
                &mut conn,
                &key_type,
                &mut scan_value,
                new_value,
                &param.key,
                &bytes_format,
                include_field_ttl,
            )?;
            cc.now_cursor = next_cursor;
            if next_cursor == 0 {
                cc.finished = true;
            }
            value = Some(field_scan_3_json(&key_type, &scan_value)?);
        }
    }

    let include_meta = field_scan_include_meta(&param);
    field_scan_4_return(
        conn,
        param.key,
        key_type,
        value.unwrap_or_default(),
        cc,
        length,
        value_truncated,
        include_meta,
    )
}

/// STRING 按阈值决定 GET 全量或 GETRANGE 预览；返回 (bytes, strlen, truncated)
fn load_string_bytes(
    conn: &mut MutexGuard<impl Commands>,
    key: &RedisKey,
    param: &FieldScanParam,
) -> AnyResult<(Vec<u8>, usize, bool)> {
    let strlen: usize = conn.strlen(key)?;
    let meta = param.meta.as_ref();
    let force_full = meta.and_then(|m| m.force_full_value).unwrap_or(false);
    if !force_full {
        if let Some(limit) = meta.and_then(|m| m.value_byte_limit) {
            if strlen > limit as usize {
                let preview = meta.and_then(|m| m.value_preview_bytes).unwrap_or(1000) as usize;
                let end = preview.saturating_sub(1) as isize;
                let value: Vec<u8> = conn.getrange(key, 0, end)?;
                return Ok((value, strlen, true));
            }
        }
    }
    let value: Vec<u8> = conn.get(key)?;
    Ok((value, strlen, false))
}

fn list_scan_desc(param: &FieldScanParam) -> bool {
    param
        .meta
        .as_ref()
        .and_then(|m| m.list_desc)
        .unwrap_or(false)
}

fn resolve_list_scan_range(param: &FieldScanParam, list_len: usize) -> (i64, i64) {
    let max_default = list_len.saturating_sub(1) as i64;
    let meta = param.meta.as_ref();
    let min = meta.and_then(|m| m.list_min_index).unwrap_or(0).max(0);
    let max = meta
        .and_then(|m| m.list_max_index)
        .unwrap_or(max_default)
        .clamp(min, max_default);
    (min, max)
}

fn field_scan_list_page(
    conn: &mut MutexGuard<impl Commands>,
    key: &RedisKey,
    param: &FieldScanParam,
    bytes_format: &BytesFormat,
    cc: &mut ScanCursor,
) -> AnyResult<Vec<RedisListItem>> {
    let count = field_scan_batch_count(param.count);
    let list_len: usize = conn.llen(key)?;
    if list_len == 0 {
        cc.finished = true;
        return Ok(Vec::new());
    }
    let (range_min, range_max) = resolve_list_scan_range(param, list_len);
    let range_len = (range_max - range_min + 1).max(0) as u64;
    if range_len == 0 {
        cc.finished = true;
        return Ok(Vec::new());
    }

    let fetched = cc.now_cursor;
    if fetched >= range_len {
        cc.finished = true;
        return Ok(Vec::new());
    }

    let batch = count.min(range_len - fetched);
    let desc = list_scan_desc(param);
    let (start, end) = if desc {
        let end = range_max - fetched as i64;
        let start = end - batch as i64 + 1;
        (start, end)
    } else {
        let start = range_min + fetched as i64;
        let end = start + batch as i64 - 1;
        (start, end)
    };
    let raw: Vec<Vec<u8>> = conn.lrange(key, start as isize, end as isize)?;
    let mut items = ui_list_items(start, &raw, bytes_format);
    // LRANGE 始终按索引升序返回；降序扫描时反转，使结果页从高索引到低索引
    if desc {
        items.reverse();
    }

    cc.now_cursor += items.len() as u64;
    if cc.now_cursor >= range_len {
        cc.finished = true;
    }
    Ok(items)
}

pub fn field_scan_0_get(
    mut conn: &mut MutexGuard<impl Commands>,
    param: &FieldScanParam,
    bytes_format: &BytesFormat,
) -> AnyResult<(Option<serde_json::Value>, ValueType, ScanCursor, usize, bool)> {
    let key = &param.key;

    let key_type = resolve_field_scan_key_type(&mut conn, key, param)?;
    let mut cc = param.cursor.clone().unwrap_or_default();

    // String类型的bytes长度
    let mut length = 0;
    let mut value_truncated = false;

    let value: Option<serde_json::Value> = match key_type {
        ValueType::None => {
            bail!(AppError::KeyNotFound {
                key: vec8_to_display_string(key.to_bytes())
            })
        }
        ValueType::String => {
            let (value, strlen, truncated) = load_string_bytes(&mut conn, key, param)?;
            length = strlen;
            value_truncated = truncated;
            let value: String = format_bytes(&value, bytes_format);
            cc.finished = true;
            Some(serde_json::to_value(value)?)
        }
        ValueType::JSON => {
            let value: Value = redis::cmd("JSON.GET").arg(key).query(&mut conn)?;
            cc.finished = true;
            Some(serde_json::from_str(&redis_value_to_string(value, "\n"))?)
        }
        ValueType::Hash => None,
        ValueType::List => {
            let items = field_scan_list_page(&mut conn, key, param, bytes_format, &mut cc)?;
            Some(serde_json::to_value(items)?)
        }
        ValueType::Stream => {
            let count = field_scan_batch_count(param.count);
            let is_desc = param
                .meta
                .as_ref()
                .and_then(|m| m.stream_desc)
                .unwrap_or(true);

            let (arg1, arg2) = if is_desc {
                // XREVRANGE: (end, start)
                let end = if cc.stream_cursor.is_empty() {
                    match param.meta.as_ref() {
                        Some(meta) if !meta.max_id.is_empty() => &meta.max_id,
                        _ => "+",
                    }
                } else {
                    &cc.stream_cursor
                };
                let start = match param.meta.as_ref() {
                    Some(meta) if !meta.min_id.is_empty() => &meta.min_id,
                    _ => "-",
                };
                (end, start)
            } else {
                // XRANGE: (start, end)
                let start = if cc.stream_cursor.is_empty() {
                    match param.meta.as_ref() {
                        Some(meta) if !meta.min_id.is_empty() => &meta.min_id,
                        _ => "-",
                    }
                } else {
                    &cc.stream_cursor
                };
                let end = match param.meta.as_ref() {
                    Some(meta) if !meta.max_id.is_empty() => &meta.max_id,
                    _ => "+",
                };
                (start, end)
            };

            let scan_count = if cc.stream_cursor.is_empty() {
                count + 1
            } else {
                count
            };

            let cmd_name = if is_desc { "XREVRANGE" } else { "XRANGE" };
            let mut cmd = redis::cmd(cmd_name);
            cmd.arg(key).arg(arg1).arg(arg2);
            cmd.arg("COUNT").arg(scan_count);
            let reply: StreamRangeReply = cmd.query(&mut conn)?;
            let mut value = ui_stream_value(reply);

            if value.len() > count as usize {
                cc.finished = false;
                cc.stream_cursor = value.pop().unwrap().id;
            } else {
                cc.finished = true;
            };
            Some(serde_json::to_value(value)?)
        }
        ValueType::Unknown(_) => {
            handle_other_value_type(&key_type, key)?;
            None
        }
        _ => None,
    };
    Ok((value, key_type, cc, length, value_truncated))
}

pub fn field_scan_1_cmd(
    key_type: &ValueType,
    key: &RedisKey,
    cursor: u64,
    pattern: &str,
    batch_count: u64,
) -> AnyResult<Cmd> {
    let scan_command = match key_type {
        ValueType::Hash => "hscan",
        ValueType::Set => "sscan",
        ValueType::ZSet => "zscan",
        _ => bail!(AppError::FieldScanNotSupported {
            value_type: ui_key_type(key_type.clone())
        }),
    };

    let mut cmd = redis::cmd(scan_command);
    cmd.arg(key).arg(cursor);
    if !pattern.is_empty() && pattern != "*" {
        cmd.arg("MATCH").arg(pattern);
    }
    cmd.arg("COUNT").arg(batch_count);
    Ok(cmd)
}

pub fn field_scan_2_value(
    conn: &mut impl Commands,
    key_type: &ValueType,
    scan_value: &mut FieldScanValue,
    new_value: Value,
    key: &RedisKey,
    bytes_format: &BytesFormat,
    include_field_ttl: bool,
) -> AnyResult<usize> {
    let new_count = match key_type {
        ValueType::Hash => {
            let value: Vec<(Vec<u8>, Vec<u8>)> = FromRedisValue::from_redis_value(new_value)?;
            let new_count = value.len();
            let mut new_value = ui_hash_value(&value, bytes_format);

            if include_field_ttl {
                let fields: Vec<&Vec<u8>> = value.iter().map(|(f, _)| f).collect();
                if let Ok(ttl_values) = conn.httl::<_, _, Vec<IntegerReplyOrNoOp>>(key, &fields) {
                    for (item, ttl_reply) in new_value.iter_mut().zip(ttl_values) {
                        item.ttl = match ttl_reply {
                            IntegerReplyOrNoOp::IntegerReply(ttl) => Some(ttl as i64),
                            IntegerReplyOrNoOp::NotExists => Some(-2),
                            IntegerReplyOrNoOp::ExistsButNotRelevant => Some(-1),
                            _ => None,
                        };
                    }
                }
            }

            scan_value.hash.extend(new_value);
            new_count
        }
        ValueType::Set => {
            let value: HashSet<Vec<u8>> = FromRedisValue::from_redis_value(new_value)?;
            let new_count = value.len();
            scan_value.set.extend(ui_set_value(value, bytes_format));
            new_count
        }

        ValueType::ZSet => {
            let value: Vec<(Vec<u8>, f64)> = FromRedisValue::from_redis_value(new_value)?;
            let new_count = value.len();
            scan_value.zset.extend(ui_zset_value(value, bytes_format));
            new_count
        }
        _ => bail!(AppError::FieldScanNotSupported {
            value_type: ui_key_type(key_type.clone())
        }),
    };
    Ok(new_count)
}

pub fn field_scan_3_json(
    key_type: &ValueType,
    scan_value: &FieldScanValue,
) -> AnyResult<serde_json::value::Value> {
    let value = match key_type {
        ValueType::Hash => serde_json::to_value(&scan_value.hash)?,
        ValueType::Set => serde_json::to_value(&scan_value.set)?,
        ValueType::ZSet => serde_json::to_value(&scan_value.zset)?,
        _ => bail!(AppError::FieldScanNotSupported {
            value_type: ui_key_type(key_type.clone())
        }),
    };
    Ok(value)
}

/// 集合类型用 HLEN/LLEN 等填充 length；String 仍用已算好的 bytes 长度
fn resolve_field_scan_length(
    conn: &mut MutexGuard<impl Commands>,
    key: &RedisKey,
    key_type: &ValueType,
    field_byte_len: usize,
) -> AnyResult<usize> {
    let len = match key_type {
        ValueType::String => field_byte_len,
        ValueType::Hash => conn.hlen(key)?,
        ValueType::List => conn.llen(key)?,
        ValueType::Set => conn.scard(key)?,
        ValueType::ZSet => conn.zcard(key)?,
        ValueType::Stream => redis::cmd("XLEN").arg(key).query(conn)?,
        _ => field_byte_len,
    };
    Ok(len)
}

pub fn field_scan_4_return(
    mut conn: MutexGuard<impl Commands>,
    key: RedisKey,
    key_type: ValueType,
    value: serde_json::Value,
    cursor: ScanCursor,
    length: usize,
    value_truncated: bool,
    include_meta: bool,
) -> AnyResult<FieldScanResult> {
    let (ttl, size, length) = if include_meta {
        let ttl: i64 = conn.ttl(&key)?;
        let size: u64 = redis::cmd("memory")
            .arg("usage")
            .arg(&key)
            .query(&mut conn)
            .unwrap_or(0);
        let length = resolve_field_scan_length(&mut conn, &key, &key_type, length)?;
        (ttl, size, length)
    } else {
        (0, 0, length)
    };

    Ok(FieldScanResult {
        key_type: ui_key_type(key_type),
        ttl,
        size,
        value,
        cursor,
        length,
        value_truncated,
    })
}

pub fn ttl0(mut conn: MutexGuard<impl Commands>, key: RedisKey, ttl: i64) -> AnyResult<()> {
    if ttl > 0 {
        // 为 key 设置超时时间。超时时间到期后，该 key 将被自动删除。
        // 请注意，调用 EXPIRE/`PEXPIRE` 时使用非正数超时，或调用 `EXPIREAT`/`PEXPIREAT` 时使用过去的时间，
        // 将导致 key 被 删除 而非过期（相应地，发出的 key 事件 将是 del，而不是 expired）。
        // 整数回复：如果未设置超时时间则返回 0；例如，key 不存在，或者由于提供的参数而跳过了操作。
        // 整数回复：如果已设置超时时间则返回 1。
        let _: () = conn.expire(&key, ttl)?;
    } else {
        // 移除 key 上已有的过期时间，将键从易失（设置了过期时间的键）变为变为持久
        // 整型回复: 如果 key 不存在或没有关联的过期时间，则返回 0。
        // 整型回复: 如果已移除过期时间，则返回 1。
        let _: () = conn.persist(&key)?;
    };
    Ok(())
}

pub fn set0(mut conn: MutexGuard<impl Commands>, param: RedisSetParam) -> AnyResult<()> {
    let key = param.key;
    let format = param.input_format.as_ref().cloned().unwrap_or_default();
    // 解析输入格式为字节（MsgPack 由前端编码为 base64 后传入）
    let bytes = parse_bytes(&param.value, &format)?;

    if param.key_type.unwrap_or_default() == ME_JSON_TYPE_NAME {
        // json 类型
        let value: serde_json::Value =
            serde_json::from_str(&param.value).with_context(|| "json parse error")?;
        let _: () = conn.json_set(&key, "$", &value)?;
        if param.ttl > 0 {
            let _: () = conn.expire(&key, param.ttl)?;
        }
    } else {
        // string 类型
        if param.ttl > 0 {
            let options = SetOptions::default().with_expiration(SetExpiry::EX(param.ttl as u64));
            let _: () = conn.set_options(&key, &bytes, options)?;
        } else {
            let _: () = conn.set(&key, &bytes)?;
        };
    }
    Ok(())
}

pub fn del0(mut conn: MutexGuard<impl Commands>, key: RedisKey) -> AnyResult<()> {
    let _: () = conn.del(&key)?;
    Ok(())
}

pub fn copy0(
    mut conn: MutexGuard<impl Commands>,
    param: RedisCopyParam,
) -> AnyResult<RedisKey> {
    let dest = &param.destination;
    if conn.exists(dest)? {
        bail!(AppError::KeyAlreadyExists {
            key: vec8_to_display_string(dest.to_bytes())
        });
    }

    let opts = CopyOptions::default().db(param.db);
    let _: bool = conn.copy(&param.source, dest, opts)?;
    Ok(param.destination.to_normal())
}

pub fn field_add0(
    mut conn: MutexGuard<impl Commands>,
    param: RedisFieldAdd,
    httl_supported: bool,
) -> AnyResult<RedisKey> {
    let key_fmt = param.key_fmt.as_ref().cloned().unwrap_or_default();
    let val_fmt = param.val_fmt.as_ref().cloned().unwrap_or_default();

    // `bytes` 为空：沿用界面上的键名 + key_fmt 解析；非空：扫描/详情得到的二进制键，避免经 String 丢失
    let key: RedisKey = if param.key.bytes.is_empty() {
        parse_bytes(&param.key.key, &key_fmt)?.into()
    } else {
        param.key
    };
    let mode = param.mode;
    let mut key_type = to_key_type(&param.key_type);

    match mode.as_str() {
        "key" => {
            let exists: bool = conn.exists(&key)?;
            if exists {
                bail!(AppError::KeyAlreadyExists {
                    key: vec8_to_display_string(key.to_bytes())
                })
            }
        }
        // 当键不存在时，下面的match会抛出对应异常
        "field" => key_type = conn.key_type(&key)?,
        _ => bail!(AppError::FieldOperationNotSupported { mode }),
    }

    let fv_list = param.field_value_list;

    match key_type {
        ValueType::String => {
            // 解析输入格式为字节，然后写入
            let bytes = parse_bytes(&param.value, &val_fmt)?;
            conn.set(&key, &bytes)?
        }
        ValueType::Hash => {
            // 先解析再写入，避免中途解析失败导致已写入部分字段
            let (field_pairs, ttls): (Vec<(Vec<u8>, Vec<u8>)>, Vec<i64>) = fv_list
                .iter()
                .map(|f| -> AnyResult<_> {
                    Ok((
                        (
                            parse_bytes(&f.field_key, &val_fmt)?,
                            parse_bytes(&f.field_value, &val_fmt)?,
                        ),
                        f.field_ttl,
                    ))
                })
                .collect::<AnyResult<Vec<_>>>()?
                .into_iter()
                .unzip();
            let _: () = conn.hset_multiple(&key, &field_pairs)?;
            if httl_supported {
                for ((fk, _), ttl) in field_pairs.iter().zip(&ttls) {
                    if *ttl > 0 {
                        let _: () = conn.hexpire(&key, *ttl, ExpireOption::NONE, fk)?;
                    }
                }
            }
        }
        ValueType::List => {
            let mut elems: Vec<Vec<u8>> = fv_list
                .iter()
                .map(|f| parse_bytes(&f.field_value, &val_fmt))
                .collect::<AnyResult<Vec<_>>>()?;
            let lpush = param.list_push_method == "lpush";
            if lpush {
                // 与一次 LPUSH key v_n … v_1 相同：表头插入后顺序与 fv_list 一致
                elems.reverse();
            }
            let _: usize = if lpush {
                conn.lpush(&key, &elems)?
            } else {
                conn.rpush(&key, &elems)?
            };
        }
        ValueType::Set => {
            let members: Vec<Vec<u8>> = fv_list
                .iter()
                .map(|f| parse_bytes(&f.field_value, &val_fmt))
                .collect::<AnyResult<Vec<_>>>()?;
            let _: usize = conn.sadd(&key, &members)?;
        }
        ValueType::ZSet => {
            let items: Vec<(Vec<u8>, f64)> = fv_list
                .iter()
                .map(|f| -> AnyResult<_> {
                    Ok((parse_bytes(&f.field_value, &val_fmt)?, f.field_score))
                })
                .collect::<AnyResult<Vec<_>>>()?;
            let pairs: Vec<(f64, Vec<u8>)> = items.into_iter().map(|(m, s)| (s, m)).collect();
            let _: usize = conn.zadd_multiple(&key, &pairs)?;
        }
        ValueType::Stream => {
            let items: Vec<(Vec<u8>, Vec<u8>)> = fv_list
                .iter()
                .map(|f| -> AnyResult<(Vec<u8>, Vec<u8>)> {
                    Ok((
                        parse_bytes(&f.field_key, &val_fmt)?,
                        parse_bytes(&f.field_value, &val_fmt)?,
                    ))
                })
                .collect::<AnyResult<Vec<_>>>()?;
            conn.xadd(&key, &param.stream_id, &items)?
        }
        ValueType::JSON => {
            let value: serde_json::Value =
                serde_json::from_str(&param.value).with_context(|| "json parse error")?;
            conn.json_set(&key, "$", &value)?
        }
        _ => {
            handle_other_value_type(&key_type, &key)?;
        }
    };

    if "key" == mode && param.ttl > 0 {
        let _: () = conn.expire(&key, param.ttl)?;
    }
    Ok(key)
}

pub fn field_set0(
    mut conn: MutexGuard<impl Commands>,
    param: RedisFieldSet,
    httl_supported: bool,
) -> AnyResult<()> {
    let key: RedisKey = param.key;
    let key_type: ValueType = conn.key_type(&key)?;
    let val_fmt = param.val_fmt.as_ref().cloned().unwrap_or_default();

    match key_type {
        ValueType::Hash => {
            // HSET 会清除字段级 TTL；UI 开启时用用户输入，未开启则写前 HTTL、写后 HEXPIRE 补回
            let key_bytes = parse_bytes(&param.field_key, &val_fmt)?;
            let value_bytes = parse_bytes(&param.field_value, &val_fmt)?;
            let include_field_ttl = resolve_include_field_ttl(param.include_field_ttl, httl_supported);
            let preserve_ttl = if httl_supported && !include_field_ttl {
                hash_field_ttl_to_preserve(&mut conn, &key, &key_bytes, httl_supported)?
            } else {
                None
            };
            let _: () = conn.hset(&key, &key_bytes, &value_bytes)?;
            if httl_supported {
                if include_field_ttl && param.field_ttl > 0 {
                    let _: () =
                        conn.hexpire(&key, param.field_ttl, ExpireOption::NONE, &key_bytes)?;
                } else if let Some(ttl) = preserve_ttl {
                    let _: () = conn.hexpire(&key, ttl, ExpireOption::NONE, &key_bytes)?;
                }
            }
        }
        ValueType::List => {
            let bytes = parse_bytes(&param.field_value, &val_fmt)?;
            let _: () = conn.lset(&key, param.field_index, &bytes)?;
        }
        ValueType::Set => {
            let src_bytes = parse_bytes(&param.src_field_value, &val_fmt)?;
            let bytes = parse_bytes(&param.field_value, &val_fmt)?;
            let _: () = conn.srem(&key, &src_bytes)?;
            let _: () = conn.sadd(&key, &bytes)?;
        }
        ValueType::ZSet => {
            let src_bytes = parse_bytes(&param.src_field_value, &val_fmt)?;
            let bytes = parse_bytes(&param.field_value, &val_fmt)?;
            let _: () = conn.zrem(&key, &src_bytes)?;
            let _: () = conn.zadd(&key, &bytes, param.field_score)?;
        }
        _ => {
            handle_other_value_type(&key_type, &key)?;
        }
    };
    Ok(())
}

/// 单条字段读取：Hash→HGET+HTTL，List→LINDEX，ZSet→ZSCORE；Set/Stream 等不支持
pub fn field_get0(
    mut conn: MutexGuard<impl Commands>,
    param: RedisFieldGet,
    httl_supported: bool,
) -> AnyResult<RedisFieldValue> {
    let key: RedisKey = param.key;
    let key_type: ValueType = conn.key_type(&key)?;
    let val_fmt = param.val_fmt.as_ref().cloned().unwrap_or_default();

    match key_type {
        ValueType::Hash => {
            let field_bytes = parse_bytes(&param.field_key, &val_fmt)?;
            let value: Option<Vec<u8>> = conn.hget(&key, &field_bytes)?;
            let value_bytes = value.ok_or_else(|| AppError::FieldNotFound {
                hash_key: param.field_key.clone(),
            })?;
            let mut field_ttl = -1i64;
            let include_field_ttl = resolve_include_field_ttl(param.include_field_ttl, httl_supported);
            if include_field_ttl {
                if let Ok(ttl_values) =
                    conn.httl::<_, _, Vec<IntegerReplyOrNoOp>>(&key, &[&field_bytes])
                {
                    field_ttl = match ttl_values.first() {
                        Some(IntegerReplyOrNoOp::IntegerReply(ttl)) => *ttl as i64,
                        Some(IntegerReplyOrNoOp::NotExists) => -2,
                        Some(IntegerReplyOrNoOp::ExistsButNotRelevant) | None => -1,
                        _ => -1,
                    };
                }
            }
            Ok(RedisFieldValue {
                field_key: format_bytes(&field_bytes, &val_fmt),
                field_value: format_bytes(&value_bytes, &val_fmt),
                field_score: 0.0,
                field_ttl,
            })
        }
        ValueType::List => {
            let value: Option<Vec<u8>> = conn.lindex(&key, param.field_index)?;
            let value_bytes = value.ok_or_else(|| AppError::FieldNotFound {
                hash_key: param.field_index.to_string(),
            })?;
            Ok(RedisFieldValue {
                field_key: String::new(),
                field_value: format_bytes(&value_bytes, &val_fmt),
                field_score: 0.0,
                field_ttl: -1,
            })
        }
        ValueType::ZSet => {
            let member_bytes = parse_bytes(&param.field_value, &val_fmt)?;
            let score: Option<f64> = conn.zscore(&key, &member_bytes)?;
            let score = score.ok_or_else(|| AppError::FieldNotFound {
                hash_key: param.field_value.clone(),
            })?;
            Ok(RedisFieldValue {
                field_key: String::new(),
                field_value: format_bytes(&member_bytes, &val_fmt),
                field_score: score,
                field_ttl: -1,
            })
        }
        _ => {
            handle_other_value_type(&key_type, &key)?;
            unreachable!()
        }
    }
}

/// Hash 全量字段名：HKEYS，按 val_fmt 格式化后返回
pub fn hash_keys0(
    mut conn: MutexGuard<impl Commands>,
    param: RedisHashKeys,
) -> AnyResult<Vec<String>> {
    let key = param.key;
    let key_type: ValueType = conn.key_type(&key)?;
    if key_type != ValueType::Hash {
        handle_other_value_type(&key_type, &key)?;
        unreachable!()
    }
    let val_fmt = param.val_fmt.as_ref().cloned().unwrap_or_default();
    let fields: Vec<Vec<u8>> = redis::cmd("HKEYS").arg(&key).query(&mut conn)?;
    Ok(fields
        .into_iter()
        .map(|f| format_bytes(&f, &val_fmt))
        .collect())
}

/// Hash 全量字段值：HVALS，按 val_fmt 格式化后返回
pub fn hash_values0(
    mut conn: MutexGuard<impl Commands>,
    param: RedisHashKeys,
) -> AnyResult<Vec<String>> {
    let key = param.key;
    let key_type: ValueType = conn.key_type(&key)?;
    if key_type != ValueType::Hash {
        handle_other_value_type(&key_type, &key)?;
        unreachable!()
    }
    let val_fmt = param.val_fmt.as_ref().cloned().unwrap_or_default();
    let values: Vec<Vec<u8>> = redis::cmd("HVALS").arg(&key).query(&mut conn)?;
    Ok(values
        .into_iter()
        .map(|v| format_bytes(&v, &val_fmt))
        .collect())
}

/// List/Set/ZSet 通用弹出：LPOP/RPOP/SPOP/ZPOPMIN/ZPOPMAX
pub fn field_pop0(
    mut conn: MutexGuard<impl Commands>,
    param: RedisPop,
) -> AnyResult<String> {
    let key = param.key;
    let key_type: ValueType = conn.key_type(&key)?;
    let cmd = param.mode.to_uppercase();
    let val_fmt = param.val_fmt.as_ref().cloned().unwrap_or_default();

    // 校验键类型
    let expected = match cmd.as_str() {
        "LPOP" | "RPOP" => ValueType::List,
        "SPOP" => ValueType::Set,
        "ZPOPMIN" | "ZPOPMAX" => ValueType::ZSet,
        other => bail!(AppError::FieldOperationNotSupported {
            mode: other.into()
        }),
    };
    if key_type != expected {
        handle_other_value_type(&key_type, &key)?;
        unreachable!()
    }

    // 执行命令
    match cmd.as_str() {
        "LPOP" | "RPOP" | "SPOP" => {
            let value: Option<Vec<u8>> = redis::cmd(&cmd).arg(&key).query(&mut conn)?;
            Ok(value.map(|v| format_bytes(&v, &val_fmt)).unwrap_or_default())
        }
        "ZPOPMIN" | "ZPOPMAX" => {
            let value: Option<Vec<(Vec<u8>, f64)>> = redis::cmd(&cmd).arg(&key).query(&mut conn)?;
            let result = value.and_then(|mut v| v.pop()).map(|(member, score)| {
                let member_str = format_bytes(&member, &val_fmt);
                format!("{} (score: {})", member_str, score)
            });
            Ok(result.unwrap_or_default())
        }
        _ => unreachable!(),
    }
}

pub fn field_del0(mut conn: MutexGuard<impl Commands>, param: RedisFieldDel) -> AnyResult<()> {
    let key: RedisKey = param.key;
    let key_type: ValueType = conn.key_type(&key)?;
    let val_fmt = param.val_fmt.as_ref().cloned().unwrap_or_default();

    match key_type {
        ValueType::Hash => {
            let field_key = parse_bytes(&param.field_key, &val_fmt)?;
            let _: () = conn.hdel(&key, field_key)?;
        }
        ValueType::List => {
            let _: () = conn.lset(&key, param.field_index, REDIS_ME_FIELD_TO_DELETE_TMP_VALUE)?;
            let _: () = conn.lrem(&key, 1, REDIS_ME_FIELD_TO_DELETE_TMP_VALUE)?;
        }
        ValueType::Set => {
            let bytes = parse_bytes(&param.field_value, &val_fmt)?;
            let _: () = conn.srem(&key, bytes)?;
        }
        ValueType::ZSet => {
            let bytes = parse_bytes(&param.field_value, &val_fmt)?;
            let _: () = conn.zrem(&key, bytes)?;
        }
        ValueType::Stream => {
            let _: () = conn.xdel(&key, &[param.stream_id])?;
        }
        _ => {
            handle_other_value_type(&key_type, &key)?;
        }
    };
    Ok(())
}

pub fn publish0(
    mut conn: MutexGuard<impl Commands>,
    channel: &str,
    message: &str,
    msg_fmt: &BytesFormat,
) -> AnyResult<()> {
    let bytes = parse_bytes(message, msg_fmt)?;
    let _: () = conn.publish(channel, &bytes)?;
    Ok(())
}

/// 将订阅框内容拆成多个 `PSUBSCRIBE` 模式（空白分隔，与 RedisInsight 一致）；无有效模式时等价于 `*`。
fn psubscribe_patterns(channel: Option<String>) -> Vec<String> {
    let Some(raw) = channel.filter(|c| !c.is_empty()) else {
        return vec!["*".into()];
    };
    let mut parts: Vec<String> = raw
        .split_whitespace()
        .map(str::to_string)
        .filter(|p| !p.is_empty())
        .collect();
    if parts.is_empty() {
        vec!["*".into()]
    } else {
        // 添加停止订阅频道, 用于停止订阅时发送消息避免阻塞
        parts.push(REDIS_ME_SUBSCRIBE_STOP_CHANNEL.into());
        parts
    }
}

pub fn subscribe0(
    mut conn: Connection,
    running: Arc<AtomicBool>,
    app_handle: AppHandle,
    channel: Option<String>,
    id: String,
    logger: Arc<CommandLogger>,
) -> AnyResult<()> {
    set_client_name(&mut conn);
    running.store(true, Relaxed);

    let patterns = psubscribe_patterns(channel);

    let _: JoinHandle<AnyResult<()>> = thread::spawn(move || {
        let cmd = redis::cmd("PSUBSCRIBE").arg(&patterns).get_packed_command();
        let start = std::time::Instant::now();
        conn.send_packed_command(&cmd)?;
        logger.log_raw(
            0,
            "PSUBSCRIBE",
            &patterns,
            None,
            start.elapsed().as_millis() as u64,
        );
        info!("subscribe start: {:?}", patterns);
        while running.load(Relaxed) {
            let response = conn.recv_response()?;
            if let Some(msg) = Msg::from_value(&response) {
                let payload: Vec<u8> = msg.get_payload()?;
                let event = SubscribeEvent {
                    id: id.clone(),
                    datetime: Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
                    channel: msg.get_channel_name().to_string(),
                    message: vec8_to_display_string(&payload),
                };
                let _ = &app_handle.emit(EVENT_SUBSCRIBE, event);
            }
        }
        info!("subscribe end: {:?}", patterns);
        Ok(())
    });
    Ok(())
}

pub fn subscribe_stop0(conn: MutexGuard<impl Commands>, running: Arc<AtomicBool>) -> AnyResult<()> {
    running.store(false, Relaxed);
    // 停止订阅时必须发送一个消息，否则会阻塞
    publish0(
        conn,
        REDIS_ME_SUBSCRIBE_STOP_CHANNEL,
        REDIS_ME_SUBSCRIBE_STOP_CHANNEL,
        &BytesFormat::UTF8,
    )
}

pub fn monitor0(
    mut conn: Connection,
    running: Arc<AtomicBool>,
    app_handle: AppHandle,
    id: String,
    logger: Arc<CommandLogger>,
) -> AnyResult<()> {
    set_client_name(&mut conn);
    running.store(true, Relaxed);

    let _: JoinHandle<AnyResult<()>> = thread::spawn(move || {
        let start = std::time::Instant::now();
        conn.send_packed_command(&redis::cmd("MONITOR").get_packed_command())?;
        logger.log_raw(0, "MONITOR", &[], None, start.elapsed().as_millis() as u64);
        info!("monitor start");
        while running.load(Relaxed) {
            let response = conn.recv_response()?;
            let command: String = from_redis_value(response)?;
            let event = MonitorEvent {
                id: id.clone(),
                datetime: Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
                command,
            };
            let _ = &app_handle.emit(EVENT_MONITOR, event);
        }
        info!("monitor end");
        Ok(())
    });

    Ok(())
}

pub fn monitor_stop0(running: Arc<AtomicBool>) -> AnyResult<()> {
    info!("monitor stop");
    running.store(false, Relaxed);
    Ok(())
}

fn handle_other_value_type(value_type: &ValueType, key: &RedisKey) -> AnyResult<serde_json::Value> {
    match value_type {
        ValueType::Unknown(other) => {
            if "none" == other {
                bail!(AppError::KeyNotFound {
                    key: vec8_to_display_string(key.to_bytes())
                })
            } else {
                bail!(AppError::KeyTypeUnknown {
                    value_type: other.into()
                })
            }
        }
        //ValueType::Stream => bail!("Unsupported Type: Stream"),
        _ => bail!(AppError::KeyTypeUnsupported {
            value_type: format!("{:?}", value_type)
        }),
    }
}

pub fn batch_key0(
    rmc: &impl MeClient,
    param: RedisBatchKey,
    assert_not_empty: bool,
) -> AnyResult<Vec<RedisKey>> {
    let key_list = if param.key_list.is_empty() {
        if param.pattern.is_empty() {
            bail!(AppError::EmptyParameters)
        }
        let scan_result = rmc.scan(ScanParam::all(param.pattern))?;
        info!("scan key count: {}", scan_result.key_list.len());
        scan_result.key_list
    } else {
        param.key_list
    };

    if assert_not_empty && key_list.is_empty() {
        bail!(AppError::EmptyKeyList)
    }

    Ok(key_list)
}

pub fn export_import_check_running(running: Arc<AtomicBool>) -> AnyResult<()> {
    if running.load(Relaxed) {
        bail!(AppError::ExportImportRunning)
    }
    running.store(true, Relaxed);
    Ok(())
}

pub fn export_csv_0_thread(
    conn: &mut impl Commands,
    key_list: Vec<RedisKey>,
    file: String,
    with_ttl: bool,
    running: Arc<AtomicBool>,
    app_handle: AppHandle,
    id: String,
) {
    info!("export keys count: {}", key_list.len());
    let result = export_keys(
        conn,
        key_list,
        &file,
        with_ttl,
        running.clone(),
        app_handle,
        id,
    );
    match result {
        Ok(_) => info!("export keys ok"),
        Err(e) => warn!("export keys err: {e}"),
    }
    running.store(false, Relaxed);
}

pub fn export_cmd_0_thread(
    conn: &mut impl Commands,
    key_list: Vec<RedisKey>,
    file: String,
    with_ttl: bool,
    running: Arc<AtomicBool>,
    app_handle: AppHandle,
    id: String,
) {
    info!("export cmd keys count: {}", key_list.len());
    let result = export_keys_as_command(
        conn,
        key_list,
        &file,
        with_ttl,
        running.clone(),
        app_handle,
        id,
    );
    match result {
        Ok(_) => info!("export cmd keys ok"),
        Err(e) => warn!("export cmd keys err: {e}"),
    }
    running.store(false, Relaxed);
}

fn export_keys_as_command(
    mut conn: impl Commands,
    key_list: Vec<RedisKey>,
    file: &str,
    with_ttl: bool,
    running: Arc<AtomicBool>,
    app_handle: AppHandle,
    id: String,
) -> AnyResult<()> {
    info!("export cmd file: {}", file);
    let mut writer = BufWriter::new(File::create(file)?);
    let mut ok_count = 0;
    let mut err_count = 0;
    let total_count = key_list.len() as u64;
    for key in key_list {
        if running.load(Relaxed) {
            let result = export_key_as_command(&mut conn, &mut writer, key, with_ttl);
            match result {
                Ok(true) => ok_count += 1,
                Ok(false) => err_count += 1,
                Err(e) => {
                    warn!("export cmd key err: {e}");
                    err_count += 1;
                }
            }
            let event = ExportImportEvent {
                id: id.clone(),
                ok_count,
                err_count,
                total_count,
                ignore_count: 0,
                finished: false,
            };
            let _ = &app_handle.emit(EVENT_EXPORT, event);
        }
    }

    let event = ExportImportEvent {
        id: id.clone(),
        ok_count,
        err_count,
        total_count,
        ignore_count: 0,
        finished: true,
    };
    let _ = &app_handle.emit(EVENT_EXPORT, event);
    writer.flush()?;
    Ok(())
}

/// 写入单键命令行；返回 Ok(true) 表示有内容写出
fn export_key_as_command(
    conn: &mut impl Commands,
    writer: &mut BufWriter<File>,
    key: RedisKey,
    with_ttl: bool,
) -> AnyResult<bool> {
    let key_bytes = key.to_bytes();
    let lines = key_as_command_lines(conn, &key)?;
    if lines.is_empty() {
        return Ok(false);
    }
    for line in &lines {
        writeln!(writer, "{line}")?;
    }
    if with_ttl {
        let ttl = conn.ttl(&key)?;
        if ttl > 0 {
            writeln!(writer, "{}", format_expire_command(&key_bytes, ttl))?;
        }
    }
    Ok(true)
}

fn export_keys(
    mut conn: impl Commands,
    key_list: Vec<RedisKey>,
    file: &str,
    with_ttl: bool,
    running: Arc<AtomicBool>,
    app_handle: AppHandle,
    id: String,
) -> AnyResult<()> {
    info!("export keys file: {}", file);
    let mut writer = BufWriter::new(File::create(file)?);
    let mut ok_count = 0;
    let mut err_count = 0;
    let total_count = key_list.len() as u64;
    for key in key_list {
        if running.load(Relaxed) {
            let result = export_key(&mut conn, &mut writer, key, with_ttl);
            match result {
                Ok(_) => ok_count += 1,
                Err(e) => {
                    warn!("export key err: {e}");
                    err_count += 1;
                }
            };
            // 通知导出进度
            let event = ExportImportEvent {
                id: id.clone(),
                ok_count,
                err_count,
                total_count,
                ignore_count: 0,
                finished: false,
            };
            let _ = &app_handle.emit(EVENT_EXPORT, event);
        }
    }

    let event = ExportImportEvent {
        id: id.clone(),
        ok_count,
        err_count,
        total_count,
        ignore_count: 0,
        finished: true,
    };
    let _ = &app_handle.emit(EVENT_EXPORT, event);
    writer.flush()?;
    Ok(())
}

fn export_key(
    conn: &mut impl Commands,
    writer: &mut BufWriter<File>,
    key: RedisKey,
    with_ttl: bool,
) -> AnyResult<()> {
    let ttl = if with_ttl { conn.ttl(&key)? } else { -1 };

    // https://redis.ac.cn/docs/latest/commands/dump/
    // DUMP key
    let bytes: Vec<u8> = redis::cmd("dump").arg(&key).query(conn)?;
    let key = BASE64_STANDARD.encode(key.to_bytes());
    let value = BASE64_STANDARD.encode(&bytes);
    // 文件写入一行
    writeln!(writer, "{key},{value},{ttl}")?;
    Ok(())
}

pub fn import_csv_0_thread(
    conn: &mut impl Commands,
    param: RedisImportCsv,
    running: Arc<AtomicBool>,
    app_handle: AppHandle,
    id: String,
) {
    info!("import csv file: {}", &param.file);
    let result = import_keys(conn, param, running.clone(), app_handle, id);
    match result {
        Ok(_) => info!("import csv file ok"),
        Err(e) => warn!("import csv file err: {e}"),
    }
    running.store(false, Relaxed);
}

fn import_keys(
    conn: &mut impl Commands,
    param: RedisImportCsv,
    running: Arc<AtomicBool>,
    app_handle: AppHandle,
    id: String,
) -> AnyResult<()> {
    let reader = BufReader::new(File::open(&param.file)?);
    let total_count = reader.lines().count() as u64;
    info!("import keys count: {}", total_count);

    let mut ok_count = 0;
    let mut err_count = 0;
    let mut ignore_count = 0;

    let reader = BufReader::new(File::open(&param.file)?);
    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if running.load(Relaxed) {
            let result = import_key(
                conn,
                line,
                param.ttl,
                &param.handle_ttl,
                &param.handle_conflict,
            );
            match result {
                Ok(_) => ok_count += 1,
                Err(e) => {
                    // 文档说明: RESTORE will return a "Target key name is busy" error when key already exists unless you use the REPLACE modifier.
                    // 实际测试: Redis 8.4.0返回的错误: "BUSYKEY": Target key name already exists.
                    if e.to_string().contains("Target key name") {
                        ignore_count += 1;
                    } else {
                        warn!("import key err: {e}");
                        err_count += 1
                    }
                }
            };
            // 通知导入进度
            let event = ExportImportEvent {
                id: id.clone(),
                ok_count,
                err_count,
                total_count,
                ignore_count,
                finished: false,
            };
            let _ = &app_handle.emit(EVENT_IMPORT, event);
        }
    }

    let event = ExportImportEvent {
        id: id.clone(),
        ok_count,
        err_count,
        total_count,
        ignore_count,
        finished: true,
    };
    let _ = &app_handle.emit(EVENT_IMPORT, event);
    Ok(())
}

fn import_key(
    conn: &mut impl Commands,
    line: &str,
    ttl: i64,
    handle_ttl: &str,
    handle_conflict: &str,
) -> AnyResult<()> {
    let parts: Vec<&str> = line.split(',').collect();
    if parts.len() != 2 && parts.len() != 3 {
        bail!(AppError::ImportInvalidLine { line: line.into() })
    }

    let ttl_part = if parts.len() == 3 { parts[2] } else { "-1" };

    // https://redis.ac.cn/docs/latest/commands/restore/
    // RESTORE key ttl serialized-value [REPLACE] [ABSTTL] [IDLETIME seconds] [FREQ frequency]
    // 如果 ttl 为 0，则创建键时不设置过期时间；否则，设置指定的过期时间（以毫秒为单位）。
    // 除非使用 REPLACE 修饰符，否则当 key 已存在时，RESTORE 将返回“Target key name is busy”错误。
    let key = BASE64_STANDARD.decode(parts[0])?;
    let value = BASE64_STANDARD.decode(parts[1])?;
    let ttl = import_restore_ttl(ttl_part, ttl, handle_ttl);

    let mut cmd = redis::cmd("restore");
    cmd.arg(&key).arg(ttl).arg(value);
    if handle_conflict == "replace" {
        cmd.arg("replace");
    }
    let _: () = cmd.query(conn)?;
    Ok(())
}

fn import_restore_ttl(part_ttl: &str, ttl: i64, handle_ttl: &str) -> i64 {
    let ttl = match handle_ttl {
        "custom" => ttl,
        "parse" => part_ttl.parse::<i64>().unwrap_or(-1),
        _ => -1,
    };

    // 注意: 导出时TTL命令返回的单位是秒, restore的ttl参数是毫秒
    if ttl <= 0 { 0 } else { ttl * 1000 }
}

pub fn import_cmd_0_thread(
    conn: &mut impl Commands,
    file: String,
    running: Arc<AtomicBool>,
    app_handle: AppHandle,
    id: String,
) {
    info!("import cmd file: {}", &file);
    let result = import_cmds(conn, file, running.clone(), app_handle, id);
    match result {
        Ok(_) => info!("import cmd file ok"),
        Err(e) => warn!("import cmd file err: {e}"),
    }
    running.store(false, Relaxed);
}

fn import_cmds(
    conn: &mut impl Commands,
    file: String,
    running: Arc<AtomicBool>,
    app_handle: AppHandle,
    id: String,
) -> AnyResult<()> {
    let reader = BufReader::new(File::open(&file)?);
    let total_count = reader.lines().count() as u64;
    info!("import cmds lines: {}", total_count);

    let mut ok_count = 0;
    let mut err_count = 0;

    let reader = BufReader::new(File::open(&file)?);
    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if running.load(Relaxed) {
            let result = import_cmd(conn, line);
            match result {
                Ok(_) => ok_count += 1,
                Err(e) => {
                    warn!("import cmd err: {e}");
                    err_count += 1
                }
            }
            // 通知导入进度
            let event = ExportImportEvent {
                id: id.clone(),
                ok_count,
                err_count,
                total_count,
                ignore_count: 0,
                finished: false,
            };
            let _ = &app_handle.emit(EVENT_IMPORT, event);
        }
    }

    let event = ExportImportEvent {
        id: id.clone(),
        ok_count,
        err_count,
        total_count,
        ignore_count: 0,
        finished: true,
    };
    let _ = &app_handle.emit(EVENT_IMPORT, event);
    Ok(())
}

fn import_cmd(mut conn: &mut impl Commands, line: &str) -> AnyResult<()> {
    // 命令日志已经输出，这里不再输出
    //info!("line: {}", line);
    let (cmd, args) = parse_command(line)?;
    redis::cmd(cmd.as_str()).arg(args).exec(&mut conn)?;
    Ok(())
}

pub fn key_type0(mut conn: MutexGuard<impl Commands>, key: RedisKey) -> AnyResult<String> {
    // 简单字符串回复：key 的类型，如果 key 不存在则返回 none
    let key_type: ValueType = conn.key_type(&key)?;
    Ok(ui_key_type(key_type))
}

/// 单键 → redis-cli 可执行命令行列表（全量读取，与键值页 fieldScan 分页无关）
fn key_as_command_lines(conn: &mut impl Commands, key: &RedisKey) -> AnyResult<Vec<String>> {
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
            format_rpush_command(key_bytes, std::slice::from_ref(&value_bytes)).ok_or_else(|| {
                AppError::Internal {
                    message: "empty list element".into(),
                }
            })?
        }
        ValueType::Set => {
            let member_bytes = parse_bytes(&param.field_value, &val_fmt)?;
            format_sadd_command(key_bytes, std::slice::from_ref(&member_bytes)).ok_or_else(|| {
                AppError::Internal {
                    message: "empty set member".into(),
                }
            })?
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
            let (id, fields) = entries.first().ok_or_else(|| AppError::FieldNotFoundStream {
                stream_id: param.stream_id.clone(),
            })?;
            format_xadd_command(key_bytes, id, fields)
        }
        other => bail!(AppError::KeyTypeUnsupported {
            value_type: ui_key_type(other)
        }),
    };
    Ok(line)
}

pub fn xinfo_groups0(
    mut conn: MutexGuard<impl Commands>,
    key: RedisKey,
) -> AnyResult<Vec<XInfoGroup>> {
    let reply: StreamInfoGroupsReply = conn.xinfo_groups(&key)?;
    Ok(reply.groups.into_iter().map(ui_xinfo_group).collect())
}

pub fn xinfo_consumers0(
    mut conn: MutexGuard<impl Commands>,
    key: RedisKey,
    group: String,
) -> AnyResult<Vec<XInfoConsumer>> {
    let reply: StreamInfoConsumersReply = conn.xinfo_consumers(&key, &group)?;
    Ok(reply.consumers.into_iter().map(ui_xinfo_consumer).collect())
}

pub fn flush_db0(mut conn: MutexGuard<impl Commands>) -> AnyResult<()> {
    let _: () = conn.flushdb()?;
    Ok(())
}

pub fn flush_all0(mut conn: MutexGuard<impl Commands>) -> AnyResult<()> {
    let _: () = conn.flushall()?;
    Ok(())
}

pub(crate) fn acl_rule_to_string(rule: Rule) -> String {
    match rule {
        Rule::On => "on".into(),
        Rule::Off => "off".into(),
        Rule::AllCommands => "allcommands".into(),
        Rule::NoCommands => "nocommands".into(),
        Rule::NoPass => "nopass".into(),
        Rule::AllKeys => "allkeys".into(),
        Rule::ResetKeys => "resetkeys".into(),
        Rule::ResetChannels => "resetchannels".into(),
        Rule::ResetPass => "resetpass".into(),
        Rule::Reset => "reset".into(),
        Rule::AddCommand(cmd) => format!("+{cmd}"),
        Rule::RemoveCommand(cmd) => format!("-{cmd}"),
        Rule::AddCategory(cat) => format!("+@{cat}"),
        Rule::RemoveCategory(cat) => format!("-@{cat}"),
        Rule::AddPass(pass) => format!(">{pass}"),
        Rule::RemovePass(pass) => format!("<{pass}"),
        Rule::AddHashedPass(hash) => hash,
        Rule::RemoveHashedPass(hash) => format!("!{hash}"),
        Rule::Pattern(pattern) => pattern,
        Rule::Channel(pattern) => pattern,
        Rule::Selector(selector) => selector
            .into_iter()
            .map(acl_rule_to_string)
            .collect::<Vec<_>>()
            .join(" "),
        Rule::Other(raw) => raw,
        _ => "unknown".into(),
    }
}

/// ACL SETUSER 单条规则参数（集群广播 route_command 用）
pub(crate) fn acl_rule_to_setuser_arg(rule: &Rule) -> String {
    match rule {
        Rule::NoPass => "nopass".into(),
        Rule::Reset => "reset".into(),
        Rule::ResetPass => "resetpass".into(),
        Rule::AddHashedPass(hash) => format!("#{hash}"),
        Rule::Selector(inner) => format!("({})", acl_rules_to_selector_text(inner)),
        other => acl_rule_to_setuser_text(other),
    }
}

pub fn build_acl_setuser_cmd(param: &AclSetuserParam) -> AnyResult<redis::Cmd> {
    let rules = acl_build_rules(param)?;
    let mut cmd = redis::cmd("ACL");
    cmd.arg("SETUSER").arg(&param.username);
    for rule in &rules {
        cmd.arg(acl_rule_to_setuser_arg(rule));
    }
    Ok(cmd)
}
fn acl_rule_to_setuser_text(rule: &Rule) -> String {
    match rule {
        Rule::On => "on".into(),
        Rule::Off => "off".into(),
        Rule::AllCommands => "allcommands".into(),
        Rule::NoCommands => "nocommands".into(),
        Rule::AllKeys => "allkeys".into(),
        Rule::ResetKeys => "resetkeys".into(),
        Rule::ResetChannels => "resetchannels".into(),
        Rule::AddCommand(cmd) => format!("+{cmd}"),
        Rule::RemoveCommand(cmd) => format!("-{cmd}"),
        Rule::AddCategory(cat) => format!("+@{cat}"),
        Rule::RemoveCategory(cat) => format!("-@{cat}"),
        Rule::Pattern(pat) => format!("~{pat}"),
        Rule::Channel(pat) if pat == "*" => "allchannels".into(),
        Rule::Channel(pat) => format!("&{pat}"),
        Rule::Other(raw) => raw.clone(),
        Rule::Selector(inner) => format!("({})", acl_rules_to_selector_text(inner)),
        _ => "unknown".into(),
    }
}

fn acl_rules_to_selector_text(rules: &[Rule]) -> String {
    rules
        .iter()
        .map(acl_rule_to_setuser_text)
        .collect::<Vec<_>>()
        .join(" ")
}

fn get_getuser_field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    if let Some(map_iter) = value.as_map_iter() {
        for (name, val) in map_iter {
            if getuser_key_name(name).as_deref() == Some(key) {
                return Some(val);
            }
        }
    } else if let Some(seq) = value.as_sequence() {
        if seq.len().is_multiple_of(2) {
            for chunk in seq.chunks(2) {
                if getuser_key_name(&chunk[0]).as_deref() == Some(key) {
                    return Some(&chunk[1]);
                }
            }
        }
    }
    None
}

fn getuser_key_name(value: &Value) -> Option<String> {
    match value {
        Value::BulkString(b) => {
            let mut s = String::from_utf8_lossy(b).trim().to_string();
            if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
                s = s[1..s.len() - 1].to_string();
            }
            Some(s)
        }
        Value::SimpleString(s) => Some(s.trim().to_string()),
        _ => None,
    }
}

/// 从 ACL GETUSER 原始响应解析 selectors（按条分组，避免 redis-rs flatten 丢结构）
fn parse_acl_selectors_from_getuser(value: &Value) -> AnyResult<Vec<String>> {
    let Some(selectors_value) = get_getuser_field(value, "selectors") else {
        return Ok(vec![]);
    };
    let arr = match selectors_value {
        Value::Array(arr) | Value::Set(arr) => arr,
        _ => return Ok(vec![]),
    };
    Ok(arr
        .iter()
        .map(selector_item_to_text)
        .filter(|text| !text.is_empty())
        .collect())
}

fn selector_item_to_text(item: &Value) -> String {
    let info = match redis::acl::AclInfo::from_redis_value_ref(item) {
        Ok(info) => info,
        Err(_) => return String::new(),
    };
    let rules: Vec<Rule> = info
        .flags
        .into_iter()
        .chain(info.commands)
        .chain(info.keys)
        .chain(info.channels)
        .collect();
    acl_rules_to_selector_text(&rules)
}

fn acl_selector_token_to_rule(token: &str) -> Rule {
    let v = token.trim();
    if v.is_empty() {
        return Rule::Other(String::new());
    }
    match v.to_ascii_lowercase().as_str() {
        "allkeys" => Rule::AllKeys,
        "resetkeys" => Rule::ResetKeys,
        "allchannels" => Rule::Other("allchannels".into()),
        "resetchannels" => Rule::ResetChannels,
        "allcommands" => Rule::AllCommands,
        "nocommands" => Rule::NoCommands,
        "on" => Rule::On,
        "off" => Rule::Off,
        _ if v.starts_with("+@")
            || v.starts_with("-@")
            || v.starts_with('+')
            || v.starts_with('-') =>
        {
            acl_rule_from_text(v)
        }
        _ if v.starts_with('~') => acl_key_rule_from_text(v),
        _ if v.starts_with('&') => acl_channel_rule_from_text(v),
        _ => Rule::Other(v.into()),
    }
}

fn acl_selector_from_text(text: &str) -> AnyResult<Rule> {
    let trimmed = text.trim();
    let inner = trimmed
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(trimmed)
        .trim();
    if inner.is_empty() {
        bail!("empty ACL selector");
    }
    let tokens = split_redis_args(inner)?;
    let rules: Vec<Rule> = tokens
        .iter()
        .map(|t| acl_selector_token_to_rule(&String::from_utf8_lossy(t)))
        .collect();
    Ok(Rule::Selector(rules))
}

fn acl_rule_from_text(text: &str) -> Rule {
    let v = text.trim();
    if let Some(cmd) = v.strip_prefix("+@") {
        return Rule::AddCategory(cmd.into());
    }
    if let Some(cmd) = v.strip_prefix("-@") {
        return Rule::RemoveCategory(cmd.into());
    }
    if let Some(cmd) = v.strip_prefix('+') {
        return Rule::AddCommand(cmd.into());
    }
    if let Some(cmd) = v.strip_prefix('-') {
        return Rule::RemoveCommand(cmd.into());
    }
    Rule::Other(v.into())
}

fn acl_key_rule_from_text(text: &str) -> Rule {
    let v = text.trim().trim_start_matches('~');
    match v.to_ascii_lowercase().as_str() {
        "allkeys" | "*" => Rule::AllKeys,
        "resetkeys" => Rule::ResetKeys,
        _ => Rule::Pattern(v.into()),
    }
}

fn acl_channel_rule_from_text(text: &str) -> Rule {
    let v = text.trim().trim_start_matches('&');
    match v.to_ascii_lowercase().as_str() {
        "allchannels" | "*" => Rule::Other("allchannels".into()),
        "resetchannels" => Rule::ResetChannels,
        _ => Rule::Channel(v.into()),
    }
}

pub(crate) fn acl_build_rules(param: &AclSetuserParam) -> AnyResult<Vec<Rule>> {
    let mut rules = vec![Rule::Reset];
    rules.push(if param.enabled { Rule::On } else { Rule::Off });

    // 密码保持规则：
    // - 新密码由前端转换为 hash 回传（若无变更会回传原 hashes）
    // - 全部为空时显式 nopass，避免 reset 后无密码且无法登录
    if param.password_hashes.is_empty() {
        rules.push(Rule::NoPass);
    } else {
        rules.extend(
            param
                .password_hashes
                .iter()
                .cloned()
                .map(Rule::AddHashedPass),
        );
    }

    // 命令规则未配置时，默认拒绝所有命令（reset 已含 -@all，这里显式写入增强可读性）
    if param.command_rules.is_empty() {
        rules.push(Rule::NoCommands);
    } else {
        rules.extend(
            param
                .command_rules
                .iter()
                .map(|x| acl_rule_from_text(x)),
        );
    }

    if param.key_patterns.is_empty() {
        rules.push(Rule::AllKeys);
    } else {
        rules.extend(
            param
                .key_patterns
                .iter()
                .map(|x| acl_key_rule_from_text(x)),
        );
    }

    if param.channel_patterns.is_empty() {
        rules.push(Rule::ResetChannels);
    } else {
        rules.extend(
            param
                .channel_patterns
                .iter()
                .map(|x| acl_channel_rule_from_text(x)),
        );
    }

    // 编辑保存时回写 selectors（与表单 selectors 字段一致）
    for selector in &param.selectors {
        let text = selector.trim();
        if text.is_empty() {
            continue;
        }
        rules.push(acl_selector_from_text(text)?);
    }
    Ok(rules)
}

pub(crate) fn acl_user_detail_from_info(
    username: &str,
    info: redis::acl::AclInfo,
    selectors: Vec<String>,
) -> AclUserDetail {
    let mut enabled = false;
    let mut nopass = false;
    let mut flags = Vec::with_capacity(info.flags.len());
    for flag in info.flags {
        match &flag {
            Rule::On => enabled = true,
            Rule::NoPass => nopass = true,
            _ => {}
        }
        flags.push(acl_rule_to_string(flag));
    }

    let password_hashes = info.passwords.into_iter().map(acl_rule_to_string).collect();
    let command_rules = info.commands.into_iter().map(acl_rule_to_string).collect();
    let key_patterns = info.keys.into_iter().map(acl_rule_to_string).collect();
    let channel_patterns = info.channels.into_iter().map(acl_rule_to_string).collect();

    AclUserDetail {
        username: username.into(),
        enabled,
        nopass,
        flags,
        password_hashes,
        command_rules,
        key_patterns,
        channel_patterns,
        selectors,
    }
}

/// ACL LIST 行内规则分词：保留 `(+set ~key)` 等 selector 整段
fn tokenize_acl_list_rule_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut i = 0;
    let bytes = text.as_bytes();
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        if bytes[i] == b'(' {
            let start = i;
            let mut depth = 0;
            while i < bytes.len() {
                if bytes[i] == b'(' {
                    depth += 1;
                }
                if bytes[i] == b')' {
                    depth -= 1;
                    if depth == 0 {
                        i += 1;
                        break;
                    }
                }
                i += 1;
            }
            tokens.push(text[start..i].to_string());
        } else {
            let start = i;
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            tokens.push(text[start..i].to_string());
        }
    }
    tokens
}

fn list_key_to_pattern(token: &str) -> String {
    let v = token.trim().trim_start_matches('~');
    match v.to_ascii_lowercase().as_str() {
        "allkeys" => "allkeys".into(),
        "*" => "*".into(),
        _ => v.into(),
    }
}

fn list_channel_to_pattern(token: &str) -> String {
    let v = token.trim().trim_start_matches('&');
    match v.to_ascii_lowercase().as_str() {
        "allchannels" => "allchannels".into(),
        "*" => "*".into(),
        _ => v.into(),
    }
}

fn selector_token_to_text(token: &str) -> String {
    let trimmed = token.trim();
    trimmed
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(trimmed)
        .trim()
        .to_string()
}

fn is_acl_list_command_rule(token: &str) -> bool {
    token.starts_with("+@")
        || token.starts_with("-@")
        || (token.starts_with('+') && token.len() > 1)
        || (token.starts_with('-') && token.len() > 1)
}

fn is_acl_list_key_rule(token: &str) -> bool {
    token.starts_with('~')
        || matches!(
            token.to_ascii_lowercase().as_str(),
            "allkeys" | "resetkeys" | "*"
        )
}

fn is_acl_list_channel_rule(token: &str) -> bool {
    token.starts_with('&')
        || matches!(
            token.to_ascii_lowercase().as_str(),
            "allchannels" | "resetchannels"
        )
}

/// 解析 ACL LIST 单行 `user <name> <rules...>` 为 AclUserDetail
pub(crate) fn parse_acl_list_line(line: &str) -> AnyResult<AclUserDetail> {
    let line = line.trim();
    let rest = line
        .strip_prefix("user ")
        .ok_or_else(|| anyhow::anyhow!("invalid ACL LIST line: {line}"))?;
    let tokens = tokenize_acl_list_rule_tokens(rest);
    let username = tokens
        .first()
        .ok_or_else(|| anyhow::anyhow!("ACL LIST line missing username: {line}"))?
        .clone();

    let mut enabled = false;
    let mut nopass = false;
    let mut flags = Vec::new();
    let mut password_hashes = Vec::new();
    let mut command_rules = Vec::new();
    let mut key_patterns = Vec::new();
    let mut channel_patterns = Vec::new();
    let mut selectors = Vec::new();

    for token in tokens.iter().skip(1) {
        if token == "on" {
            enabled = true;
            flags.push("on".into());
        } else if token == "off" {
            enabled = false;
            flags.push("off".into());
        } else if token == "nopass" {
            nopass = true;
            flags.push("nopass".into());
        } else if let Some(hash) = token.strip_prefix('#') {
            password_hashes.push(hash.to_string());
        } else if token.starts_with('(') {
            let text = selector_token_to_text(token);
            if !text.is_empty() {
                selectors.push(text);
            }
        } else if is_acl_list_command_rule(token) {
            command_rules.push(token.clone());
        } else if is_acl_list_key_rule(token) {
            key_patterns.push(list_key_to_pattern(token));
        } else if is_acl_list_channel_rule(token) {
            channel_patterns.push(list_channel_to_pattern(token));
        } else {
            flags.push(token.clone());
            if token == "nopass" {
                nopass = true;
            }
        }
    }

    Ok(AclUserDetail {
        username,
        enabled,
        nopass,
        flags,
        password_hashes,
        command_rules,
        key_patterns,
        channel_patterns,
        selectors,
    })
}

pub fn acl_list_users0(mut conn: MutexGuard<impl Commands>) -> AnyResult<Vec<AclUserDetail>> {
    let lines: Vec<String> = conn.acl_list()?;
    let mut users = Vec::with_capacity(lines.len());
    for line in lines {
        let line = line.trim();
        if line.is_empty() || !line.starts_with("user ") {
            continue;
        }
        users.push(parse_acl_list_line(line)?);
    }
    users.sort_by(|a, b| a.username.cmp(&b.username));
    Ok(users)
}

pub fn acl_getuser0(
    mut conn: MutexGuard<impl Commands>,
    username: &str,
) -> AnyResult<AclUserDetail> {
    let raw: Value = redis::cmd("ACL")
        .arg("GETUSER")
        .arg(username)
        .query(&mut *conn)?;
    let info: Option<redis::acl::AclInfo> = FromRedisValue::from_redis_value(raw.clone())?;
    let info = info.ok_or_else(|| anyhow::anyhow!("ACL user not found: {username}"))?;
    let selectors = parse_acl_selectors_from_getuser(&raw)?;

    Ok(acl_user_detail_from_info(username, info, selectors))
}

pub fn acl_users0(mut conn: MutexGuard<impl Commands>) -> AnyResult<Vec<String>> {
    Ok(conn.acl_users()?)
}

pub fn acl_whoami0(mut conn: MutexGuard<impl Commands>) -> AnyResult<String> {
    Ok(conn.acl_whoami()?)
}

pub fn acl_cat0(
    mut conn: MutexGuard<impl Commands>,
    category: Option<String>,
) -> AnyResult<Vec<String>> {
    let set: HashSet<String> = match category.filter(|x| !x.is_empty()) {
        Some(cat) => conn.acl_cat_categoryname(cat)?,
        None => conn.acl_cat()?,
    };
    let mut list: Vec<String> = set.into_iter().collect();
    list.sort();
    Ok(list)
}

pub fn acl_genpass0(
    mut conn: MutexGuard<impl Commands>,
    bits: Option<i64>,
) -> AnyResult<String> {
    if let Some(v) = bits {
        Ok(conn.acl_genpass_bits(v as isize)?)
    } else {
        Ok(conn.acl_genpass()?)
    }
}

/// ACL LOG 单条：Redis 返回扁平 key/value 数组
fn parse_acl_log_entry(value: Value) -> AnyResult<AclLogEntry> {
    let pairs = match value {
        Value::Array(arr) => arr,
        _ => bail!("ACL log entry should be an array"),
    };

    let mut log_entry = AclLogEntry::default();
    let mut i = 0;
    while i + 1 < pairs.len() {
        let key = redis_value_to_string(pairs[i].clone(), "");
        let val = pairs[i + 1].clone();
        match key.as_str() {
            "count" => {
                if let Value::Int(c) = val {
                    log_entry.count = c as u64;
                }
            }
            "reason" => log_entry.reason = acl_log_value_to_string(val),
            "context" => log_entry.context = acl_log_value_to_string(val),
            "object" => log_entry.object = acl_log_value_to_string(val),
            "username" => log_entry.username = acl_log_value_to_string(val),
            "age-seconds" => {
                if let Ok(a) = acl_log_value_to_string(val).parse::<f64>() {
                    log_entry.age_seconds = a;
                }
            }
            "client-info" => log_entry.client_info = acl_log_value_to_string(val),
            "entry-id" => {
                if let Value::Int(id) = val {
                    log_entry.entry_id = id as u64;
                }
            }
            "timestamp-created" => {
                if let Value::Int(t) = val {
                    log_entry.timestamp_created = t as u64;
                }
            }
            "timestamp-last-updated" | "timestamp-last" => {
                if let Value::Int(t) = val {
                    log_entry.timestamp_last_updated = t as u64;
                }
            }
            _ => {}
        }
        i += 2;
    }
    Ok(log_entry)
}

fn acl_log_value_to_string(value: Value) -> String {
    match value {
        Value::BulkString(b) => String::from_utf8_lossy(&b).to_string(),
        Value::SimpleString(s) => s,
        Value::Int(i) => i.to_string(),
        other => redis_value_to_string(other, " "),
    }
}

/// ACL LOG: 获取 ACL 安全日志
pub fn acl_log0(
    mut conn: MutexGuard<impl Commands>,
    count: Option<u64>,
) -> AnyResult<Vec<AclLogEntry>> {
    let count = count.unwrap_or(10) as isize;
    let value: Value = redis::cmd("ACL")
        .arg("LOG")
        .arg(count)
        .query(&mut *conn)?;

    match value {
        Value::Array(entries) => entries.into_iter().map(parse_acl_log_entry).collect(),
        _ => bail!("ACL LOG response should be an array"),
    }
}

/// ACL DRYRUN: 模拟执行命令，检查用户权限
pub fn acl_dryrun0(
    mut conn: MutexGuard<impl Commands>,
    username: String,
    command: String,
) -> AnyResult<String> {
    // 解析命令字符串为命令名和参数
    let (cmd_name, cmd_args) = parse_command(&command)?;
    
    if cmd_name.is_empty() {
        return Err(anyhow::anyhow!("Command cannot be empty"));
    }
    
    // 使用 redis-rs 内置的 acl_dryrun 方法
    let cmd_args: Vec<String> = cmd_args
        .iter()
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .collect();
    let result: String = conn.acl_dryrun(&username, &cmd_name, &cmd_args)?;
    Ok(result)
}

// 集群和单机共享的方法, 由于Commands不是dyn 兼容的, 无法直接写在父类中(也许有其他办法?)
#[macro_export]
macro_rules! implement_pipeline_commands {
    ($struct_name:ident) => {
        fn mock_data(&self, count: u64) -> AnyResult<()> {
            let mut pipe = $struct_name::with_capacity(count as usize);
            for _ in 0..count {
                // string
                let key = format!("redis-me-mock:string:{}", random_string(10));
                pipe.set(&key, random_string(10)).ignore();

                // hash
                let field_count = random_range(3, 200);
                let key = format!("redis-me-mock:hash:{}", random_string(10));
                for x in 0..field_count {
                    pipe.hset(&key, format!("key{x}"), random_string(10))
                        .ignore();
                }

                // list
                let key = format!("redis-me-mock:list:{}", random_string(10));
                for _ in 0..field_count {
                    pipe.rpush(&key, random_string(10)).ignore();
                }

                // set
                let key = format!("redis-me-mock:set:{}", random_string(10));
                for _ in 0..field_count {
                    pipe.sadd(&key, random_string(10)).ignore();
                }

                // zset
                let key = format!("redis-me-mock:zset:{}", random_string(10));
                for _ in 0..field_count {
                    pipe.zadd(&key, random_string(10), random_range(1, 100))
                        .ignore();
                }
            }

            let mut conn = self.get_conn()?;
            let _: () = pipe.query(&mut conn)?;
            Ok(())
        }
    };
}

#[cfg(test)]
mod acl_selector_tests {
    use super::*;
    use redis::acl::Rule;

    #[test]
    fn selector_text_roundtrip() {
        let rules = vec![
            Rule::RemoveCategory("all".into()),
            Rule::AddCommand("set".into()),
            Rule::Pattern("key2".into()),
        ];
        let text = acl_rules_to_selector_text(&rules);
        assert_eq!(text, "-@all +set ~key2");

        let Rule::Selector(parsed) = acl_selector_from_text(&text).expect("parse selector") else {
            panic!("expected Rule::Selector");
        };
        assert_eq!(parsed.len(), 3);
        assert!(matches!(parsed[0], Rule::RemoveCategory(_)));
        assert!(matches!(parsed[1], Rule::AddCommand(_)));
        assert!(matches!(parsed[2], Rule::Pattern(_)));
    }

    #[test]
    fn acl_build_rules_keeps_selectors() {
        let param = AclSetuserParam {
            username: "u1".into(),
            enabled: true,
            password_hashes: vec![],
            command_rules: vec!["+@read".into()],
            key_patterns: vec!["*".into()],
            channel_patterns: vec!["*".into()],
            selectors: vec!["-@all +set ~key2".into()],
        };
        let rules = acl_build_rules(&param).expect("build acl rules");
        assert!(
            rules
                .iter()
                .any(|r| matches!(r, Rule::Selector(_))),
            "expected Rule::Selector in built rules"
        );
    }

    #[test]
    fn parse_acl_list_default_user() {
        let detail = parse_acl_list_line("user default on nopass ~* +@all").expect("parse");
        assert_eq!(detail.username, "default");
        assert!(detail.enabled);
        assert!(detail.nopass);
        assert_eq!(detail.key_patterns, vec!["*"]);
        assert!(detail.command_rules.contains(&"+@all".to_string()));
    }

    #[test]
    fn parse_acl_list_with_hash_and_selector() {
        let line = "user bob on #abc123 ~redis:* -@all +set (-@all +get ~key1)";
        let detail = parse_acl_list_line(line).expect("parse");
        assert_eq!(detail.username, "bob");
        assert_eq!(detail.password_hashes, vec!["abc123".to_string()]);
        assert_eq!(detail.key_patterns, vec!["redis:*"]);
        assert_eq!(detail.command_rules, vec!["-@all".to_string(), "+set".to_string()]);
        assert_eq!(detail.selectors, vec!["-@all +get ~key1".to_string()]);
    }
}
