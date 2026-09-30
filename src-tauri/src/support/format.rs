//! 将 Redis 键值格式化为 redis-cli 可粘贴执行的命令行，并把命令行拆回参数。

use crate::support::util::AnyResult;
use anyhow::bail;

/// 双引号包裹 + C 风格转义（与 redis-cli `sdscatrepr` 一致）
pub fn format_quoted(bytes: &[u8]) -> String {
    let mut s = String::from('"');
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'"' => {
                s.push_str("\\\"");
                i += 1;
            }
            b'\\' => {
                s.push_str("\\\\");
                i += 1;
            }
            b'\n' => {
                s.push_str("\\n");
                i += 1;
            }
            b'\r' => {
                s.push_str("\\r");
                i += 1;
            }
            b'\t' => {
                s.push_str("\\t");
                i += 1;
            }
            b'\x07' => {
                s.push_str("\\a");
                i += 1;
            }
            b'\x08' => {
                s.push_str("\\b");
                i += 1;
            }
            0x20..=0x7e => {
                s.push(b as char);
                i += 1;
            }
            _ => {
                if let Some((ch, len)) = read_utf8_char(bytes, i)
                    && !ch.is_control()
                {
                    s.push(ch);
                    i += len;
                } else {
                    s.push_str(&format!("\\x{:02x}", b));
                    i += 1;
                }
            }
        }
    }
    s.push('"');
    s
}

/// `SET key value`
pub fn format_set_command(key: &[u8], value: &[u8]) -> String {
    format!("SET {} {}", format_quoted(key), format_quoted(value))
}

/// `EXPIRE key seconds`。`ttl_secs` 是十进制明文，不再加引号。
pub fn format_expire_command(key: &[u8], ttl_secs: i64) -> String {
    format!("EXPIRE {} {}", format_quoted(key), ttl_secs)
}

/// `HMSET key field value ...`。没有字段时返回 `None`，避免写出空命令。
pub fn format_hmset_command(key: &[u8], pairs: &[(Vec<u8>, Vec<u8>)]) -> Option<String> {
    if pairs.is_empty() {
        return None;
    }
    let mut parts = vec!["HMSET".to_string(), format_quoted(key)];
    for (f, v) in pairs {
        parts.push(format_quoted(f));
        parts.push(format_quoted(v));
    }
    Some(parts.join(" "))
}

/// 单个 Hash 字段：`HSET key field value`
pub fn format_hset_command(key: &[u8], field: &[u8], value: &[u8]) -> String {
    format!(
        "HSET {} {} {}",
        format_quoted(key),
        format_quoted(field),
        format_quoted(value)
    )
}

/// Array 单槽：`ARSET key index value`
pub fn format_arset_command(key: &[u8], index: i64, value: &[u8]) -> String {
    format!(
        "ARSET {} {} {}",
        format_quoted(key),
        index,
        format_quoted(value)
    )
}

/// Vector Set：`VADD key VALUES dim f1..fn element [SETATTR json]`
pub fn format_vadd_command(
    key: &[u8],
    vector: &[f64],
    element: &[u8],
    attrs: Option<&str>,
) -> String {
    let floats = vector
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" ");
    let mut cmd = format!(
        "VADD {} VALUES {} {} {}",
        format_quoted(key),
        vector.len(),
        floats,
        format_quoted(element)
    );
    if let Some(a) = attrs.map(str::trim).filter(|s| !s.is_empty()) {
        cmd.push_str(" SETATTR ");
        cmd.push_str(&format_quoted(a.as_bytes()));
    }
    cmd
}

/// Array 多槽：`ARMSET key index value [index value ...]`
pub fn format_armset_command(key: &[u8], pairs: &[(i64, Vec<u8>)]) -> Option<String> {
    if pairs.is_empty() {
        return None;
    }
    let mut parts = vec!["ARMSET".to_string(), format_quoted(key)];
    for (idx, v) in pairs {
        parts.push(idx.to_string());
        parts.push(format_quoted(v));
    }
    Some(parts.join(" "))
}

/// `RPUSH key element ...`。没有元素时返回 `None`。
pub fn format_rpush_command(key: &[u8], items: &[Vec<u8>]) -> Option<String> {
    if items.is_empty() {
        return None;
    }
    let mut parts = vec!["RPUSH".to_string(), format_quoted(key)];
    for item in items {
        parts.push(format_quoted(item));
    }
    Some(parts.join(" "))
}

/// `SADD key member ...`。没有成员时返回 `None`。
pub fn format_sadd_command(key: &[u8], members: &[Vec<u8>]) -> Option<String> {
    if members.is_empty() {
        return None;
    }
    let mut parts = vec!["SADD".to_string(), format_quoted(key)];
    for m in members {
        parts.push(format_quoted(m));
    }
    Some(parts.join(" "))
}

/// `ZADD key score member ...`。整数分数不带小数点。没有成员时返回 `None`。
pub fn format_zadd_command(key: &[u8], pairs: &[(Vec<u8>, f64)]) -> Option<String> {
    if pairs.is_empty() {
        return None;
    }
    let mut parts = vec!["ZADD".to_string(), format_quoted(key)];
    for (member, score) in pairs {
        parts.push(format_score(*score));
        parts.push(format_quoted(member));
    }
    Some(parts.join(" "))
}

/// `XADD key id field value ...`，字段顺序保持传入顺序。
pub fn format_xadd_command(key: &[u8], id: &[u8], fields: &[(Vec<u8>, Vec<u8>)]) -> String {
    let mut parts = vec!["XADD".to_string(), format_quoted(key), format_quoted(id)];
    for (f, v) in fields {
        parts.push(format_quoted(f));
        parts.push(format_quoted(v));
    }
    parts.join(" ")
}

/// `JSON.SET key $ json`
pub fn format_json_set_command(key: &[u8], json: &[u8]) -> String {
    format!("JSON.SET {} $ {}", format_quoted(key), format_quoted(json))
}

/// TimeSeries 单样本：`TS.ADD key timestamp value`（timestamp/value 为十进制明文）
pub fn format_ts_add_command(key: &[u8], timestamp: &str, value: &str) -> String {
    format!(
        "TS.ADD {} {} {}",
        format_quoted(key),
        timestamp.trim(),
        value.trim()
    )
}

/// 与 redis-cli sdssplitargs 一致的分词（终端命令、ACL selector 等复用）
pub fn split_redis_args(line: &str) -> AnyResult<Vec<Vec<u8>>> {
    let mut args = Vec::new();
    let bytes = line.as_bytes();
    let mut p = 0usize;

    loop {
        while p < bytes.len() && bytes[p].is_ascii_whitespace() {
            p += 1;
        }
        if p >= bytes.len() {
            break;
        }

        let mut current = Vec::new();
        let mut in_double = false;
        let mut in_single = false;
        let mut done = false;

        while !done {
            if p >= bytes.len() {
                if in_double || in_single {
                    bail!("unbalanced quotes");
                }
                done = true;
                continue;
            }

            if in_double {
                if bytes[p] == b'\\'
                    && p + 1 < bytes.len()
                    && bytes[p + 1] == b'x'
                    && p + 3 < bytes.len()
                    && is_hex_digit(bytes[p + 2])
                    && is_hex_digit(bytes[p + 3])
                {
                    let byte = hex_digit_to_int(bytes[p + 2]) * 16 + hex_digit_to_int(bytes[p + 3]);
                    current.push(byte);
                    p += 3;
                } else if bytes[p] == b'\\' && p + 1 < bytes.len() {
                    p += 1;
                    let c = match bytes[p] {
                        b'n' => b'\n',
                        b'r' => b'\r',
                        b't' => b'\t',
                        b'b' => b'\x08',
                        b'a' => b'\x07',
                        other => other,
                    };
                    current.push(c);
                } else if bytes[p] == b'"' {
                    if p + 1 < bytes.len() && !bytes[p + 1].is_ascii_whitespace() {
                        bail!("unbalanced quotes");
                    }
                    done = true;
                } else {
                    current.push(bytes[p]);
                }
            } else if in_single {
                if bytes[p] == b'\\' && p + 1 < bytes.len() && bytes[p + 1] == b'\'' {
                    p += 1;
                    current.push(b'\'');
                } else if bytes[p] == b'\'' {
                    if p + 1 < bytes.len() && !bytes[p + 1].is_ascii_whitespace() {
                        bail!("unbalanced quotes");
                    }
                    done = true;
                } else {
                    current.push(bytes[p]);
                }
            } else {
                match bytes[p] {
                    b' ' | b'\n' | b'\r' | b'\t' => done = true,
                    b'"' => in_double = true,
                    b'\'' => in_single = true,
                    ch => current.push(ch),
                }
            }

            if p < bytes.len() {
                p += 1;
            }
        }

        args.push(current);
    }

    Ok(args)
}

// 解析命令：主要考虑解析带有引号的参数，比如：config set save "3600 1 300 100 60 10000"
pub fn parse_command(command: &str) -> AnyResult<(String, Vec<Vec<u8>>)> {
    let tokens = split_redis_args(command.trim())?;
    let first = tokens
        .first()
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .unwrap_or_default();
    let other = tokens.into_iter().skip(1).collect();
    Ok((first, other))
}

// ------------------------------ 仅本文件使用 ------------------------------

/// redis-cli `\xNN` 转义里的一个十六进制字符。
fn is_hex_digit(c: u8) -> bool {
    c.is_ascii_digit() || (b'a'..=b'f').contains(&c) || (b'A'..=b'F').contains(&c)
}

/// 一个十六进制字符转成 0–15。调用方应先用 `is_hex_digit` 判断。
fn hex_digit_to_int(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a' | b'A' => 10,
        b'b' | b'B' => 11,
        b'c' | b'C' => 12,
        b'd' | b'D' => 13,
        b'e' | b'E' => 14,
        b'f' | b'F' => 15,
        _ => 0,
    }
}

/// 按首字节判断一个 UTF-8 字符占几个字节。非法首字节返回 `None`。
fn utf8_char_len(b: u8) -> Option<usize> {
    if b <= 0x7f {
        Some(1)
    } else if (b & 0xe0) == 0xc0 {
        Some(2)
    } else if (b & 0xf0) == 0xe0 {
        Some(3)
    } else if (b & 0xf8) == 0xf0 {
        Some(4)
    } else {
        None
    }
}

/// 从 `i` 读出一个完整 UTF-8 字符。截断或非法序列返回 `None`。
fn read_utf8_char(bytes: &[u8], i: usize) -> Option<(char, usize)> {
    let len = utf8_char_len(bytes[i])?;
    if i + len > bytes.len() {
        return None;
    }
    let s = std::str::from_utf8(&bytes[i..i + len]).ok()?;
    let ch = s.chars().next()?;
    Some((ch, len))
}

/// 整数分数写成不带小数点的十进制，其余用 `Display`。
fn format_score(score: f64) -> String {
    if score.fract() == 0.0 && score.is_finite() {
        format!("{}", score as i64)
    } else {
        score.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 换行和不可见字节按 redis-cli 引号转义。
    #[test]
    fn test_format_quoted_newline_and_binary() {
        assert_eq!(format_quoted(b"Line01\nLine02"), "\"Line01\\nLine02\"");
        assert_eq!(format_quoted(b"\x00\x01\xff"), "\"\\x00\\x01\\xff\"");
        assert_eq!(format_quoted(b"\x07\x08"), "\"\\a\\b\"");
    }

    /// 多行 SET 转义之后还能拆回原来的字节。
    #[test]
    fn test_format_set_multiline() {
        let cmd = format_set_command(b"MultiLine", b"Line01\nLine02");
        assert_eq!(cmd, r#"SET "MultiLine" "Line01\nLine02""#);
        let args = split_redis_args(&cmd).unwrap();
        assert_eq!(args[1], b"MultiLine");
        assert_eq!(args[2], b"Line01\nLine02");
    }

    /// 中文和 emoji 写成字面量，不拆成 `\x`。
    #[test]
    fn test_format_set_utf8_literal() {
        let value = "RDM!\n中文 hepengju 表情😄 \n\nOfficial"
            .as_bytes()
            .to_vec();
        let cmd = format_set_command(b"RedisME", &value);
        assert!(cmd.contains("中文"));
        assert!(cmd.contains("😄"));
        assert!(!cmd.contains("\\xe4"));
        let args = split_redis_args(&cmd).unwrap();
        assert_eq!(args[2], value);
    }

    /// 单个 Hash 字段的 HSET 能拆回键、字段和值。
    #[test]
    fn test_format_hset_single_field() {
        let cmd = format_hset_command(b"user:1", b"name", b"\xe5\xbc\xa0\xe4\xb8\x89");
        assert_eq!(cmd, r#"HSET "user:1" "name" "张三""#);
        let args = split_redis_args(&cmd).unwrap();
        assert_eq!(args[1], b"user:1");
        assert_eq!(args[2], b"name");
        assert_eq!(args[3], b"\xe5\xbc\xa0\xe4\xb8\x89");
    }

    /// ARSET 把十进制下标放在键和值中间。
    #[test]
    fn test_format_arset_command() {
        let cmd = format_arset_command(b"arr:1", 3, b"hello");
        assert_eq!(cmd, r#"ARSET "arr:1" 3 "hello""#);
        let args = split_redis_args(&cmd).unwrap();
        assert_eq!(args[1], b"arr:1");
        assert_eq!(args[2], b"3");
        assert_eq!(args[3], b"hello");
    }

    /// ARMSET 按给定顺序写出多组下标和值。
    #[test]
    fn test_format_armset_command() {
        let pairs = vec![(0i64, b"a".to_vec()), (2, b"b\nc".to_vec())];
        let cmd = format_armset_command(b"arr:1", &pairs).unwrap();
        assert_eq!(cmd, r#"ARMSET "arr:1" 0 "a" 2 "b\nc""#);
    }

    /// HMSET 字段顺序保持传入顺序，不按名字重排。
    #[test]
    fn test_format_hmset_all_quoted() {
        let pairs = vec![
            (b"k3".to_vec(), b"v111".to_vec()),
            (b"k1".to_vec(), b"v111".to_vec()),
            (b"k2".to_vec(), b"v111".to_vec()),
        ];
        let cmd = format_hmset_command(b"hepengju:hash", &pairs).unwrap();
        assert_eq!(
            cmd,
            r#"HMSET "hepengju:hash" "k3" "v111" "k1" "v111" "k2" "v111""#
        );
        let args = split_redis_args(&cmd).unwrap();
        assert_eq!(args[1], b"hepengju:hash");
        assert_eq!(args[2], b"k3");
    }

    /// Hash 值里的中文写成字面量，拆回去仍是原来的字节。
    #[test]
    fn test_format_hmset_utf8_field() {
        let pairs = vec![
            (b"name".to_vec(), b"\xe5\xbc\xa0\xe4\xb8\x89".to_vec()),
            (b"age".to_vec(), b"28".to_vec()),
        ];
        let cmd = format_hmset_command(b"user:1", &pairs).unwrap();
        assert!(cmd.contains("\"user:1\""));
        assert!(cmd.contains("\"张三\""));
        let args = split_redis_args(&cmd).unwrap();
        assert_eq!(args[2], b"name");
        assert_eq!(args[3], b"\xe5\xbc\xa0\xe4\xb8\x89");
        assert_eq!(args[4], b"age");
        assert_eq!(args[5], b"28");
    }

    /// List 元素里的换行转义后还能拆回。
    #[test]
    fn test_format_rpush_newline() {
        let cmd = format_rpush_command(b"mylist", &[b"a".to_vec(), b"b\nc".to_vec()]).unwrap();
        assert_eq!(cmd, r#"RPUSH "mylist" "a" "b\nc""#);
        let args = split_redis_args(&cmd).unwrap();
        assert_eq!(args[3], b"b\nc");
    }

    /// 含 `0x00` 的值转义后再解析，字节不变。
    #[test]
    fn test_format_binary_roundtrip() {
        let cmd = format_set_command(b"binkey", b"\x00\x01\xffhello");
        let args = split_redis_args(&cmd).unwrap();
        assert_eq!(args[2], b"\x00\x01\xffhello");
    }

    /// 没有字段或元素时不生成空命令。
    #[test]
    fn test_empty_collection_returns_none() {
        assert!(format_hmset_command(b"k", &[]).is_none());
        assert!(format_rpush_command(b"k", &[]).is_none());
    }

    /// SADD、ZADD 和 JSON.SET 的参数顺序固定，JSON 文本拆回去不加引号层。
    #[test]
    fn test_format_sadd_zadd_json() {
        let sadd = format_sadd_command(b"myset", &[b"m1".to_vec(), b"m2".to_vec()]).unwrap();
        assert_eq!(sadd, r#"SADD "myset" "m1" "m2""#);
        let zadd = format_zadd_command(b"rank", &[(b"a".to_vec(), 1.5)]).unwrap();
        assert_eq!(zadd, r#"ZADD "rank" 1.5 "a""#);
        let json = format_json_set_command(b"doc", br#"{"name":"test"}"#);
        assert_eq!(json, r#"JSON.SET "doc" $ "{\"name\":\"test\"}""#);
        let args = split_redis_args(&json).unwrap();
        assert_eq!(args[3], br#"{"name":"test"}"#);
    }

    /// 整数分数不带小数点；过期秒数、VADD 属性和 TS.ADD 时间戳按明文拼上。
    #[test]
    fn score_expire_vadd_and_ts_add() {
        let zadd = format_zadd_command(b"rank", &[(b"a".to_vec(), 2.0)]).unwrap();
        assert_eq!(zadd, r#"ZADD "rank" 2 "a""#);
        assert_eq!(format_expire_command(b"k", 60), r#"EXPIRE "k" 60"#);

        let plain = format_vadd_command(b"vec", &[1.0, 0.5], b"e", Some("  "));
        assert_eq!(plain, r#"VADD "vec" VALUES 2 1 0.5 "e""#);
        let with_attr = format_vadd_command(b"vec", &[1.0], b"e", Some(" {\"a\":1} "));
        assert!(with_attr.contains(" SETATTR "));
        assert!(with_attr.ends_with(r#""{\"a\":1}""#));

        assert_eq!(
            format_ts_add_command(b"ts", " 100 ", " 1.5 "),
            r#"TS.ADD "ts" 100 1.5"#
        );
    }

    /// XADD 字段按传入顺序，不按字段名排序。
    #[test]
    fn test_format_xadd_field_order() {
        let fields = vec![
            (b"f2".to_vec(), b"v2".to_vec()),
            (b"f1".to_vec(), b"v1".to_vec()),
        ];
        let cmd = format_xadd_command(b"stream", b"1-0", &fields);
        assert_eq!(cmd, r#"XADD "stream" "1-0" "f2" "v2" "f1" "v1""#);
    }

    /// 一行命令拆成命令名和参数，引号里的空格不切开。
    #[test]
    fn test_parse_command() {
        let (cmd, args) = parse_command("").unwrap();
        assert_eq!(cmd, "");
        assert!(args.is_empty());

        let (cmd, args) = parse_command("ping").unwrap();
        assert_eq!(cmd, "ping");
        assert!(args.is_empty());

        let (cmd, args) = parse_command("set name hepengju").unwrap();
        assert_eq!(cmd, "set");
        assert_eq!(args, vec![b"name".to_vec(), b"hepengju".to_vec()]);

        let (cmd, args) = parse_command(r#"config set save "3600 1 300 100 60 10000" "#).unwrap();
        assert_eq!(cmd, "config");
        assert_eq!(
            args,
            vec![
                b"set".to_vec(),
                b"save".to_vec(),
                b"3600 1 300 100 60 10000".to_vec()
            ]
        );

        let (cmd, args) = parse_command(r#"config set save '3600 1 300 100 60 10000' "#).unwrap();
        assert_eq!(cmd, "config");
        assert_eq!(
            args,
            vec![
                b"set".to_vec(),
                b"save".to_vec(),
                b"3600 1 300 100 60 10000".to_vec()
            ]
        );
    }

    /// 反斜杠转义按 redis-cli 规则还原成字节。
    #[test]
    fn test_split_redis_args_escapes() {
        let args = split_redis_args(r#"SET "MultiLine" "Line01\nLine02""#).unwrap();
        assert_eq!(args[0], b"SET");
        assert_eq!(args[1], b"MultiLine");
        assert_eq!(args[2], b"Line01\nLine02");

        let args = split_redis_args(r#"SET 'MultiLine' 'Line01\nLine02'"#).unwrap();
        assert_eq!(args[2], b"Line01\\nLine02");

        let args = split_redis_args(r#"SET key "\xff\x00""#).unwrap();
        assert_eq!(args[2], vec![0xff, 0x00]);

        let args = split_redis_args(r#"call "Sabrina" and "Mark Smith\n""#).unwrap();
        assert_eq!(
            args,
            vec![
                b"call".to_vec(),
                b"Sabrina".to_vec(),
                b"and".to_vec(),
                b"Mark Smith\n".to_vec()
            ]
        );

        assert!(split_redis_args(r#""foo"bar"#).is_err());

        // 双引号内 \"、\\
        let args = split_redis_args(r#"SET key "say \"hi\"""#).unwrap();
        assert_eq!(args[2], br#"say "hi""#);

        let args = split_redis_args(r#"SET key "a\\b""#).unwrap();
        assert_eq!(args[2], br"a\b");

        // 空引号参数
        let args = split_redis_args(r#"SET key """#).unwrap();
        assert_eq!(args, vec![b"SET".to_vec(), b"key".to_vec(), Vec::new()]);

        // 单引号内 \'
        let args = split_redis_args(r#"SET key 'it\'s'"#).unwrap();
        assert_eq!(args[2], b"it's");

        // 未闭合引号
        assert!(split_redis_args(r#"SET key "abc"#).is_err());
        assert!(split_redis_args(r#"SET key "abc\"#).is_err());
    }
}
