use crate::client::ops::field_scan::{handle_other_value_type, vgetattr_opt, vsetattr_json_or_clear};
use crate::client::ops::info::parse_info_kv_items;
use crate::model::*;
use crate::support::error::AppError;
use crate::support::util::*;
use anyhow::{Context, bail};
use log::info;
use parking_lot::MutexGuard;
use redis::{Commands, FromRedisValue, Value, ValueType};

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

/// VSIM 分数。整数、浮点和字符串都能收，其他类型报错。
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
