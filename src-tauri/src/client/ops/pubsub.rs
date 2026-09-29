use crate::utils::command_log::CommandLogger;
use crate::net::conn::set_client_name;
use crate::utils::model::*;
use crate::utils::util::*;
use Ordering::Relaxed;
use chrono::Local;
use log::info;
use parking_lot::MutexGuard;
use redis::{Commands, Connection, Msg, from_redis_value};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::thread::JoinHandle;
use tauri::{AppHandle, Emitter};

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
