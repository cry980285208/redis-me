//! 键空间 / 键事件通知：在专用连接上 `PSUBSCRIBE` 固定频道，把频道名与消息解析成
//! `(db, key, event)` 后通过 `EVENT_NOTIFY` 推给前端。线程 / 事件 / 停止机制镜像 `pubsub`。
//! 走独立的 `notify_running`，与发布订阅 Tab 互不顶掉，可同时订阅。

use crate::client::ops::pubsub::publish0;
use crate::model::*;
use crate::net::conn::set_client_name;
use crate::support::command_log::CommandLogger;
use crate::support::util::*;
use Ordering::Relaxed;
use chrono::Local;
use log::info;
use parking_lot::MutexGuard;
use redis::{Commands, Connection, Msg};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::thread::JoinHandle;
use tauri::{AppHandle, Emitter};

/// 在后台线程里订阅键空间通知频道，消息解析成 `NotifyEvent` 通过事件推到前端。
///
/// `node` 为事件来源节点地址（集群传各 master，单机传空串）；多条连接共用同一个 `running`，
/// 停止时置 flag 并向停止频道发一条即可唤醒所有线程。
pub fn notify_subscribe_node0(
    mut conn: Connection,
    running: Arc<AtomicBool>,
    app_handle: AppHandle,
    patterns: Vec<String>,
    id: String,
    node: String,
    logger: Arc<CommandLogger>,
) -> AnyResult<()> {
    set_client_name(&mut conn);
    running.store(true, Relaxed);

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
        info!("notify subscribe start: {:?} @ {}", patterns, node);
        while running.load(Relaxed) {
            let response = conn.recv_response()?;
            if let Some(msg) = Msg::from_value(&response) {
                let channel = msg.get_channel_name().to_string();
                let payload: Vec<u8> = msg.get_payload()?;
                // 停止频道等非通知频道解析为 None，仅用于唤醒，不进表格
                let Some((db, key, event)) =
                    parse_notify_channel(&channel, &vec8_to_display_string(&payload))
                else {
                    continue;
                };
                let event = NotifyEvent {
                    id: id.clone(),
                    datetime: Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
                    node: node.clone(),
                    db,
                    key,
                    event,
                };
                let _ = &app_handle.emit(EVENT_NOTIFY, event);
            }
        }
        info!("notify subscribe end: {:?} @ {}", patterns, node);
        Ok(())
    });
    Ok(())
}

/// 让通知订阅循环退出，并向停止频道发一条消息把阻塞的读唤醒（同发布订阅）。
pub fn notify_subscribe_stop0(
    conn: MutexGuard<impl Commands>,
    running: Arc<AtomicBool>,
) -> AnyResult<()> {
    running.store(false, Relaxed);
    // 停止订阅时必须发送一个消息，否则会阻塞
    publish0(
        conn,
        REDIS_ME_NOTIFY_STOP_CHANNEL,
        REDIS_ME_NOTIFY_STOP_CHANNEL,
        &BytesFormat::UTF8,
    )
}

/// 由视角（keyspace / keyevent）、库号与匹配模式构建 `PSUBSCRIBE` 模式；追加停止频道用于唤醒阻塞读。
///
/// `db` 为 `None` 时用 `@*` 通配所有库；非 `keyevent` 一律按键空间视角处理。
/// `pattern` 为 `None`/空时用 `*` 匹配全部；否则原样作为频道后缀 glob（keyspace 匹配键名，keyevent 匹配事件名）。
pub fn build_notify_patterns(
    perspective: &str,
    db: Option<u16>,
    pattern: Option<&str>,
) -> Vec<String> {
    let db_part = match db {
        Some(n) => n.to_string(),
        None => "*".to_string(),
    };
    let prefix = if perspective == "keyevent" {
        "__keyevent@"
    } else {
        "__keyspace@"
    };
    let match_part = match pattern {
        Some(p) if !p.is_empty() => p,
        _ => "*",
    };
    vec![
        format!("{prefix}{db_part}__:{match_part}"),
        REDIS_ME_NOTIFY_STOP_CHANNEL.into(),
    ]
}

/// 解析键空间通知频道为 `(db, key, event)`。
///
/// - `__keyspace@<db>__:<key>`：频道带键，payload 是事件名。
/// - `__keyevent@<db>__:<event>`：频道带事件名，payload 是键。
/// - 停止频道 / 无法识别的频道：返回 `None`（丢弃，不污染表格）。
///
/// `<db>` 解析失败按 0；键里若含分隔串 `__:` 只在首个处切分，其余原样保留。
pub fn parse_notify_channel(channel: &str, payload: &str) -> Option<(u16, String, String)> {
    if let Some(rest) = channel.strip_prefix("__keyspace@") {
        let (db_str, key) = rest.split_once("__:")?;
        let db = db_str.parse::<u16>().unwrap_or(0);
        Some((db, key.to_string(), payload.to_string()))
    } else if let Some(rest) = channel.strip_prefix("__keyevent@") {
        let (db_str, event) = rest.split_once("__:")?;
        let db = db_str.parse::<u16>().unwrap_or(0);
        Some((db, payload.to_string(), event.to_string()))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 视角决定前缀，db 为 None 时用 `@*`，pattern 为 None 时用 `*`；末尾恒追加停止频道用于唤醒。
    #[test]
    fn build_patterns_by_perspective_and_db() {
        assert_eq!(
            build_notify_patterns("keyspace", None, None),
            vec![
                "__keyspace@*__:*".to_string(),
                REDIS_ME_NOTIFY_STOP_CHANNEL.into()
            ]
        );
        assert_eq!(
            build_notify_patterns("keyevent", Some(2), None),
            vec![
                "__keyevent@2__:*".to_string(),
                REDIS_ME_NOTIFY_STOP_CHANNEL.into()
            ]
        );
        // 非 keyevent 一律按键空间视角
        assert_eq!(build_notify_patterns("", None, None)[0], "__keyspace@*__:*");
    }

    /// pattern 非空时作为频道后缀 glob；空串等同 None（回退 `*`）。
    #[test]
    fn build_patterns_uses_custom_match() {
        assert_eq!(
            build_notify_patterns("keyspace", Some(0), Some("user:*"))[0],
            "__keyspace@0__:user:*"
        );
        assert_eq!(
            build_notify_patterns("keyevent", None, Some("*expired*"))[0],
            "__keyevent@*__:*expired*"
        );
        // 空串回退到 *
        assert_eq!(
            build_notify_patterns("keyspace", None, Some(""))[0],
            "__keyspace@*__:*"
        );
    }

    /// 键空间视角：频道带键，payload 是事件名。
    #[test]
    fn parse_keyspace_channel_payload_is_event() {
        assert_eq!(
            parse_notify_channel("__keyspace@0__:user:1", "set"),
            Some((0, "user:1".to_string(), "set".to_string()))
        );
    }

    /// 键事件视角：频道带事件名，payload 是键。
    #[test]
    fn parse_keyevent_channel_payload_is_key() {
        assert_eq!(
            parse_notify_channel("__keyevent@3__:expired", "user:1"),
            Some((3, "user:1".to_string(), "expired".to_string()))
        );
    }

    /// 键里含分隔串时只在首个 `__:` 处切分，其余原样保留。
    #[test]
    fn parse_keeps_delimiter_inside_key() {
        assert_eq!(
            parse_notify_channel("__keyspace@1__:a__:b", "del"),
            Some((1, "a__:b".to_string(), "del".to_string()))
        );
    }

    /// 停止频道、未知频道、缺分隔串一律返回 None（不 emit）。
    #[test]
    fn parse_ignores_stop_and_unknown_channels() {
        assert_eq!(
            parse_notify_channel(REDIS_ME_NOTIFY_STOP_CHANNEL, "x"),
            None
        );
        assert_eq!(parse_notify_channel("somechannel", "x"), None);
        assert_eq!(parse_notify_channel("__keyspace@0__", "set"), None);
    }
}
