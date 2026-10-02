use crate::client::ops::info::info_to_chart;
use crate::client::state::MeBase;
use crate::model::*;
use crate::support::error::AppError;
use crate::support::util::*;
use anyhow::bail;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[rustfmt::skip]
pub trait MeClient: Send + Sync {
    // 连接与库
    fn base(&self) -> &MeBase;
    fn db_list(&self) -> AnyResult<Vec<RedisDB>>;
    fn select_db(&self, db: u16) -> AnyResult<()>;

    // 信息与节点
    fn info(&self, node: Option<String>) -> AnyResult<RedisInfo>;
    fn info_list(&self) -> AnyResult<Vec<RedisInfo>>;
    fn node_list(&self) -> AnyResult<Vec<RedisNode>>;

    // 扫描
    fn scan(&self, param: ScanParam) -> AnyResult<ScanResult>;
    fn field_scan(&self, param: FieldScanParam) -> AnyResult<FieldScanResult>;

    // 键
    fn ttl(&self, key: RedisKey, ttl: i64) -> AnyResult<()>;
    fn set(&self, param: RedisSetParam) -> AnyResult<()>;
    fn del(&self, key: RedisKey) -> AnyResult<()>;
    fn rename(&self, key: RedisKey, new_key: RedisKey) -> AnyResult<RedisKey>;
    fn copy(&self, param: RedisCopyParam) -> AnyResult<RedisKey>;
    fn object_info(&self, key: RedisKey) -> AnyResult<RedisObjectInfo>;
    fn key_type(&self, key: RedisKey) -> AnyResult<String>;
    fn key_slot(&self, key: RedisKey) -> AnyResult<u64>;
    fn key_node(&self, key: RedisKey) -> AnyResult<Vec<RedisNode>>;
    fn batch_del(&self, param: RedisBatchKey) -> AnyResult<()>;
    fn batch_ttl(&self, param: RedisBatchTtl) -> AnyResult<()>;
    fn flush_db(&self) -> AnyResult<()>;
    fn flush_all(&self) -> AnyResult<()>;

    // 字段
    fn field_add(&self, param: RedisFieldAdd) -> AnyResult<RedisKey>;
    fn field_set(&self, param: RedisFieldSet) -> AnyResult<()>;
    fn field_ttl(&self, param: RedisFieldTtl) -> AnyResult<()>;
    fn field_get(&self, param: RedisFieldGet) -> AnyResult<RedisFieldValue>;
    fn hash_keys(&self, param: RedisHashKeys) -> AnyResult<Vec<String>>;
    fn hash_values(&self, param: RedisHashKeys) -> AnyResult<Vec<String>>;
    fn field_pop(&self, param: RedisPop) -> AnyResult<String>;
    fn field_del(&self, param: RedisFieldDel) -> AnyResult<()>;

    // 有序集合
    fn zset_rank(&self, param: RedisZsetRank) -> AnyResult<RedisZsetRankResult>;
    fn zset_range(&self, param: RedisZsetRange) -> AnyResult<Vec<RedisZsetRangeItem>>;

    // Array
    fn ar_last_items(&self, param: RedisArLastItems) -> AnyResult<Vec<RedisArLastItemsItem>>;
    fn ar_info(&self, key: RedisKey) -> AnyResult<Vec<RedisArInfoItem>>;

    // Vector Set
    fn v_info(&self, key: RedisKey) -> AnyResult<Vec<RedisArInfoItem>>;
    fn v_getattr(&self, param: RedisVAttr) -> AnyResult<String>;
    fn v_setattr(&self, param: RedisVAttr) -> AnyResult<()>;
    fn v_sim(&self, param: RedisVSim) -> AnyResult<Vec<RedisVSimItem>>;

    // TimeSeries
    fn ts_info(&self, key: RedisKey) -> AnyResult<Vec<RedisArInfoItem>>;

    // RedisSearch（索引工作区，不是键类型）
    fn search_index_list(&self) -> AnyResult<Vec<SearchIndexInfo>>;
    fn search_query(&self, param: SearchQueryParam) -> AnyResult<SearchQueryResult>;
    fn search_index_drop(&self, index: String) -> AnyResult<()>;
    /// `kind` 为 `bikes` 或 `movies`。索引已存在时不写数据。
    fn search_sample_load(&self, kind: String) -> AnyResult<SearchSampleResult>;

    // Stream
    fn xinfo_groups(&self, key: RedisKey) -> AnyResult<Vec<XInfoGroup>>;
    fn xinfo_consumers(&self, key: RedisKey, group: String) -> AnyResult<Vec<XInfoConsumer>>;

    // 命令
    fn execute_command(&self, param: RedisCommand) -> AnyResult<String>;
    fn get_key_as_command(&self, key: RedisKey) -> AnyResult<String>;
    fn get_field_as_command(&self, param: RedisFieldAsCommand) -> AnyResult<String>;

    // 配置与诊断
    fn config_get(&self, pattern: &str, node: Option<String>) -> AnyResult<HashMap<String, String>>;
    fn config_set(&self, key: &str, value: &str, node: Option<String>) -> AnyResult<()>;
    fn slow_log(&self, count: Option<u64>, node: Option<String>) -> AnyResult<Vec<RedisSlowLog>>;
    fn memory_usage_keys(&self, keys: &[RedisKey], size_limit: u64, need_key_type: bool) -> AnyResult<Vec<RedisKeySize>>;
    fn client_list(&self, node: Option<String>, client_type: Option<String>) -> AnyResult<Vec<RedisClientInfo>>;

    // 发布订阅与监视
    fn publish(&self, channel: &str, message: &str, msg_fmt: Option<BytesFormat>) -> AnyResult<()>;
    fn subscribe(&self, channel: Option<String>) -> AnyResult<()>;
    fn subscribe_stop(&self) -> AnyResult<()>;
    fn monitor(&self, node: &str) -> AnyResult<()>;
    fn monitor_stop(&self) -> AnyResult<()>;

    // 导入导出
    fn export_csv(&self, param: RedisExportCsv) -> AnyResult<()>;
    fn import_csv(&self, param: RedisImportCsv) -> AnyResult<()>;
    fn import_cmd(&self, file: String) -> AnyResult<()>;
    fn mock_data(&self, count: u64) -> AnyResult<()>;

    // ACL
    fn acl_users(&self) -> AnyResult<Vec<String>>;
    fn acl_list_users(&self) -> AnyResult<Vec<AclUserDetail>>;
    fn acl_getuser(&self, username: &str) -> AnyResult<AclUserDetail>;
    fn acl_setuser(&self, param: AclSetuserParam) -> AnyResult<()>;
    fn acl_deluser(&self, usernames: Vec<String>) -> AnyResult<usize>;
    fn acl_whoami(&self) -> AnyResult<String>;
    fn acl_cat(&self, category: Option<String>) -> AnyResult<Vec<String>>;
    fn acl_genpass(&self, bits: Option<i64>) -> AnyResult<String>;
    fn acl_save(&self) -> AnyResult<()>;
    fn acl_load(&self) -> AnyResult<()>;
    fn acl_log(&self, count: Option<u64>) -> AnyResult<Vec<AclLogEntry>>;
    fn acl_log_reset(&self) -> AnyResult<()>;
    fn acl_dryrun(&self, username: String, command: String) -> AnyResult<String>;

    // ------------------------------ 默认实现 ------------------------------

    /// 连接名称，取自配置。
    fn name(&self) -> String {
        self.base().conf.name.clone()
    }

    /// 单节点监控图，由 INFO 换算。
    fn chart(&self, node: Option<String>) -> AnyResult<RedisChart> {
        info_to_chart(self.info(node)?)
    }

    /// 各节点监控图，由 INFO 列表换算。
    fn chart_list(&self) -> AnyResult<Vec<RedisChart>> {
        let info_list = self.info_list()?;
        info_list.into_iter().map(info_to_chart).collect()
    }

    /// 按模式扫描一页键，再估算这些键的内存占用。
    fn memory_usage(&self, param: RedisMemoryParam) -> AnyResult<RedisMemoryResult> {
        let scan = self.scan(ScanParam {
            pattern: param.pattern.clone().unwrap_or_else(|| "*".into()),
            scan_type: None,
            cursor: param.cursor.clone(),
            exact: false,
            count: param.scan_count,
        })?;
        let scanned = scan.key_list.len() as u64;
        let key_list = self.memory_usage_keys(
            &scan.key_list,
            param.size_limit,
            param.need_key_type.unwrap_or(false),
        )?;
        Ok(RedisMemoryResult {
            key_list,
            cursor: scan.cursor,
            scanned,
        })
    }

    /// 已有导入或导出在跑时拒绝再开一个，并把自己标成进行中。返回的标记交给后台线程。
    fn export_import_check_running(&self) -> AnyResult<Arc<AtomicBool>> {
        let running = self.base().export_import_running.clone();
        if running.load(Ordering::Relaxed) {
            bail!(AppError::ExportImportRunning);
        }
        running.store(true, Ordering::Relaxed);
        Ok(running)
    }

    /// 查询本连接已记录的命令日志。
    fn command_logs(&self, limit: Option<u64>) -> AnyResult<Vec<CommandLogEntry>> {
        Ok(self.base().command_logger.query(limit))
    }

    /// 清空本连接的命令日志。
    fn command_logs_clear(&self) -> AnyResult<()> {
        self.base().command_logger.clear();
        Ok(())
    }
}
