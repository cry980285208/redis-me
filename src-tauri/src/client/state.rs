use crate::client::client_trait::MeClient;
use crate::client::impl_cluster::MeCluster;
use crate::client::impl_single::MeSingle;
use crate::utils::error::AppError;
use crate::utils::model::{AppSettings, ConnConfig};
use crate::utils::util::AnyResult;
use log::{debug, info};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tauri::{AppHandle, Manager, State};

#[derive(Default)]
pub struct AppState {
    // 初始化连接列表
    pub connections: Mutex<HashMap<String, ConnConfig>>,

    // 缓存连接客户端
    pub clients: RwLock<HashMap<String, Arc<Box<dyn MeClient>>>>,

    /// 全局设置（建连/命令超时等），由前端 app_settings 同步
    pub app_settings: RwLock<AppSettings>,
}

/// 读取当前全局建连超时、命令超时
pub fn app_timeouts(app: &AppHandle) -> (Duration, Duration) {
    let state: State<AppState> = app.state();
    let s = state.app_settings.read().unwrap();
    (s.connection_timeout(), s.command_timeout())
}

pub trait ClientAccess {
    fn conn_list(&self, conn_list: Vec<ConnConfig>) -> AnyResult<()>;
    fn app_settings(&self, app_settings: AppSettings) -> AnyResult<()>;
    fn get_client(&self, id: &str) -> AnyResult<Arc<Box<dyn MeClient>>>;
    fn connect(&self, app_handle: AppHandle, id: &str) -> AnyResult<Arc<Box<dyn MeClient>>>;
    fn disconnect(&self, id: &str) -> AnyResult<()>;
}

impl ClientAccess for AppHandle {
    fn conn_list(&self, conn_list: Vec<ConnConfig>) -> AnyResult<()> {
        let state: State<AppState> = self.state();
        let mut map = state.connections.lock().unwrap();
        map.clear();
        for conn in conn_list {
            map.insert(conn.id.clone(), conn);
        }
        debug!("同步连接列表完成: {}", map.len());
        Ok(())
    }

    fn app_settings(&self, app_settings: AppSettings) -> AnyResult<()> {
        let state: State<AppState> = self.state();
        *state.app_settings.write().unwrap() = app_settings.normalized();
        debug!(
            "同步应用设置: connection_timeout_secs={}, command_timeout_secs={}",
            state.app_settings.read().unwrap().connection_timeout_secs,
            state.app_settings.read().unwrap().command_timeout_secs
        );
        Ok(())
    }

    fn get_client(&self, id: &str) -> AnyResult<Arc<Box<dyn MeClient>>> {
        let state: State<AppState> = self.state();
        {
            // Read lock在此代码块内，自动释放锁
            let clients = state.clients.read().unwrap();
            if let Some(client) = clients.get(id) {
                debug!("获取连接: {}", client.name());
                return Ok(Arc::clone(client));
            }
        }
        self.connect(self.clone(), id)
    }

    fn connect(&self, app_handle: AppHandle, id: &str) -> AnyResult<Arc<Box<dyn MeClient>>> {
        let state: State<AppState> = self.state();
        let map = state.connections.lock().unwrap();
        let conn = map
            .get(id)
            .ok_or(AppError::ConnectionNotFound { id: id.into() })?;

        let (connect_timeout, command_timeout) = app_timeouts(self);
        let mut clients = state.clients.write().unwrap();
        let client = Arc::new(if conn.cluster {
            MeCluster::init(conn, connect_timeout, command_timeout)?
        } else {
            MeSingle::init(conn, connect_timeout, command_timeout)?
        });

        client
            .base()
            .command_logger
            .bind_app_handle(app_handle.clone());
        *client.base().app_handle.write() = Some(app_handle);
        clients.insert(id.to_string(), Arc::clone(&client));
        info!("连接成功: {}", client.name());
        Ok(client)
    }

    fn disconnect(&self, id: &str) -> AnyResult<()> {
        let state: State<AppState> = self.state();
        let mut clients = state.clients.write().unwrap();
        let client = clients.get(id);
        match client {
            Some(client) => {
                info!("断开连接: {}", client.name());
                clients.remove(id);
            }
            None => info!("未找到连接, 断开忽略: {}", id),
        };
        Ok(())
    }
}
