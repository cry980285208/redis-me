use crate::utils::command_log::CommandLogger;
use crate::utils::conn::set_client_name;
use crate::utils::error::AppError;
use crate::utils::model::*;
use crate::utils::util::*;
use Ordering::Relaxed;
use anyhow::{Context, bail};
use chrono::Local;
use log::info;
use parking_lot::MutexGuard;
use redis::acl::Rule;
use redis::streams::{StreamInfoConsumersReply, StreamInfoGroupsReply};
use redis::{Commands, Connection, FromRedisValue, Msg, Value, ValueType, from_redis_value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::thread::JoinHandle;
use tauri::{AppHandle, Emitter};

use crate::client::ops::field_scan::{handle_other_value_type, vgetattr_opt, vsetattr_json_or_clear};

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

    fn field_ttl(&self, param: RedisFieldTtl) -> AnyResult<()>;

    fn field_get(&self, param: RedisFieldGet) -> AnyResult<RedisFieldValue>;

    fn hash_keys(&self, param: RedisHashKeys) -> AnyResult<Vec<String>>;

    fn hash_values(&self, param: RedisHashKeys) -> AnyResult<Vec<String>>;

    fn field_pop(&self, param: RedisPop) -> AnyResult<String>;

    fn field_del(&self, param: RedisFieldDel) -> AnyResult<()>;

    fn zset_rank(&self, param: RedisZsetRank) -> AnyResult<RedisZsetRankResult>;

    fn zset_range(&self, param: RedisZsetRange) -> AnyResult<Vec<RedisZsetRangeItem>>;

    fn ar_last_items(&self, param: RedisArLastItems) -> AnyResult<Vec<RedisArLastItemsItem>>;

    fn ar_info(&self, key: RedisKey) -> AnyResult<Vec<RedisArInfoItem>>;

    fn v_info(&self, key: RedisKey) -> AnyResult<Vec<RedisArInfoItem>>;

    fn ts_info(&self, key: RedisKey) -> AnyResult<Vec<RedisArInfoItem>>;

    fn v_getattr(&self, param: RedisVAttr) -> AnyResult<String>;

    fn v_setattr(&self, param: RedisVAttr) -> AnyResult<()>;

    fn v_sim(&self, param: RedisVSim) -> AnyResult<Vec<RedisVSimItem>>;

    fn object_info(&self, key: RedisKey) -> AnyResult<RedisObjectInfo>;

    fn execute_command(&self, param: RedisCommand) -> AnyResult<String>;

    fn config_get(&self, pattern: &str, node: Option<String>)
    -> AnyResult<HashMap<String, String>>;

    fn config_set(&self, key: &str, value: &str, node: Option<String>) -> AnyResult<()>;

    fn slow_log(&self, count: Option<u64>, node: Option<String>) -> AnyResult<Vec<RedisSlowLog>>;

    /// 对一批键 pipeline MEMORY USAGE（及可选 TYPE），只保留 >= size_limit 的。
    fn memory_usage_keys(
        &self,
        keys: &[RedisKey],
        size_limit: u64,
        need_key_type: bool,
    ) -> AnyResult<Vec<RedisKeySize>>;

    /// 一轮：复用 `scan()` 游标（含集群主节点），再测这批键的内存。
    fn memory_usage(&self, param: RedisMemoryParam) -> AnyResult<RedisMemoryResult> {
        let scan = self.scan(ScanParam {
            pattern: param.pattern.clone().unwrap_or_else(|| "*".into()),
            scan_type: None,
            cursor: param.cursor.clone(),
            exact: false,
            count: param.scan_count,
        })?;
        let scanned = scan.key_list.len() as u64;
        let key_list = self.memory_usage_keys(
            &scan.key_list,
            param.size_limit,
            param.need_key_type.unwrap_or(false),
        )?;
        Ok(RedisMemoryResult {
            key_list,
            cursor: scan.cursor,
            scanned,
        })
    }

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

/// Vector Set VINFO：元数据；行结构与 ARINFO 相同（field/value）。
pub fn v_info0(
    mut conn: MutexGuard<impl Commands>,
    key: RedisKey,
) -> AnyResult<Vec<RedisArInfoItem>> {
    let key_type: ValueType = conn.key_type(&key)?;
    if key_type != ValueType::VectorSet {
        handle_other_value_type(&key_type, &key)?;
        unreachable!()
    }
    let raw: Value = redis::cmd("VINFO").arg(&key).query(&mut conn)?;
    parse_info_kv_items(raw, "VINFO")
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

/// Vector Set VGETATTR：按需读取元素 attrs（不随 VRANGE）
pub fn v_getattr0(mut conn: MutexGuard<impl Commands>, param: RedisVAttr) -> AnyResult<String> {
    let key: RedisKey = param.key;
    let key_type: ValueType = conn.key_type(&key)?;
    if key_type != ValueType::VectorSet {
        handle_other_value_type(&key_type, &key)?;
        unreachable!()
    }
    let val_fmt = param.val_fmt.as_ref().cloned().unwrap_or_default();
    let elem = parse_bytes(&param.field_key, &val_fmt)?;
    Ok(vgetattr_opt(&mut conn, &key, &elem).unwrap_or_default())
}

/// Vector Set VSETATTR：空串删除属性
pub fn v_setattr0(mut conn: MutexGuard<impl Commands>, param: RedisVAttr) -> AnyResult<()> {
    let key: RedisKey = param.key;
    let key_type: ValueType = conn.key_type(&key)?;
    if key_type != ValueType::VectorSet {
        handle_other_value_type(&key_type, &key)?;
        unreachable!()
    }
    let val_fmt = param.val_fmt.as_ref().cloned().unwrap_or_default();
    let elem = parse_bytes(&param.field_key, &val_fmt)?;
    vsetattr_json_or_clear(&mut conn, &key, &elem, &param.attrs)
}

/// Vector Set VSIM：相似度查询。固定 WITHSCORES；可选 WITHATTRIBS / EPSILON / EF / FILTER。
/// 用原始 cmd（redis-rs VSimOptions 缺 WITHATTRIBS / EPSILON）。
pub fn v_sim0(
    mut conn: MutexGuard<impl Commands>,
    param: RedisVSim,
) -> AnyResult<Vec<RedisVSimItem>> {
    let key_type: ValueType = conn.key_type(&param.key)?;
    if key_type != ValueType::VectorSet {
        handle_other_value_type(&key_type, &param.key)?;
        unreachable!()
    }
    let val_fmt = param.val_fmt.as_ref().cloned().unwrap_or_default();

    match vsim_query(&mut conn, &param, &val_fmt, param.with_attribs) {
        Ok(raw) => parse_vsim_items(raw, param.with_attribs, &val_fmt),
        // WITHATTRIBS 不支持（Redis 8.0.0–8.0.2）：去掉重试，再用 VGETATTR pipeline 补属性（语义无损）
        Err(_) if param.with_attribs => {
            let raw = vsim_query(&mut conn, &param, &val_fmt, false)?;
            let mut items = parse_vsim_items(raw, false, &val_fmt)?;
            fill_vsim_attrs(&mut conn, &param.key, &mut items, &val_fmt);
            Ok(items)
        }
        Err(e) => Err(e),
    }
}

/// 组装并执行 VSIM；with_attribs 控制是否附带 WITHATTRIBS
fn vsim_query(
    conn: &mut impl Commands,
    param: &RedisVSim,
    val_fmt: &BytesFormat,
    with_attribs: bool,
) -> AnyResult<Value> {
    let count = if param.count == 0 { 10 } else { param.count };
    let mode = param.mode.trim().to_ascii_lowercase();

    let mut cmd = redis::cmd("VSIM");
    cmd.arg(&param.key);
    match mode.as_str() {
        "ele" => {
            let elem = parse_bytes(&param.field_key, val_fmt)?;
            if elem.is_empty() {
                bail!("VSIM ELE requires element name");
            }
            cmd.arg("ELE").arg(&elem);
        }
        "values" => {
            if param.vector.is_empty() {
                bail!("VSIM VALUES requires vector");
            }
            cmd.arg("VALUES").arg(param.vector.len());
            for f in &param.vector {
                cmd.arg(*f);
            }
        }
        other => bail!("unsupported VSIM mode: {other}"),
    }
    // 固定开分；双 WITH* 时 RESP2 序为 ele, score, attribs
    cmd.arg("WITHSCORES");
    if with_attribs {
        cmd.arg("WITHATTRIBS");
    }
    cmd.arg("COUNT").arg(count);
    if let Some(eps) = param.epsilon {
        cmd.arg("EPSILON").arg(eps);
    }
    if let Some(ef) = param.ef {
        cmd.arg("EF").arg(ef);
    }
    let filter = param.filter.trim();
    if !filter.is_empty() {
        cmd.arg("FILTER").arg(filter);
    }
    Ok(cmd.query(conn)?)
}

/// VGETATTR pipeline 补 VSIM 结果属性（同一 key 同 slot，1 RTT）；失败保留空属性不阻断
fn fill_vsim_attrs(
    conn: &mut impl Commands,
    key: &RedisKey,
    items: &mut [RedisVSimItem],
    val_fmt: &BytesFormat,
) {
    if items.is_empty() {
        return;
    }
    let elems: Vec<Vec<u8>> = match items
        .iter()
        .map(|it| parse_bytes(&it.key, val_fmt))
        .collect::<AnyResult<Vec<_>>>()
    {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut pipe = redis::pipe();
    for elem in &elems {
        pipe.cmd("VGETATTR").arg(key).arg(elem.as_slice());
    }
    let packed = pipe.get_packed_pipeline();
    let results: Vec<redis::Value> = match conn.req_packed_commands(&packed, 0, elems.len()) {
        Ok(r) => r,
        Err(_) => return,
    };
    for (item, raw) in items.iter_mut().zip(results) {
        let attrs: Option<String> = FromRedisValue::from_redis_value_ref(&raw).unwrap_or_default();
        item.attrs = attrs.unwrap_or_default();
    }
}

/// RESP2：仅 WITHSCORES → ele,score；双 WITH* → ele,score,attribs
fn parse_vsim_items(
    raw: Value,
    with_attribs: bool,
    val_fmt: &BytesFormat,
) -> AnyResult<Vec<RedisVSimItem>> {
    let arr = match raw {
        Value::Nil => return Ok(Vec::new()),
        Value::Array(a) => a,
        // RESP3 下模块命令以 Map 回复（实测确认；ele → score，或 ele → [score, attribs]），拍平后按原 stride 解析
        Value::Map(pairs) => {
            // RESP3 Map 形态确认点：日志出现即说明兼容分支生效
            info!("RESP3: VSIM 以 Map 返回，拍平后按 stride 解析");
            flatten_vsim_map(pairs, with_attribs)
        }
        other => bail!(AppError::Internal {
            message: format!("unexpected VSIM reply: {:?}", other)
        }),
    };
    let stride = if with_attribs { 3 } else { 2 };
    if arr.len() % stride != 0 {
        bail!(AppError::Internal {
            message: format!(
                "unexpected VSIM reply length {} (stride {})",
                arr.len(),
                stride
            )
        });
    }
    let mut items = Vec::with_capacity(arr.len() / stride);
    let mut i = 0;
    while i < arr.len() {
        let key_bytes = redis_value_to_bulk_bytes(arr[i].clone());
        let score = redis_value_as_f64(arr[i + 1].clone())?;
        let attrs = if with_attribs {
            match &arr[i + 2] {
                Value::Nil => String::new(),
                v => redis_value_to_string(v.clone(), ""),
            }
        } else {
            String::new()
        };
        items.push(RedisVSimItem {
            key: format_bytes(&key_bytes, val_fmt),
            score,
            attrs,
        });
        i += stride;
    }
    Ok(items)
}

/// RESP3 Map 回复拍平：ele→score 展为 [ele, score]；ele→[score, attribs] 展为 [ele, score, attribs]
fn flatten_vsim_map(pairs: Vec<(Value, Value)>, with_attribs: bool) -> Vec<Value> {
    let mut flat = Vec::with_capacity(pairs.len() * 3);
    for (k, v) in pairs {
        flat.push(k);
        match v {
            Value::Array(mut pair) if with_attribs && pair.len() == 2 => {
                flat.push(pair.remove(0));
                flat.push(pair.remove(0));
            }
            other => flat.push(other),
        }
    }
    flat
}

fn redis_value_as_f64(value: Value) -> AnyResult<f64> {
    match value {
        Value::Double(d) => Ok(d),
        Value::Int(i) => Ok(i as f64),
        Value::BulkString(b) => {
            let s = String::from_utf8_lossy(&b);
            s.parse::<f64>()
                .with_context(|| format!("invalid VSIM score: {s}"))
        }
        Value::SimpleString(s) => s
            .parse::<f64>()
            .with_context(|| format!("invalid VSIM score: {s}")),
        other => bail!(AppError::Internal {
            message: format!("unexpected VSIM score: {:?}", other)
        }),
    }
}

/// ARINFO / VINFO 等扁平键值回复 → 保序 field/value 行
fn parse_info_kv_items(raw: Value, cmd: &str) -> AnyResult<Vec<RedisArInfoItem>> {
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
    if running.swap(false, Relaxed) {
        info!("monitor stop");
    }
    Ok(())
}

pub fn key_type0(mut conn: MutexGuard<impl Commands>, key: RedisKey) -> AnyResult<String> {
    // 简单字符串回复：key 的类型，如果 key 不存在则返回 none
    let key_type: ValueType = conn.key_type(&key)?;
    Ok(ui_key_type(key_type))
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
        rules.extend(param.command_rules.iter().map(|x| acl_rule_from_text(x)));
    }

    if param.key_patterns.is_empty() {
        rules.push(Rule::AllKeys);
    } else {
        rules.extend(param.key_patterns.iter().map(|x| acl_key_rule_from_text(x)));
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

pub fn acl_genpass0(mut conn: MutexGuard<impl Commands>, bits: Option<i64>) -> AnyResult<String> {
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
    let value: Value = redis::cmd("ACL").arg("LOG").arg(count).query(&mut *conn)?;

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
            rules.iter().any(|r| matches!(r, Rule::Selector(_))),
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
        assert_eq!(
            detail.command_rules,
            vec!["-@all".to_string(), "+set".to_string()]
        );
        assert_eq!(detail.selectors, vec!["-@all +get ~key1".to_string()]);
    }

    fn cmd_args(cmd: &redis::Cmd) -> Vec<String> {
        cmd.args_iter()
            .map(|arg| match arg {
                redis::Arg::Simple(bytes) => String::from_utf8(bytes.to_vec()).unwrap(),
                redis::Arg::Cursor => "CURSOR".into(),
                _ => unreachable!("command args are plain bytes"),
            })
            .collect()
    }

    #[test]
    fn setuser_keeps_selector_after_other_rules() {
        let param = AclSetuserParam {
            username: "u1".into(),
            enabled: true,
            password_hashes: vec![],
            command_rules: vec!["+@read".into()],
            key_patterns: vec!["*".into()],
            channel_patterns: vec!["*".into()],
            selectors: vec!["-@all +set ~key2".into()],
        };
        let args = cmd_args(&build_acl_setuser_cmd(&param).expect("setuser"));
        let pos = |needle: &str| args.iter().position(|arg| arg == needle).unwrap();
        assert!(pos("SETUSER") < pos("+@read"));
        assert!(pos("+@read") < pos("allkeys"));
        assert!(pos("allkeys") < pos("(-@all +set ~key2)"));
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
