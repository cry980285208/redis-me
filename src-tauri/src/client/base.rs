use crate::support::capabilities::ServerCapabilities;
use crate::support::error::AppError;
use crate::model::ConnConfig;
use crate::support::util::{AnyResult, CONNECTION_CONNECT_TIMEOUT, CONNECTION_NORMAL_TIMEOUT};
use chrono::Utc;
use parking_lot::RwLock;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU16};
use std::time::Duration;
use tauri::AppHandle;

/// 客户端的公共属性。只在后端使用，不参与前端类型导出。
#[derive(Debug, Clone)]
pub struct MeBase {
    pub id: String,
    pub conf: ConnConfig,
    pub db: Arc<AtomicU16>,
    pub subscribe_running: Arc<AtomicBool>,
    pub monitor_running: Arc<AtomicBool>,
    pub export_import_running: Arc<AtomicBool>,
    pub last_check_time: Arc<AtomicI64>,
    /// 已建立连接上的单次命令读写超时（init 时从 AppSettings 快照）
    pub command_timeout: Duration,
    /// 建连超时（TCP+握手+PING；init 时从 AppSettings 快照，重连复用）
    pub connection_timeout: Duration,
    /// 本连接命令执行日志（环形缓冲）
    pub command_logger: Arc<crate::support::command_log::CommandLogger>,
    /// 用于后台线程 emit 事件到前端
    pub app_handle: Arc<RwLock<Option<AppHandle>>>,
    /// 连接成功后检测的服务器能力
    pub capabilities: ServerCapabilities,
}

impl From<&ConnConfig> for MeBase {
    fn from(conf: &ConnConfig) -> Self {
        MeBase {
            id: conf.id.clone(),
            conf: conf.clone(),
            db: Arc::new(AtomicU16::new(conf.db)),
            subscribe_running: Arc::new(AtomicBool::new(false)),
            monitor_running: Arc::new(AtomicBool::new(false)),
            export_import_running: Arc::new(AtomicBool::new(false)),
            last_check_time: Arc::new(AtomicI64::new(Utc::now().timestamp())),
            command_timeout: CONNECTION_NORMAL_TIMEOUT,
            connection_timeout: CONNECTION_CONNECT_TIMEOUT,
            command_logger: Arc::new(crate::support::command_log::CommandLogger::new(
                conf.id.clone(),
                conf.name.clone(),
            )),
            app_handle: Arc::new(RwLock::new(None::<AppHandle>)),
            capabilities: ServerCapabilities::default(),
        }
    }
}

impl MeBase {
    /// 获取绑定的 AppHandle，未初始化时返回错误
    pub fn get_app_handle(&self) -> AnyResult<AppHandle> {
        self.app_handle.read().clone().ok_or_else(|| {
            AppError::Internal {
                message: "AppHandle not initialized".to_string(),
            }
            .into()
        })
    }
}
