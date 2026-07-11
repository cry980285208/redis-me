#![cfg_attr(test, allow(warnings))] // 整个文件在测试时禁用该警告

use crate::api_model;
use crate::utils::capabilities::ServerCapabilities;
use crate::utils::conn::{get_client_cluster, get_client_single};
use crate::utils::error::AppError;
use crate::utils::util::{
    AnyResult, vec8_to_display_string,
};
use chrono::Utc;
use redis::{RedisWrite, ToRedisArgs, ToSingleRedisArg};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU16};
use std::time::Duration;
use parking_lot::RwLock;
use tauri::AppHandle;

/// 终端输出格式，对应 redis-cli `--raw` / `--csv` / `--json`；默认 TTY
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CliOutputMode {
    #[default]
    Standard,
    Raw,
    Json,
    Csv,
}

/// 前后端 IPC 字节格式：utf8 文本或 base64 原始字节（hex/binary/msgpack 等视图格式在前端处理）
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum BytesFormat {
    #[default]
    UTF8, // 默认字符串（UTF-8 lossy）
    Base64, // 原始字节的 Base64 编码
}

/// 连接 meta 值（与前端 JSON 结构一致，供 specta 导出）
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(untagged)]
pub enum ConnMetaValue {
    String(String),
    Number(f64),
    Bool(bool),
    Object(HashMap<String, ConnMetaValue>),
    Array(Vec<ConnMetaValue>),
    Null,
}

// 连接信息
api_model!(
    #[derive(Default)]
    ConnConfig {
        id: String,
        name: String,

        host: String,
        port: u16,
        username: String,
        password: String,
        db: u16,

        // 集群模式
        cluster: bool,

        // SSL连接
        ssl: bool,
        ssl_option: SslOption,

        // 哨兵模式
        sentinel: bool,
        sentinel_option: SentinelOption,

        // SSH隧道
        ssh: bool,
        ssh_option: SshOption,

        // 扩展元信息（分组、命令映射、库别名等，与前端 conn.meta 一致）
        #[serde(default)]
        meta: HashMap<String, ConnMetaValue>,
    }
);

impl ConnConfig {
    /// 从 meta.commandMap 解析命令映射（键为小写原命令名）。
    pub fn command_map(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        let Some(ConnMetaValue::Object(obj)) = self.meta.get("commandMap") else {
            return map;
        };
        for (k, v) in obj {
            let ConnMetaValue::String(mapped) = v else {
                continue;
            };
            let cmd = k.trim().to_ascii_lowercase();
            let mapped = mapped.trim();
            if !cmd.is_empty() && !mapped.is_empty() {
                map.insert(cmd, mapped.to_string());
            }
        }
        map
    }
}

api_model!(
    #[derive(Default)]
    SslOption {
        key: String,
        cert: String,
        ca: String,
    }
);

api_model!(
    #[derive(Default)]
    SentinelOption {
        master_name: String,
        master_username: String,
        master_password: String,
    }
);

api_model!(
    #[derive(Default)]
    SshOption {
        host: String,
        port: u16,

        login_type: String, // pwd 用户名/密码, pkfile 私钥文件
        username: String,
        password: String,
        pkfile: String,
        passphrase: String,
    }
);

// 全局应用设置：由前端 settings 同步，新连接/重连时快照 command_timeout
api_model!(
    AppSettings {
        command_timeout_secs: u64,
    }
);

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            command_timeout_secs: crate::utils::util::CONNECTION_NORMAL_TIMEOUT.as_secs(),
        }
    }
}

impl AppSettings {
    pub fn normalized(self) -> Self {
        Self {
            command_timeout_secs: self.command_timeout_secs.clamp(5, 300),
        }
    }

    pub fn command_timeout(&self) -> Duration {
        Duration::from_secs(self.command_timeout_secs)
    }
}

impl ConnConfig {
    pub fn test(&self) -> AnyResult<()> {
        if self.cluster {
            get_client_cluster(self)?;
        } else {
            get_client_single(self)?;
        };
        // 单机模式返回的元组在测试后丢弃，SSH 隧道随之关闭
        // 集群模式不支持 SSH
        Ok(())
    }

    pub fn masters(&self) -> AnyResult<Vec<HashMap<String, String>>> {
        let mut conf = self.clone();
        conf.sentinel = false;
        let (client, _) = get_client_single(&conf)?;
        let mut conn = client.get_connection()?;
        let masters: Vec<HashMap<String, String>> =
            redis::cmd("sentinel").arg("masters").query(&mut conn)?;
        Ok(masters)
    }
}

// 客户端的公共属性（仅后端内部使用，不参与前端类型导出）
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
    /// 本连接命令执行日志（环形缓冲）
    pub command_logger: Arc<crate::utils::command_log::CommandLogger>,
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

            command_timeout: crate::utils::util::CONNECTION_NORMAL_TIMEOUT,
            command_logger: Arc::new(crate::utils::command_log::CommandLogger::new(
                conf.id.clone(),
                conf.name.clone(),
            )),
            app_handle: Arc::new(RwLock::new(None::<AppHandle>)),
            capabilities: ServerCapabilities::default(),
        }
    }
}

// 新增：MeBase 更新版本和能力的方法
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

// 数据库信息
api_model!(RedisDB { db: u16, size: u64 });

// 信息 图形
api_model!(
    #[derive(Default)]
    RedisChart {
        node: String,

        // db0:keys=1558,expires=0,avg_ttl=0,subexpiry=0; db1:keys=50,expires=0,avg_ttl=0,subexpiry=0
        key_total: u64,                 // 键总数
        connected_clients: u64,         // 客户端数量
        instantaneous_ops_per_sec: f64, // 命令执行数/秒
        used_memory: u64,               // 内存使用量
        instantaneous_input_kbps: f64,  // 网络输入
        instantaneous_output_kbps: f64, // 网络输出

        total_connections_received: u64, // 服务器接受的总连接数
        total_commands_processed: u64,   // 服务器处理的总命令数

        // 计算缓存命中率: Cache Hit Ratio = keyspace_hits / (keyspace_hits + keyspace_misses)
        keyspace_hits: u64,   // 在主字典中成功查找键的数量
        keyspace_misses: u64, // 在主字典中查找键失败的数量
        cache_hit_ratio: f64, // 缓存命中率
    }
);

// 信息 info命令
api_model!(RedisInfo {
    node: String,
    info: String,
});

// 集群节点
api_model!(
#[derive(Default)]
RedisNode {
    id: String,
    node: String,
    flags: String,
    slots: Option<String>,
    slave_of_node: Option<String>
});

// 扫描参数
api_model!(ScanParam {
    #[serde(rename = "match")]
    pattern: String,

    #[serde(rename = "type")]
    scan_type: Option<String>,

    cursor: Option<ScanCursor>,

    /// 完全匹配：true 时后端 EXISTS；false 时 SCAN
    exact: bool,
});

impl ScanParam {
    pub fn all(pattern: String) -> Self {
        ScanParam {
            pattern,
            scan_type: None,
            cursor: None,
            exact: false,
        }
    }
}

// fieldScan 按类型的扩展参数：Stream 范围、STRING 大值预览阈值等
api_model!(FiledScanMeta {
    /// Stream XREVRANGE 上界和下界
    max_id: String,
    min_id: String,
    /// STRING 全量加载字节上限；超过且未 force 时仅 GETRANGE 预览前 value_preview_bytes
    value_byte_limit: Option<u64>,
    value_preview_bytes: Option<u64>,
    force_full_value: Option<bool>,
    /// List LRANGE 下界；空则 0
    list_min_index: Option<i64>,
    /// List LRANGE 上界；空则 len-1
    list_max_index: Option<i64>,
    /// List 扫描方向：true 从 max 向 min，false 从 min 向 max
    list_desc: Option<bool>,
    /// Stream 扫描方向：true 从 max 向 min（XREVRANGE），false 从 min 向 max（XRANGE）
    stream_desc: Option<bool>,
});

api_model!(FieldScanParam {
    key: RedisKey,
    count: u64,
    cursor: Option<ScanCursor>,
    /// HSCAN/SSCAN/ZSCAN 的 MATCH pattern（前端字段名 match）
    #[serde(rename = "match")]
    pattern: String,
    /// 完全匹配：true 时走 HGET / SISMEMBER / ZSCORE
    exact: bool,
    meta: Option<FiledScanMeta>,
    bytes_format: Option<BytesFormat>, // 扫描/展示用字节格式
    /// 是否拉取 TYPE/TTL/MEMORY/HLEN；前端续扫时为 false
    include_meta: Option<bool>,
    /// 续扫时传入（include_meta=false），避免重复 TYPE
    key_type: Option<String>,
    /// Hash 扫描是否附带 HTTL（默认 false 以提速）
    include_field_ttl: Option<bool>,
});

api_model!(XInfoGroup{
    name: String,
    consumers: usize,
    pending: usize,
    last_delivered_id: String,
    entries_read: Option<usize>,
    lag: Option<usize>
});

api_model!(XInfoConsumer {
    name: String,
    pending: usize,
    idle: usize,
});

api_model!(
#[derive(Default)]
FieldScanValue {
    hash: Vec<RedisHashItem>,
    set: Vec<String>,
    zset: Vec<RedisZetItem>,
});

// 扫描游标
api_model!(
#[derive(Default)]
ScanCursor {
    ready_nodes: Vec<String>,
    now_node: String,
    now_cursor: u64,
    stream_cursor: String,
    finished: bool,
});

// 扫描结果
api_model!(ScanResult {
    key_list: Vec<RedisKey>,
    cursor: ScanCursor,
});

api_model!(FieldScanResult {
    #[serde(rename = "type")]
    key_type: String,
    ttl: i64,
    size: u64,
    #[specta(type = specta_typescript::Any)]
    value: serde_json::Value,
    cursor: ScanCursor,
    length: usize, // String/Hash字段：原始 bytes 长度；集合类型：元素总数(HLEN/LLEN/SCARD/ZCARD/XLEN)
    /// STRING 因超过 value_byte_limit 仅返回预览片段时为 true
    value_truncated: bool,
});

// Redis键: 由于键是字节存储的，考虑转换为utf-8字符串显示后可能会丢失信息，因此封装为对象
// 备注: 为了方便传输与前端对比是否相等，将bytes序列化为base64字符串。
//     （jackson针对bytes序列化, 默认会进行base64编码, 返回是字符串）
api_model!(RedisKey {
    key: String,    // 显示

    #[serde(with = "v8_base64")]
    #[specta(type = String)]
    bytes: Vec<u8>, // 修改、删除等依据 ==> 查询出来的二进制键（JSON 为 Base64 字符串）
});

impl RedisKey {
    pub fn to_bytes(&self) -> &[u8] {
        // 扫描出来的键进行修改或删除时, 传入bytes. 完全新增的键，传入字符串, bytes为空
        if self.bytes.is_empty() {
            self.key.as_bytes()
        } else {
            &self.bytes
        }
    }

    pub fn to_normal(&self) -> Self {
        if self.key.is_empty() {
            RedisKey::from(self.bytes.clone())
        } else if self.bytes.is_empty() {
            RedisKey::from(self.key.clone())
        } else {
            self.clone()
        }
    }
}

impl From<&str> for RedisKey {
    fn from(s: &str) -> Self {
        RedisKey {
            key: s.to_string(),
            bytes: Vec::from(s),
        }
    }
}
impl From<String> for RedisKey {
    fn from(s: String) -> Self {
        RedisKey {
            key: s.clone(),
            bytes: Vec::from(s),
        }
    }
}
impl From<Vec<u8>> for RedisKey {
    fn from(bytes: Vec<u8>) -> Self {
        RedisKey {
            key: vec8_to_display_string(&bytes),
            bytes,
        }
    }
}

impl From<RedisKey> for String {
    fn from(redis_key: RedisKey) -> Self {
        if redis_key.key.is_empty() {
            String::from_utf8_lossy(&redis_key.bytes).to_string()
        } else {
            redis_key.key.clone()
        }
    }
}

impl ToRedisArgs for RedisKey {
    fn write_redis_args<W>(&self, out: &mut W)
    where
        W: ?Sized + RedisWrite,
    {
        out.write_arg(self.to_bytes())
    }
}
impl ToSingleRedisArg for RedisKey {}

// 复制键：COPY source destination [DB destination-db] [REPLACE]
api_model!(RedisCopyParam {
    source: RedisKey,
    destination: RedisKey,
    db: u16,
});

// 批量删除
api_model!(RedisBatchKey {
    #[serde(rename = "match")]
    pattern: String,
    key_list: Vec<RedisKey>,
});

// 批量更新过期时间
api_model!(RedisBatchTtl {
    key_list: Vec<RedisKey>,
    ttl: i64
});

fn default_export_format() -> String {
    "csv".into()
}

// 导出（csv：DUMP 格式；cmd：redis-cli 可执行命令文本）
api_model!(RedisExportCsv {
    #[serde(rename = "match")]
    pattern: String,
    key_list: Vec<RedisKey>,
    file: String,
    with_ttl: bool,
    #[serde(default = "default_export_format")]
    export_format: String,
});

impl From<RedisExportCsv> for RedisBatchKey {
    fn from(value: RedisExportCsv) -> Self {
        RedisBatchKey {
            pattern: value.pattern,
            key_list: value.key_list,
        }
    }
}

// 导入
api_model!(RedisImportCsv {
    file: String,
    ttl: i64,
    handle_ttl: String, // TTL处理: 尝试读取 parse, 自定义 custom, 永久 forever
    handle_conflict: String, // 冲突处理: 覆盖 replace, 忽略 ignore
});

// Hash条目
api_model!(RedisHashItem{
    key: String,
    value: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    ttl: Option<i64>,
});

// List 条目（fieldScan 返回，index 为 Redis 列表下标）
api_model!(RedisListItem {
    index: i64,
    value: String,
});

// Zset条目
api_model!(RedisZetItem {
    value: String,
    score: f64,
});

// Stream条目
api_model!(RedisStreamItem {
    id: String,
    value: HashMap<String, String>, // map转化为的json字符串
});

// 字段新增
api_model!(RedisFieldAdd {
    /// 目标 Redis 键（与 `RedisFieldSet` / `RedisFieldDel` 一致）；`bytes` 为空时由 `key` 文本 + `key_fmt` 解析
    key: RedisKey,
    mode: String,    // key-新增键, field-新增字段

    #[serde(rename = "type")]
    key_type: String,
    ttl: i64,
    value: String, // 字段类型为String时的值

    list_push_method: String, // lpush, rpush
    field_value_list: Vec<RedisFieldValue>,
    stream_id: String, // stream

    /// 仅 Redis 顶层键名（`key`）如何解码为字节；不含 Hash/Stream 的字段名
    key_fmt: Option<BytesFormat>,
    /// 除 Redis 键名外的输入：String 值、Hash 字段名与值、List/Set/ZSet 成员、Stream 字段名与值等
    val_fmt: Option<BytesFormat>,

});

// 字段修改
api_model!(RedisFieldSet {
    key: RedisKey,
    src_field_value: String,
    field_index: isize,
    field_key: String,
    field_value: String,
    field_score: f64,
    field_ttl: i64, // 字段 TTL（秒），仅 Redis/Valkey >= 7.4
    /// true：界面展示/编辑字段 TTL；false：不拉取列表 TTL，保存时仍保留原有过期
    include_field_ttl: Option<bool>,
    /// 编辑字段时解析用户输入（含 Hash 字段名）；Redis 键由 `key` 承载，不再经此格式解析
    val_fmt: Option<BytesFormat>,
});

// Hash HKEYS / HVALS 共用参数
api_model!(RedisHashKeys {
    key: RedisKey,
    /// Hash 字段名/值解码格式，与 field_get / fieldScan 一致
    val_fmt: Option<BytesFormat>,
});

// List LPOP / RPOP：键走 RedisKey.to_bytes()，支持二进制键
api_model!(RedisListPop {
    key: RedisKey,
    /// left=LPOP，right=RPOP
    side: String,
    /// 弹出元素的展示格式
    val_fmt: Option<BytesFormat>,
});

api_model!(RedisFieldGet {
    key: RedisKey,
    field_index: isize,
    field_key: String,
    /// ZSet 成员定位；Hash 用 field_key、List 用 field_index
    field_value: String,
    /// 为 true 时对 Hash 执行 HTTL；默认 false
    include_field_ttl: Option<bool>,
    val_fmt: Option<BytesFormat>,
});

// 表格单行 → redis-cli 命令（与 RedisFieldDel 相同的行定位字段）
api_model!(RedisFieldAsCommand {
    key: RedisKey,
    field_index: isize,
    field_key: String,
    field_value: String,
    stream_id: String,
    val_fmt: Option<BytesFormat>,
});

// 字段值
api_model!(RedisFieldValue {
    field_key: String,
    field_value: String,
    field_score: f64,
    field_ttl: i64, // 字段 TTL（秒），仅 Redis/Valkey >= 7.4
});

// 字段删除
api_model!(RedisFieldDel {
    key: RedisKey,
    field_index: isize,
    field_key: String,
    field_value: String,
    stream_id: String, // stream
    val_fmt: Option<BytesFormat>, // 非 utf8 时 field_key/field_value 为 base64  wire 字符串
});

// 设置参数
api_model!(RedisSetParam {
    key: RedisKey,
    value: String,
    ttl: i64,
    key_type: Option<String>,
    input_format: Option<BytesFormat>,
});

// 执行命令
api_model!(RedisCommand {
    command: String,
    node: Option<String>,
    auto_broadcast: Option<bool>,
    /// 终端输出格式；`None` 等同 `standard`（TTY）
    output_mode: Option<CliOutputMode>,
});

// 命令执行日志条目
api_model!(CommandLogEntry {
    id: u64,
    timestamp: String,
    db_index: u16,
    command: String,
    args: Vec<String>,
    full_command: String,
    duration_ms: u64,
    error: Option<String>,
});

// 慢日志
api_model!(RedisSlowLog {
    node: String,
    id: u64,
    time: String,
    client: String,
    command: String,
    cost: f64,
    client_name: String
});

// 内存分析参数
api_model!(RedisMemoryParam {
    #[serde(rename = "match")]
    pattern: Option<String>, // 匹配模式

    size_limit: u64,   // 大小限制, 推荐: 100kb 即102400
    count_limit: u64,  // 数量限制, 推荐: 1000
    scan_count: u64,   // 每次扫描, 推荐: 1000
    scan_total: u64,   // 扫描数量限制, 推荐: 10000
    sleep_millis: u64, // 扫描间隔, 推荐: 1000

    need_key_type: Option<bool>, // 是否需要返回键类型
});

// 内存分析结果
api_model!(RedisKeySize {
    key: String,    // 显示

    #[serde(with = "v8_base64")]
    #[specta(type = String)]
    bytes: Vec<u8>, // 修改、删除等依据（JSON 为 Base64 字符串）

    #[serde(rename = "type")]
    key_type: String ,  // 类型
    size: u64,        // 大小
});

impl From<(Vec<u8>, u64, String)> for RedisKeySize {
    fn from((key, size, key_type): (Vec<u8>, u64, String)) -> Self {
        RedisKeySize {
            key: vec8_to_display_string(&key),
            bytes: key,
            size,
            key_type,
        }
    }
}

// 客户端（缺省字段：`parse_client_info` 未写入 JSON 时由结构体 `#[serde(default)]` 填 0 / ""）
api_model!(
    #[serde(default)]
    #[derive(Default)]
    RedisClientInfo {
        id: u64,        // 唯一的 64 位客户端 ID
        addr: String,   // 客户端的地址/端口
        laddr: String,  // 客户端连接到的本地地址/端口（绑定地址）
        fd: u64,        // 对应于套接字的文件描述符
        name: String,   // 客户端使用 CLIENT SETNAME 设置的名称
        age: u64,       // 连接的总持续时间（秒）
        idle: u64,      // 连接的空闲时间（秒）
        flags: String,  // 客户端标志（见下文）
        db: u64,        // 当前数据库 ID
        sub: u64,       // 频道订阅数
        psub: u64,      // 模式匹配订阅数
        ssub: u64,      // 分片频道订阅数。在 Redis 7.0.3 中添加
        multi: i64,     // MULTI/EXEC 上下文中的命令数（无事务时常为 -1）
        watch: u64,     // 此客户端当前正在监视的键数。在 Redis 7.4 中添加
        qbuf: u64,      // 查询缓冲区长度（0 表示没有待处理的查询）
        qbuf_free: u64, // 查询缓冲区的可用空间（0 表示缓冲区已满）
        argv_mem: u64,  // 下一个命令的不完整参数（已从查询缓冲区中提取）
        multi_mem: u64, // 缓冲的多命令使用的内存。在 Redis 7.0 中添加
        obl: u64,       // 输出缓冲区长度
        oll: u64,       // 输出列表长度（当缓冲区满时，回复在此列表中排队）
        omem: u64,      // 输出缓冲区内存使用情况
        tot_mem: u64,   // 此客户端在其各种缓冲区中消耗的总内存
        events: String, // 文件描述符事件（见下文）
        cmd: String,    // 执行的最后一条命令
        user: String,   // 客户端的已认证用户名
        redir: u64,     // 当前客户端跟踪重定向的客户端 id
        resp: u8,       // 客户端 RESP 协议版本。在 Redis 7.0 中添加
        rbp: u64,       // 客户端连接以来其读取缓冲区的峰值大小。在 Redis 7.0 中添加
        rbs: u64,       // 客户端读取缓冲区当前大小（字节）。在 Redis 7.0 中添加
        io_thread: u64, // 分配给客户端的 I/O 线程 ID。在 Redis 8.0 中添加
    }
);

api_model!(SubscribeEvent {
    id: String,
    datetime: String,
    channel: String,
    message: String,
});

api_model!(MonitorEvent {
    id: String,
    datetime: String,
    command: String,
});

api_model!(CommandLogEvent {
    id: String,
    entry: CommandLogEntry,
});

api_model!(ExportImportEvent {
    id: String,
    ok_count: u64,
    err_count: u64,
    total_count: u64,
    ignore_count: u64,
    finished: bool
});

// ACL 用户详情（由 ACL GETUSER 结构化转换而来）
api_model!(
    #[derive(Default)]
    AclUserDetail {
        username: String,
        enabled: bool,
        nopass: bool,
        flags: Vec<String>,
        password_hashes: Vec<String>,
        command_rules: Vec<String>,
        key_patterns: Vec<String>,
        channel_patterns: Vec<String>,
        selectors: Vec<String>
    }
);

// ACL SETUSER 参数（新建/更新用户）
api_model!(AclSetuserParam {
    username: String,
    enabled: bool,
    password_hashes: Vec<String>,
    command_rules: Vec<String>,
    key_patterns: Vec<String>,
    channel_patterns: Vec<String>,
    /// Redis 7.2+ selector，每条为 SETUSER 括号内规则串（如 `-@all +set ~key2`）
    selectors: Vec<String>,
});

// ACL LOG 条目结构（字段顺序与 Redis ACL LOG 文档一致）
api_model!(
    #[derive(Default)]
    AclLogEntry {
        count: u64,
        reason: String,
        context: String,
        object: String,
        username: String,
        age_seconds: f64,
        client_info: String,
        entry_id: u64,
        timestamp_created: u64,
        timestamp_last_updated: u64,
    }
);

//~~~~~ 自定义Vec<u8>序列化为Base64字符串
mod v8_base64 {
    use base64::Engine;
    use base64::prelude::BASE64_STANDARD;
    use serde::de::Error;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let base64_string = BASE64_STANDARD.encode(bytes);
        serializer.serialize_str(&base64_string)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let base64_string = String::deserialize(deserializer)?;
        let bytes = BASE64_STANDARD
            .decode(base64_string.as_bytes())
            .map_err(|e| Error::custom(format!("Base64 decode error: {}", e)))?;
        Ok(bytes)
    }
}
