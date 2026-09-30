//! Redis 客户端。`MeClient` 由单机和集群分别实现，具体命令在 `ops`。

pub mod client_trait;
pub mod impl_cluster;
pub mod impl_single;
pub mod state;

pub mod ops;
