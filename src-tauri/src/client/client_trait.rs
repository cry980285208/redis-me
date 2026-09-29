use crate::utils::error::AppError;
use crate::utils::model::*;
use crate::utils::util::*;
use anyhow::{Context, bail};
use log::info;
use parking_lot::MutexGuard;
use redis::streams::{StreamInfoConsumersReply, StreamInfoGroupsReply};
use redis::{Commands, FromRedisValue, Value, ValueType};
use std::collections::HashMap;

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
