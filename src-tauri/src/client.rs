//! Redis 客户端。`MeClient` 由单机和集群分别实现，具体命令在 `ops`。
//! Redis 回复转成界面行在 `support::convert`，不放进 `ops`。

pub mod me_client;
pub mod me_cluster;
pub mod me_single;
pub mod state;

pub mod ops;
