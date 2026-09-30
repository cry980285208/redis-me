use crate::client::state::MeBase;
use crate::model::*;
use crate::support::util::*;
use std::collections::HashMap;

/// 单机和集群共用的 Redis 操作。两边各自实现，共享逻辑在 `ops` 的自由函数里。
pub trait MeClient: Send + Sync {
    /// 这条连接的运行时状态。
    fn base(&self) -> &MeBase;

    /// 连接显示名。
    fn name(&self) -> String {
        self.base().conf.name.clone()
    }

    /// 数据库列表。
    fn db_list(&self) -> AnyResult<Vec<RedisDB>>;

    /// 切换当前库。
    fn select_db(&self, db: u16) -> AnyResult<()>;

    /// 单节点 `INFO`。集群可指定节点。
    fn info(&self, node: Option<String>) -> AnyResult<RedisInfo>;

    /// 每个节点一份 `INFO`。单机只有一条。
    fn info_list(&self) -> AnyResult<Vec<RedisInfo>>;

    /// 把一份 `INFO` 收成图表数据。
    fn chart(&self, node: Option<String>) -> AnyResult<RedisChart> {
        info_to_chart(self.info(node)?)
    }

    /// 每个节点一张图。
    fn chart_list(&self) -> AnyResult<Vec<RedisChart>> {
        let info_list = self.info_list()?;
        info_list.into_iter().map(info_to_chart).collect()
    }

    /// 集群节点列表。单机返回空。
    fn node_list(&self) -> AnyResult<Vec<RedisNode>>;

    /// 按 pattern 扫描键。
    fn scan(&self, param: ScanParam) -> AnyResult<ScanResult>;

    /// 扫描一个键里面的字段或元素。
    fn field_scan(&self, param: FieldScanParam) -> AnyResult<FieldScanResult>;

    /// 设置键的 TTL，秒。负数表示取消过期。
    fn ttl(&self, key: RedisKey, ttl: i64) -> AnyResult<()>;

    /// 按类型写入整个键。
    fn set(&self, param: RedisSetParam) -> AnyResult<()>;

    /// 删除键。
    fn del(&self, key: RedisKey) -> AnyResult<()>;

    /// 重命名。集群上可能跨 slot。
    fn rename(&self, key: RedisKey, new_key: RedisKey) -> AnyResult<RedisKey>;

    /// 复制键。
    fn copy(&self, param: RedisCopyParam) -> AnyResult<RedisKey>;

    /// 给已有键新增字段或元素。
    fn field_add(&self, param: RedisFieldAdd) -> AnyResult<RedisKey>;

    /// 改一个字段或元素。
    fn field_set(&self, param: RedisFieldSet) -> AnyResult<()>;

    /// Hash 字段过期（`HEXPIRE` / `HPERSIST`）。
    fn field_ttl(&self, param: RedisFieldTtl) -> AnyResult<()>;

    /// 读取单条字段。
    fn field_get(&self, param: RedisFieldGet) -> AnyResult<RedisFieldValue>;

    /// Hash 全量字段名。
    fn hash_keys(&self, param: RedisHashKeys) -> AnyResult<Vec<String>>;

    /// Hash 全量字段值。
    fn hash_values(&self, param: RedisHashKeys) -> AnyResult<Vec<String>>;

    /// List / Set / ZSet 弹出元素。
    fn field_pop(&self, param: RedisPop) -> AnyResult<String>;

    /// 删除字段或元素。
    fn field_del(&self, param: RedisFieldDel) -> AnyResult<()>;

    /// ZSet 排名。
    fn zset_rank(&self, param: RedisZsetRank) -> AnyResult<RedisZsetRankResult>;

    /// ZSet 按名次取一段。
    fn zset_range(&self, param: RedisZsetRange) -> AnyResult<Vec<RedisZsetRangeItem>>;

    /// Array 尾部若干元素。
    fn ar_last_items(&self, param: RedisArLastItems) -> AnyResult<Vec<RedisArLastItemsItem>>;

    /// Array `ARINFO`。
    fn ar_info(&self, key: RedisKey) -> AnyResult<Vec<RedisArInfoItem>>;

    /// Vector Set `VINFO`，行结构和 `ARINFO` 相同。
    fn v_info(&self, key: RedisKey) -> AnyResult<Vec<RedisArInfoItem>>;

    /// TimeSeries `TS.INFO`，行结构和 `ARINFO` 相同。
    fn ts_info(&self, key: RedisKey) -> AnyResult<Vec<RedisArInfoItem>>;

    /// Vector Set `VGETATTR`。
    fn v_getattr(&self, param: RedisVAttr) -> AnyResult<String>;

    /// Vector Set `VSETATTR`。空串表示删掉属性。
    fn v_setattr(&self, param: RedisVAttr) -> AnyResult<()>;

    /// Vector Set `VSIM`。
    fn v_sim(&self, param: RedisVSim) -> AnyResult<Vec<RedisVSimItem>>;

    /// `OBJECT` 自省：编码、空闲时间、引用计数、访问频率。
    fn object_info(&self, key: RedisKey) -> AnyResult<RedisObjectInfo>;

    /// 在终端里执行一条命令，返回 redis-cli 风格文本。
    fn execute_command(&self, param: RedisCommand) -> AnyResult<String>;

    /// `CONFIG GET`。
    fn config_get(&self, pattern: &str, node: Option<String>)
    -> AnyResult<HashMap<String, String>>;

    /// `CONFIG SET`。
    fn config_set(&self, key: &str, value: &str, node: Option<String>) -> AnyResult<()>;

    /// 慢日志。
    fn slow_log(&self, count: Option<u64>, node: Option<String>) -> AnyResult<Vec<RedisSlowLog>>;

    /// 对一批键 pipeline MEMORY USAGE（及可选 TYPE），只保留 >= size_limit 的。
    fn memory_usage_keys(
        &self,
        keys: &[RedisKey],
        size_limit: u64,
        need_key_type: bool,
    ) -> AnyResult<Vec<RedisKeySize>>;

    /// 一轮：复用 `scan()` 游标（含集群主节点），再测这批键的内存。
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

    /// `CLIENT LIST`。
    fn client_list(
        &self,
        node: Option<String>,
        client_type: Option<String>,
    ) -> AnyResult<Vec<RedisClientInfo>>;

    /// `PUBLISH`。
    fn publish(&self, channel: &str, message: &str, msg_fmt: Option<BytesFormat>) -> AnyResult<()>;

    /// 按模式订阅，空模式等价于 `*`。
    fn subscribe(&self, channel: Option<String>) -> AnyResult<()>;
    /// 停掉当前订阅循环。
    fn subscribe_stop(&self) -> AnyResult<()>;

    /// `MONITOR`。集群要指定节点。
    fn monitor(&self, node: &str) -> AnyResult<()>;
    /// 停掉 `MONITOR`。
    fn monitor_stop(&self) -> AnyResult<()>;

    /// 按 pattern 或键列表批量删除。
    fn batch_del(&self, param: RedisBatchKey) -> AnyResult<()>;
    /// 批量改 TTL。
    fn batch_ttl(&self, param: RedisBatchTtl) -> AnyResult<()>;
    /// 导出 CSV 或命令文件。
    fn export_csv(&self, param: RedisExportCsv) -> AnyResult<()>;
    /// 从 CSV 导入。
    fn import_csv(&self, param: RedisImportCsv) -> AnyResult<()>;
    /// 按行执行命令文件。
    fn import_cmd(&self, file: String) -> AnyResult<()>;

    /// 写入一批随机样本。
    fn mock_data(&self, count: u64) -> AnyResult<()>;
    /// `TYPE`。
    fn key_type(&self, key: RedisKey) -> AnyResult<String>;
    /// 把整个键格式化成 redis-cli 命令。
    fn get_key_as_command(&self, key: RedisKey) -> AnyResult<String>;
    /// 把表格里的一行格式化成命令。
    fn get_field_as_command(&self, param: RedisFieldAsCommand) -> AnyResult<String>;
    /// `XINFO GROUPS`。
    fn xinfo_groups(&self, key: RedisKey) -> AnyResult<Vec<XInfoGroup>>;
    /// `XINFO CONSUMERS`。
    fn xinfo_consumers(&self, key: RedisKey, group: String) -> AnyResult<Vec<XInfoConsumer>>;
    /// 键所在的集群 slot。
    fn key_slot(&self, key: RedisKey) -> AnyResult<u64>;
    /// 键所在的集群节点。
    fn key_node(&self, key: RedisKey) -> AnyResult<Vec<RedisNode>>;
    /// `FLUSHDB`。
    fn flush_db(&self) -> AnyResult<()>;
    /// `FLUSHALL`。
    fn flush_all(&self) -> AnyResult<()>;

    /// `ACL USERS`，只要用户名。
    fn acl_users(&self) -> AnyResult<Vec<String>>;
    /// `ACL LIST` 解析后的用户详情。
    fn acl_list_users(&self) -> AnyResult<Vec<AclUserDetail>>;
    /// `ACL GETUSER`。
    fn acl_getuser(&self, username: &str) -> AnyResult<AclUserDetail>;
    /// `ACL SETUSER`。
    fn acl_setuser(&self, param: AclSetuserParam) -> AnyResult<()>;
    /// `ACL DELUSER`。
    fn acl_deluser(&self, usernames: Vec<String>) -> AnyResult<usize>;
    /// `ACL WHOAMI`。
    fn acl_whoami(&self) -> AnyResult<String>;
    /// `ACL CAT`。
    fn acl_cat(&self, category: Option<String>) -> AnyResult<Vec<String>>;
    /// `ACL GENPASS`。
    fn acl_genpass(&self, bits: Option<i64>) -> AnyResult<String>;
    /// `ACL SAVE`。
    fn acl_save(&self) -> AnyResult<()>;
    /// `ACL LOAD`。
    fn acl_load(&self) -> AnyResult<()>;
    /// `ACL LOG`。
    fn acl_log(&self, count: Option<u64>) -> AnyResult<Vec<AclLogEntry>>;
    /// `ACL LOG RESET`。
    fn acl_log_reset(&self) -> AnyResult<()>;
    /// `ACL DRYRUN`。
    fn acl_dryrun(&self, username: String, command: String) -> AnyResult<String>;

    /// 打开日志面板时拉一份快照。
    fn command_logs(&self, limit: Option<u64>) -> AnyResult<Vec<CommandLogEntry>> {
        Ok(self.base().command_logger.query(limit))
    }

    /// 清空这条连接的命令日志。
    fn command_logs_clear(&self) -> AnyResult<()> {
        self.base().command_logger.clear();
        Ok(())
    }
}
