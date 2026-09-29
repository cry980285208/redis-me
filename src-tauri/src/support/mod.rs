//! 与某一次 Redis 命令无关的支撑：错误、工具函数、命令日志、宏和应用启动。

pub mod app_store;
pub mod capabilities;
pub mod command_log;
pub mod error;
pub mod macros;
pub mod setup;
pub mod util;
