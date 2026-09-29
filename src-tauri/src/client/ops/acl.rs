use crate::model::*;
use crate::support::util::*;
use anyhow::bail;
use parking_lot::MutexGuard;
use redis::acl::Rule;
use redis::{Commands, FromRedisValue, Value};
use std::collections::HashSet;

pub fn acl_rule_to_string(rule: Rule) -> String {
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
pub fn acl_rule_to_setuser_arg(rule: &Rule) -> String {
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

pub fn acl_build_rules(param: &AclSetuserParam) -> AnyResult<Vec<Rule>> {
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

pub fn acl_user_detail_from_info(
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
pub fn parse_acl_list_line(line: &str) -> AnyResult<AclUserDetail> {
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
