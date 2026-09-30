mod api;
pub mod client;
pub mod model;
pub mod net;
pub mod support;

use crate::client::state::AppState;
use crate::support::setup::{
    app_setup, init_logger, install_rustls, single_instance, specta_builder, window_state,
};

/// 启动应用：安装 rustls、注册命令、挂上插件，并显示主窗口。
#[rustfmt::skip] // 行尾注释按列对齐，rustfmt 会把多余空格收成一个
pub fn run() {
    install_rustls();

    let specta_builder = specta_builder();
    let invoke_handler = specta_builder.invoke_handler();

    tauri::Builder::default()
        .plugin(single_instance())                            // 单实例，再次启动时聚焦已有窗口
        .plugin(window_state())                               // 记住窗口位置和大小，不记住可见性
        .plugin(tauri_plugin_os::init())                      // 操作系统信息
        .plugin(tauri_plugin_system_fonts::init())            // 系统字体
        .plugin(tauri_plugin_process::init())                 // 进程（重启、退出）
        .plugin(tauri_plugin_updater::Builder::new().build()) // 应用更新
        .plugin(tauri_plugin_fs::init())                      // 文件系统（导入导出）
        .plugin(tauri_plugin_store::Builder::new().build())   // 状态存储（连接、设置的自动保存和读取）
        .plugin(tauri_plugin_dialog::init())                  // 弹框选择文件
        .plugin(tauri_plugin_opener::init())                  // 打开外部链接
        .plugin(tauri_plugin_shell::init())                   // 自定义 Formatter 执行外部脚本
        .plugin(init_logger().build())                        // 日志
        .setup(app_setup(specta_builder))                     // 挂上 specta 事件并显示主窗口
        .manage(AppState::default())                          // 状态管理，保持 Redis 连接
        .invoke_handler(invoke_handler)                       // 注册 Tauri 命令
        .run(tauri::generate_context!())                      // 按 tauri.conf 启动
        .expect("error while running tauri application");     // 启动失败时退出
}
