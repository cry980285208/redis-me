// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// 桌面进程入口，真正的启动在库的 `run`。
fn main() {
    redis_me_lib::run()
}
