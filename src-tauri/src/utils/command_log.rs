//! 命令执行日志：拦截 Redis 请求并写入环形缓冲。
//!
//! - 单机：LoggingConnection 包装 Connection，在 ConnectionLike 拦截。
//! - 集群：LoggingClusterConnection 包装 ClusterConnection（ConnectionLike + route_command）。
//! - Pipeline 仅在 req_packed_commands 记一条汇总；Subscribe/Monitor/导出线程走独立连接，不记录。
//! - 连接初始化命令（INFO、CLIENT SETNAME、PING、CLUSTER NODES 等）均记入日志。
//! - 错误判定：`req_command` 的 `Err`，以及 RESP3 下 `Ok(Value::ServerError)`（与 redis-rs `Cmd::query` 的 `extract_error` 一致）。
//! - 写入后通过 `command-log` 事件推增量；打开面板时 `command_logs(limit)` 拉一次快照。

use crate::utils::model::{CommandLogEntry, CommandLogEvent};
use crate::utils::util::EVENT_COMMAND_LOG;
use chrono::Local;
use log::debug;
use parking_lot::RwLock;
use redis::cluster::ClusterConnection;
use redis::cluster_routing::RoutingInfo;
use redis::{Arg, Cmd, Connection, ConnectionLike, RedisResult, Value};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

const DEFAULT_MAX_ENTRIES: usize = 1_000;

/// 每连接一份，挂在 MeBase.command_logger
#[derive(Debug)]
pub struct CommandLogger {
    entries: RwLock<Vec<CommandLogEntry>>,
    next_id: AtomicU64,
    max_entries: usize,
    conn_id: String,
    /// 连接显示名，仅用于控制台 info 日志
    conn_name: String,
    app_handle: RwLock<Option<AppHandle>>,
}

impl CommandLogger {
    pub fn new(conn_id: String, conn_name: String) -> Self {
        Self {
            entries: RwLock::new(Vec::new()),
            next_id: AtomicU64::new(1),
            max_entries: DEFAULT_MAX_ENTRIES,
            conn_id,
            conn_name,
            app_handle: RwLock::new(None),
        }
    }

    /// connect 成功后绑定，用于 push 时 emit 增量事件
    pub fn bind_app_handle(&self, app_handle: AppHandle) {
        *self.app_handle.write() = Some(app_handle);
    }

    pub fn clear(&self) {
        self.entries.write().clear();
    }

    /// 最近 limit 条（新→旧），供打开面板时拉快照
    pub fn query(&self, limit: Option<u64>) -> Vec<CommandLogEntry> {
        let limit = limit.unwrap_or(1000) as usize;
        let entries = self.entries.read();
        entries
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }

    pub fn log_from_cmd(&self, db_index: u16, cmd: &Cmd, result: &RedisResult<Value>, duration_ms: u64) {
        let (command, args) = parse_cmd(cmd);
        let error = command_log_error(result);
        self.push_entry(db_index, &command, &args, error, duration_ms);
    }

    pub fn log_raw(&self, db_index: u16, command: &str, args: &[String], error: Option<String>, duration_ms: u64) {
        self.push_entry(db_index, command, args, error, duration_ms);
    }

    fn push_entry(
        &self,
        db_index: u16,
        command: &str,
        args: &[String],
        error: Option<String>,
        duration_ms: u64,
    ) {
        let full_command = if args.is_empty() {
            command.to_string()
        } else {
            format!("{} {}", command, args.join(" "))
        };

        let entry = CommandLogEntry {
            id: self.next_id.fetch_add(1, Ordering::Relaxed),
            timestamp: Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            db_index,
            command: command.to_string(),
            args: args.to_vec(),
            full_command,
            duration_ms,
            error,
        };

        debug!("[{}] db={} {}", self.conn_name, db_index, entry.full_command);
        self.emit_entry(&entry);

        let mut entries = self.entries.write();
        entries.push(entry);
        if entries.len() > self.max_entries {
            let drop_count = entries.len() - self.max_entries;
            entries.drain(0..drop_count);
        }
    }

    fn emit_entry(&self, entry: &CommandLogEntry) {
        let Some(app_handle) = self.app_handle.read().clone() else {
            return;
        };
        let event = CommandLogEvent {
            id: self.conn_id.clone(),
            entry: entry.clone(),
        };
        let _ = app_handle.emit(EVENT_COMMAND_LOG, event);
    }

    fn log_pipeline_packed(
        &self,
        db_index: u16,
        count: usize,
        result: &RedisResult<Vec<Value>>,
        duration_ms: u64,
    ) {
        let summary = format!("{}x commands", count.max(1));
        let error = result.as_ref().err().map(|e| e.to_string());
        self.push_entry(db_index, "PIPELINE", &[summary], error, duration_ms);
    }
}

/// 单机模式：包装 Connection（勿 DerefMut，避免与 redis blanket ConnectionLike 冲突）
pub struct LoggingConnection {
    inner: Connection,
    logger: Arc<CommandLogger>,
    db_index: u16,
}

impl LoggingConnection {
    pub fn new(inner: Connection, logger: Arc<CommandLogger>, db_index: u16) -> Self {
        Self {
            inner,
            logger,
            db_index,
        }
    }

    pub fn set_read_timeout(&mut self, timeout: Option<Duration>) -> RedisResult<()> {
        self.inner.set_read_timeout(timeout)
    }

    pub fn set_write_timeout(&mut self, timeout: Option<Duration>) -> RedisResult<()> {
        self.inner.set_write_timeout(timeout)
    }

    pub fn set_db_index(&mut self, db: u16) {
        self.db_index = db;
    }
}

impl ConnectionLike for LoggingConnection {
    fn req_command(&mut self, cmd: &Cmd) -> RedisResult<Value> {
        let start = Instant::now();
        let result = self.inner.req_command(cmd);
        let duration_ms = start.elapsed().as_millis() as u64;
        self.logger
            .log_from_cmd(self.db_index, cmd, &result, duration_ms);
        result
    }

    fn req_packed_command(&mut self, cmd: &[u8]) -> RedisResult<Value> {
        self.inner.req_packed_command(cmd)
    }

    fn req_packed_commands(
        &mut self,
        cmd: &[u8],
        offset: usize,
        count: usize,
    ) -> RedisResult<Vec<Value>> {
        let start = Instant::now();
        let result = self.inner.req_packed_commands(cmd, offset, count);
        let duration_ms = start.elapsed().as_millis() as u64;
        self.logger
            .log_pipeline_packed(self.db_index, count, &result, duration_ms);
        result
    }

    fn get_db(&self) -> i64 {
        self.db_index as i64
    }

    fn check_connection(&mut self) -> bool {
        self.inner.check_connection()
    }

    fn is_open(&self) -> bool {
        self.inner.is_open()
    }
}

/// 集群模式：包装 ClusterConnection
pub struct LoggingClusterConnection {
    inner: ClusterConnection,
    logger: Arc<CommandLogger>,
    db_index: u16,
}

impl LoggingClusterConnection {
    pub fn new(inner: ClusterConnection, logger: Arc<CommandLogger>, db_index: u16) -> Self {
        Self {
            inner,
            logger,
            db_index,
        }
    }

    pub fn set_read_timeout(&mut self, timeout: Option<Duration>) -> RedisResult<()> {
        self.inner.set_read_timeout(timeout)
    }

    pub fn set_write_timeout(&mut self, timeout: Option<Duration>) -> RedisResult<()> {
        self.inner.set_write_timeout(timeout)
    }

    pub fn route_command(&mut self, cmd: &Cmd, route: RoutingInfo) -> RedisResult<Value> {
        let start = Instant::now();
        let result = self.inner.route_command(cmd, route);
        let duration_ms = start.elapsed().as_millis() as u64;
        self.logger
            .log_from_cmd(self.db_index, cmd, &result, duration_ms);
        result
    }

    pub fn inner_mut(&mut self) -> &mut ClusterConnection {
        &mut self.inner
    }

    /// ClusterPipeline::query 只接受 ClusterConnection，在此补记 PIPELINE 汇总
    pub fn cluster_pipe_query<T: redis::FromRedisValue>(
        &mut self,
        pipe: &redis::cluster::ClusterPipeline,
        command_count: usize,
    ) -> RedisResult<T> {
        let start = Instant::now();
        let result = pipe.query(self.inner_mut());
        let duration_ms = start.elapsed().as_millis() as u64;
        let log_result: RedisResult<Vec<Value>> = match &result {
            Ok(_) => Ok(vec![]),
            Err(e) => Err(e.clone()),
        };
        self.logger.log_pipeline_packed(
            self.db_index,
            command_count,
            &log_result,
            duration_ms,
        );
        result
    }
}

impl ConnectionLike for LoggingClusterConnection {
    fn req_command(&mut self, cmd: &Cmd) -> RedisResult<Value> {
        let start = Instant::now();
        let result = self.inner.req_command(cmd);
        let duration_ms = start.elapsed().as_millis() as u64;
        self.logger
            .log_from_cmd(self.db_index, cmd, &result, duration_ms);
        result
    }

    fn req_packed_command(&mut self, cmd: &[u8]) -> RedisResult<Value> {
        self.inner.req_packed_command(cmd)
    }

    fn req_packed_commands(
        &mut self,
        cmd: &[u8],
        offset: usize,
        count: usize,
    ) -> RedisResult<Vec<Value>> {
        let start = Instant::now();
        let result = self.inner.req_packed_commands(cmd, offset, count);
        let duration_ms = start.elapsed().as_millis() as u64;
        self.logger
            .log_pipeline_packed(self.db_index, count, &result, duration_ms);
        result
    }

    fn get_db(&self) -> i64 {
        self.db_index as i64
    }

    fn check_connection(&mut self) -> bool {
        self.inner.check_connection()
    }

    fn is_open(&self) -> bool {
        self.inner.is_open()
    }
}

fn command_log_error(result: &RedisResult<Value>) -> Option<String> {
    match result {
        Err(e) => Some(e.to_string()),
        Ok(value) => value.clone().extract_error().err().map(|e| e.to_string()),
    }
}

fn parse_cmd(cmd: &Cmd) -> (String, Vec<String>) {
    let args: Vec<String> = cmd
        .args_iter()
        .filter_map(|arg| match arg {
            Arg::Simple(bytes) => String::from_utf8(bytes.to_vec()).ok(),
            Arg::Cursor => Some("CURSOR".to_string()),
            _ => None,
        })
        .collect();
    let command = args
        .first()
        .cloned()
        .unwrap_or_else(|| "UNKNOWN".into())
        .to_uppercase();
    let rest = args.into_iter().skip(1).collect();
    (command, rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_overwrites_oldest() {
        let logger = CommandLogger {
            entries: RwLock::new(Vec::new()),
            next_id: AtomicU64::new(1),
            max_entries: 3,
            conn_id: "test".into(),
            conn_name: "test".into(),
            app_handle: RwLock::new(None),
        };
        let cmd = redis::cmd("GET");
        for _ in 0..5 {
            logger.log_from_cmd(0, &cmd, &Ok(Value::Okay), 1);
        }
        assert_eq!(logger.entries.read().len(), 3);
    }

    #[test]
    fn command_log_error_includes_server_error_value() {
        let wire = b"-NOPERM User has no permissions to run the 'config|get' command\r\n";
        let value = redis::parse_redis_value(wire).unwrap();
        let err = command_log_error(&Ok(value)).expect("should detect server error");
        assert!(err.contains("NoPerm") || err.contains("permissions"));
    }
}
