use crate::client::client_trait::*;
use crate::client::ops::acl::{
    acl_build_rules, acl_cat0, acl_dryrun0, acl_genpass0, acl_getuser0, acl_list_users0, acl_log0,
    acl_users0, acl_whoami0,
};
use crate::client::ops::as_cmd::{get_field_as_command0, get_key_as_command0};
use crate::client::ops::field_scan::{field_scan0, zset_range0, zset_rank0};
use crate::client::ops::import_export::{
    export_cmd_0_thread, export_csv_0_thread, export_import_check_running, import_cmd_0_thread,
    import_csv_0_thread,
};
use crate::client::ops::info::{
    ar_info0, ar_last_items0, flush_all0, flush_db0, key_type0, object_info0, ts_info0,
    xinfo_consumers0, xinfo_groups0,
};
use crate::client::ops::key::{
    copy0, del0, field_add0, field_del0, field_get0, field_pop0, field_set0, field_ttl0,
    hash_keys0, hash_values0, set0, ttl0,
};
use crate::client::ops::pubsub::{monitor_stop0, monitor0, publish0, subscribe_stop0, subscribe0};
use crate::client::ops::scan::{batch_key0, scan_0_batch_count, scan_0_exact, scan_1_cmd};
use crate::client::ops::vector::{v_getattr0, v_info0, v_setattr0, v_sim0};
use crate::client::state::MeBase;
use crate::implement_pipeline_commands;
use crate::model::*;
use crate::net::conn::{get_client_single, init_single_connection, set_client_name_unless_minimal};
use crate::support::capabilities::detect_server_capabilities;
use crate::support::command_log::LoggingConnection;
use crate::support::error::AppError;
use crate::support::util::*;
use anyhow::bail;
use chrono::Utc;
use log::{debug, info, warn};
use parking_lot::{Mutex, MutexGuard};
use redis::{Client, Commands, Connection, ConnectionLike, Pipeline, Value};
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::atomic::Ordering::Relaxed;
use std::thread;
use std::time::Duration;

/// 单机 Redis 客户端。
pub struct MeSingle {
    base: MeBase,
    client: Client,
    conn: Mutex<LoggingConnection>,
}

impl Deref for MeSingle {
    type Target = MeBase;

    /// 让单机客户端直接用 `MeBase` 上的字段。
    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl Drop for MeSingle {
    /// 关掉还在跑的订阅和 MONITOR。连接已经断了就忽略错误。
    fn drop(&mut self) {
        // Drop 时静默忽略错误（连接可能已关闭）
        let _ = self.subscribe_stop();
        let _ = self.monitor_stop();
        self.export_import_running.store(false, Relaxed);
    }
}

impl MeClient for MeSingle {
    /// 这条连接的运行时状态。
    fn base(&self) -> &MeBase {
        &self.base
    }

    /// 数据库列表。
    fn db_list(&self) -> AnyResult<Vec<RedisDB>> {
        let map = match self.config_get("databases", None) {
            Ok(map) => map,
            Err(e) => {
                let db = self.db.load(Relaxed);
                warn!("CONFIG GET databases 不可用，退回当前库 db{db}: {e}");
                return Ok(vec![RedisDB { db, size: 0 }]);
            }
        };
        let db_count = map
            .get("databases")
            .unwrap_or(&"0".to_string())
            .parse::<u16>()?;
        info!("db_count: {}", db_count);
        let mut db_list = vec![];
        for i in 0..db_count {
            db_list.push(RedisDB { db: i, size: 0 })
        }
        Ok(db_list)
    }

    /// 切换当前库。
    fn select_db(&self, db: u16) -> AnyResult<()> {
        if self.db.load(Relaxed) == db {
            return Ok(());
        }

        self.db.store(db, Relaxed);
        let mut conn = self.get_conn()?;
        let _: () = redis::cmd("select").arg(db).query(&mut conn)?;
        conn.set_db_index(db);
        info!("select db: {}", db);
        Ok(())
    }

    /// 单节点 `INFO`。集群可指定节点。
    fn info(&self, _node: Option<String>) -> AnyResult<RedisInfo> {
        let mut conn = self.get_conn()?;
        let info: String = redis::cmd("info").query(&mut conn)?;
        Ok(RedisInfo {
            node: "".to_string(),
            info,
        })
    }

    /// 每个节点一份 `INFO`。单机只有一条。
    fn info_list(&self) -> AnyResult<Vec<RedisInfo>> {
        let info = self.info(None)?;
        Ok(vec![info])
    }

    /// 集群节点列表。单机返回空。
    fn node_list(&self) -> AnyResult<Vec<RedisNode>> {
        Ok(vec![])
    }

    /// 按 pattern 扫描键。
    fn scan(&self, param: ScanParam) -> AnyResult<ScanResult> {
        let mut conn = self.get_conn()?;

        // exact=true → EXISTS；否则 SCAN
        if let Some(result) = scan_0_exact(&mut conn, &param.pattern, param.exact)? {
            return Ok(result);
        }

        let mut cc = param.cursor.unwrap_or_default();
        let batch_count = scan_0_batch_count(param.count);

        // 只执行一次 SCAN，扫描次数和数据量判断完全由前端控制
        let cmd = scan_1_cmd(
            cc.now_cursor,
            &param.pattern,
            batch_count,
            param.scan_type.clone(),
        );
        let (next_cursor, new_keys): (u64, Vec<Vec<u8>>) = cmd.query(&mut conn)?;

        cc.now_cursor = next_cursor;
        if next_cursor == 0 {
            cc.finished = true;
        }

        Ok(ScanResult {
            cursor: cc,
            key_list: ui_key_list(new_keys),
        })
    }

    /// 扫描一个键里面的字段或元素。
    fn field_scan(&self, param: FieldScanParam) -> AnyResult<FieldScanResult> {
        let httl_supported = self.base().capabilities.httl_supported;
        field_scan0(self.get_conn()?, param, httl_supported)
    }

    /// 设置键的 TTL，秒。负数表示取消过期。
    fn ttl(&self, key: RedisKey, ttl: i64) -> AnyResult<()> {
        ttl0(self.get_conn()?, key, ttl)
    }

    /// 按类型写入整个键。
    fn set(&self, param: RedisSetParam) -> AnyResult<()> {
        set0(self.get_conn()?, param)
    }

    /// 删除键。
    fn del(&self, key: RedisKey) -> AnyResult<()> {
        del0(self.get_conn()?, key)
    }

    /// 重命名。集群上可能跨 slot。
    fn rename(&self, key: RedisKey, new_key: RedisKey) -> AnyResult<RedisKey> {
        // 防止同名重命名时执行无意义操作
        if key.to_bytes() == new_key.to_bytes() {
            return Ok(new_key.to_normal());
        }

        let mut conn = self.get_conn()?;
        // https://redis.ac.cn/docs/latest/commands/rename/
        let _: () = conn.rename(&key, &new_key)?;
        Ok(new_key.to_normal())
    }

    /// 复制键。
    fn copy(&self, param: RedisCopyParam) -> AnyResult<RedisKey> {
        copy0(self.get_conn()?, param)
    }

    /// 给已有键新增字段或元素。
    fn field_add(&self, param: RedisFieldAdd) -> AnyResult<RedisKey> {
        field_add0(
            self.get_conn()?,
            param,
            self.base().capabilities.httl_supported,
        )
    }

    /// 改一个字段或元素。
    fn field_set(&self, param: RedisFieldSet) -> AnyResult<()> {
        field_set0(
            self.get_conn()?,
            param,
            self.base().capabilities.httl_supported,
        )
    }

    /// Hash 字段过期（`HEXPIRE` / `HPERSIST`）。
    fn field_ttl(&self, param: RedisFieldTtl) -> AnyResult<()> {
        field_ttl0(
            self.get_conn()?,
            param,
            self.base().capabilities.httl_supported,
        )
    }

    /// 读取单条字段。
    fn field_get(&self, param: RedisFieldGet) -> AnyResult<RedisFieldValue> {
        field_get0(
            self.get_conn()?,
            param,
            self.base().capabilities.httl_supported,
        )
    }

    /// Hash 全量字段名。
    fn hash_keys(&self, param: RedisHashKeys) -> AnyResult<Vec<String>> {
        hash_keys0(self.get_conn()?, param)
    }

    /// Hash 全量字段值。
    fn hash_values(&self, param: RedisHashKeys) -> AnyResult<Vec<String>> {
        hash_values0(self.get_conn()?, param)
    }

    /// List / Set / ZSet 弹出元素。
    fn field_pop(&self, param: RedisPop) -> AnyResult<String> {
        field_pop0(self.get_conn()?, param)
    }

    /// 删除字段或元素。
    fn field_del(&self, param: RedisFieldDel) -> AnyResult<()> {
        field_del0(self.get_conn()?, param)
    }

    /// ZSet 排名。
    fn zset_rank(&self, param: RedisZsetRank) -> AnyResult<RedisZsetRankResult> {
        zset_rank0(self.get_conn()?, param)
    }

    /// ZSet 按名次取一段。
    fn zset_range(&self, param: RedisZsetRange) -> AnyResult<Vec<RedisZsetRangeItem>> {
        zset_range0(self.get_conn()?, param)
    }

    /// Array 尾部若干元素。
    fn ar_last_items(&self, param: RedisArLastItems) -> AnyResult<Vec<RedisArLastItemsItem>> {
        ar_last_items0(self.get_conn()?, param)
    }

    /// Array `ARINFO`。
    fn ar_info(&self, key: RedisKey) -> AnyResult<Vec<RedisArInfoItem>> {
        ar_info0(self.get_conn()?, key)
    }

    /// Vector Set `VINFO`，行结构和 `ARINFO` 相同。
    fn v_info(&self, key: RedisKey) -> AnyResult<Vec<RedisArInfoItem>> {
        v_info0(self.get_conn()?, key)
    }

    /// TimeSeries `TS.INFO`，行结构和 `ARINFO` 相同。
    fn ts_info(&self, key: RedisKey) -> AnyResult<Vec<RedisArInfoItem>> {
        ts_info0(self.get_conn()?, key)
    }

    /// Vector Set `VGETATTR`。
    fn v_getattr(&self, param: RedisVAttr) -> AnyResult<String> {
        v_getattr0(self.get_conn()?, param)
    }

    /// Vector Set `VSETATTR`。空串表示删掉属性。
    fn v_setattr(&self, param: RedisVAttr) -> AnyResult<()> {
        v_setattr0(self.get_conn()?, param)
    }

    /// Vector Set `VSIM`。
    fn v_sim(&self, param: RedisVSim) -> AnyResult<Vec<RedisVSimItem>> {
        v_sim0(self.get_conn()?, param)
    }

    /// `OBJECT` 自省：编码、空闲时间、引用计数、访问频率。
    fn object_info(&self, key: RedisKey) -> AnyResult<RedisObjectInfo> {
        object_info0(self.get_conn()?, key)
    }

    /// 在终端里执行一条命令，返回 redis-cli 风格文本。
    fn execute_command(&self, param: RedisCommand) -> AnyResult<String> {
        let (cmd, args) = parse_command(param.command.as_str())?;
        if cmd.is_empty() {
            return Ok("".into());
        };

        let mut conn = self.get_conn()?;
        let value = redis::cmd(cmd.as_str()).arg(&args).query(&mut conn)?;
        Ok(redis_value_to_cli_display(
            value,
            param.output_mode,
            &cmd,
            &args,
        ))
    }

    /// `CONFIG GET`。
    fn config_get(
        &self,
        pattern: &str,
        _node: Option<String>,
    ) -> AnyResult<HashMap<String, String>> {
        let cmd = resolve_command_name(&self.conf, "config");
        let mut conn = self.get_conn()?;
        let result: HashMap<String, String> =
            redis::cmd(&cmd).arg("get").arg(pattern).query(&mut conn)?;
        Ok(result)
    }

    /// `CONFIG SET`。
    fn config_set(&self, key: &str, value: &str, _node: Option<String>) -> AnyResult<()> {
        let cmd = resolve_command_name(&self.conf, "config");
        let mut conn = self.get_conn()?;
        let _: () = redis::cmd(&cmd)
            .arg("set")
            .arg(key)
            .arg(value)
            .query(&mut conn)?;
        Ok(())
    }

    /// 慢日志。
    fn slow_log(&self, count: Option<u64>, _node: Option<String>) -> AnyResult<Vec<RedisSlowLog>> {
        let mut conn = self.get_conn()?;
        let mut logs = vec![];
        let value_list: Vec<Value> = redis::cmd("slowlog")
            .arg("get")
            .arg(count.unwrap_or(128))
            .query(&mut conn)?;
        for value in value_list {
            let log = redis_value_to_log(value, "")?;
            logs.push(log);
        }
        Ok(logs)
    }

    /// 对一批键 pipeline MEMORY USAGE（及可选 TYPE），只保留 >= size_limit 的。
    fn memory_usage_keys(
        &self,
        keys: &[RedisKey],
        size_limit: u64,
        need_key_type: bool,
    ) -> AnyResult<Vec<RedisKeySize>> {
        if keys.is_empty() {
            return Ok(vec![]);
        }
        let mut conn = self.get_conn()?;
        let mut pipe = Pipeline::with_capacity(keys.len());
        for key in keys {
            pipe.cmd("memory").arg("usage").arg(key.to_bytes());
        }
        let sizes: Vec<Option<u64>> = pipe.query(&mut conn)?;
        let mut out: Vec<(Vec<u8>, u64, String)> = vec![];
        for (index, size) in sizes.into_iter().enumerate() {
            if let Some(size) = size
                && size >= size_limit
            {
                out.push((keys[index].to_bytes().to_vec(), size, "unknown".into()));
            }
        }
        if need_key_type && !out.is_empty() {
            let mut pipe = Pipeline::with_capacity(out.len());
            for key in out.iter() {
                pipe.cmd("type").arg(&key.0);
            }
            let types: Vec<Option<String>> = pipe.query(&mut conn)?;
            for (index, key_type) in types.into_iter().enumerate() {
                out[index].2 = key_type.unwrap_or("deleted".into());
            }
        }
        Ok(tuple_to_key_size(out))
    }

    /// `CLIENT LIST`。
    fn client_list(
        &self,
        _node: Option<String>,
        client_type: Option<String>,
    ) -> AnyResult<Vec<RedisClientInfo>> {
        let mut conn = self.get_conn()?;
        let mut cmd = redis::cmd("client");
        cmd.arg("list");
        if let Some(ref client_type_val) = client_type
            && !client_type_val.is_empty()
        {
            cmd.arg("type").arg(client_type_val);
        }
        let client: String = cmd.query(&mut conn)?;

        let mut clients = vec![];
        for client_info in client.lines() {
            let client: RedisClientInfo = parse_client_info(client_info)?;
            clients.push(client);
        }
        Ok(clients)
    }

    /// `PUBLISH`。
    fn publish(&self, channel: &str, message: &str, msg_fmt: Option<BytesFormat>) -> AnyResult<()> {
        let fmt = msg_fmt.unwrap_or_default();
        publish0(self.get_conn()?, channel, message, &fmt)
    }

    /// 按模式订阅，空模式等价于 `*`。
    fn subscribe(&self, channel: Option<String>) -> AnyResult<()> {
        let conn = self
            .client
            .get_connection_with_timeout(self.connection_timeout)?;
        let running = self.subscribe_running.clone();
        let app_handle = self.base().get_app_handle()?;
        let logger = self.base().command_logger.clone();
        subscribe0(conn, running, app_handle, channel, self.id.clone(), logger)
    }

    /// 停掉当前订阅循环。
    fn subscribe_stop(&self) -> AnyResult<()> {
        subscribe_stop0(self.get_conn()?, self.subscribe_running.clone())
    }

    /// `MONITOR`。集群要指定节点。
    fn monitor(&self, _node: &str) -> AnyResult<()> {
        let conn = self
            .client
            .get_connection_with_timeout(self.connection_timeout)?;
        let running = self.monitor_running.clone();
        let app_handle = self.base().get_app_handle()?;
        let logger = self.base().command_logger.clone();
        monitor0(conn, running, app_handle, self.id.clone(), logger)
    }

    /// 停掉 `MONITOR`。
    fn monitor_stop(&self) -> AnyResult<()> {
        monitor_stop0(self.monitor_running.clone())
    }

    /// 按 pattern 或键列表批量删除。
    fn batch_del(&self, param: RedisBatchKey) -> AnyResult<()> {
        let key_list = batch_key0(self, param, false)?;
        if key_list.is_empty() {
            return Ok(());
        }

        let size = key_list.len();
        let mut pipe = Pipeline::with_capacity(size);
        for key in key_list {
            pipe.del(&key).ignore();
        }
        let mut conn = self.get_conn()?;
        let _: () = pipe.query(&mut conn)?;
        info!("batch delete finished: {}", size);
        Ok(())
    }

    /// 批量改 TTL。
    fn batch_ttl(&self, param: RedisBatchTtl) -> AnyResult<()> {
        if param.key_list.is_empty() {
            return Ok(());
        }

        let size = param.key_list.len();
        let mut pipe = Pipeline::with_capacity(size);
        for key in param.key_list {
            if param.ttl > 0 {
                pipe.expire(&key, param.ttl).ignore();
            } else {
                pipe.persist(&key).ignore();
            }
        }
        let mut conn = self.get_conn()?;
        let _: () = pipe.query(&mut conn)?;
        info!("batch ttl finished: {}", size);
        Ok(())
    }

    /// 导出 CSV 或命令文件。
    fn export_csv(&self, param: RedisExportCsv) -> AnyResult<()> {
        let key_list = batch_key0(self, param.clone().into(), true)?;
        let conn = self.get_new_conn()?;
        let logger = self.base().command_logger.clone();
        let db_index = self.db.load(Relaxed);
        let mut logging_conn = LoggingConnection::new(conn, logger, db_index);
        let running = self.export_import_running.clone();
        let id = self.id.clone();
        let app_handle = self.base().get_app_handle()?;
        export_import_check_running(running.clone())?;
        let export_format = param.export_format.clone();
        let file = param.file.clone();
        let with_ttl = param.with_ttl;
        thread::spawn(move || {
            if export_format == "cmd" {
                export_cmd_0_thread(
                    &mut logging_conn,
                    key_list,
                    file,
                    with_ttl,
                    running,
                    app_handle,
                    id,
                );
            } else {
                export_csv_0_thread(
                    &mut logging_conn,
                    key_list,
                    file,
                    with_ttl,
                    running,
                    app_handle,
                    id,
                );
            }
        });
        Ok(())
    }

    /// 从 CSV 导入。
    fn import_csv(&self, param: RedisImportCsv) -> AnyResult<()> {
        let conn = self.get_new_conn()?;
        let logger = self.base().command_logger.clone();
        let db_index = self.db.load(Relaxed);
        let mut logging_conn = LoggingConnection::new(conn, logger, db_index);
        let running = self.export_import_running.clone();
        let id = self.id.clone();
        let app_handle = self.base().get_app_handle()?;
        export_import_check_running(running.clone())?;
        thread::spawn(move || {
            import_csv_0_thread(&mut logging_conn, param, running, app_handle, id)
        });
        Ok(())
    }

    /// 按行执行命令文件。
    fn import_cmd(&self, file: String) -> AnyResult<()> {
        let conn = self.get_new_conn()?;
        let logger = self.base().command_logger.clone();
        let db_index = self.db.load(Relaxed);
        let mut logging_conn = LoggingConnection::new(conn, logger, db_index);
        let running = self.export_import_running.clone();
        let id = self.id.clone();
        let app_handle = self.base().get_app_handle()?;
        export_import_check_running(running.clone())?;
        thread::spawn(move || {
            import_cmd_0_thread(&mut logging_conn, file, running, app_handle, id)
        });
        Ok(())
    }

    /// `TYPE`。
    fn key_type(&self, key: RedisKey) -> AnyResult<String> {
        key_type0(self.get_conn()?, key)
    }

    /// 把整个键格式化成 redis-cli 命令。
    fn get_key_as_command(&self, key: RedisKey) -> AnyResult<String> {
        get_key_as_command0(self.get_conn()?, key)
    }

    /// 把表格里的一行格式化成命令。
    fn get_field_as_command(&self, param: RedisFieldAsCommand) -> AnyResult<String> {
        get_field_as_command0(self.get_conn()?, param)
    }

    /// `XINFO GROUPS`。
    fn xinfo_groups(&self, key: RedisKey) -> AnyResult<Vec<XInfoGroup>> {
        xinfo_groups0(self.get_conn()?, key)
    }

    /// `XINFO CONSUMERS`。
    fn xinfo_consumers(&self, key: RedisKey, group: String) -> AnyResult<Vec<XInfoConsumer>> {
        xinfo_consumers0(self.get_conn()?, key, group)
    }

    /// 键所在的集群 slot。
    fn key_slot(&self, _key: RedisKey) -> AnyResult<u64> {
        Ok(0)
    }

    /// 键所在的集群节点。
    fn key_node(&self, _key: RedisKey) -> AnyResult<Vec<RedisNode>> {
        let node = format!("{}:{}", self.conf.host, self.conf.port);
        Ok(vec![RedisNode {
            node,
            ..RedisNode::default()
        }])
    }

    /// `FLUSHDB`。
    fn flush_db(&self) -> AnyResult<()> {
        flush_db0(self.get_conn()?)
    }

    /// `FLUSHALL`。
    fn flush_all(&self) -> AnyResult<()> {
        flush_all0(self.get_conn()?)
    }

    /// `ACL USERS`，只要用户名。
    fn acl_users(&self) -> AnyResult<Vec<String>> {
        acl_users0(self.get_conn()?)
    }

    /// `ACL LIST` 解析后的用户详情。
    fn acl_list_users(&self) -> AnyResult<Vec<AclUserDetail>> {
        acl_list_users0(self.get_conn()?)
    }

    /// `ACL GETUSER`。
    fn acl_getuser(&self, username: &str) -> AnyResult<AclUserDetail> {
        acl_getuser0(self.get_conn()?, username)
    }

    /// `ACL SETUSER`。
    fn acl_setuser(&self, param: AclSetuserParam) -> AnyResult<()> {
        let rules = acl_build_rules(&param)?;
        let _: () = self
            .get_conn()?
            .acl_setuser_rules(&param.username, &rules)?;
        Ok(())
    }

    /// `ACL DELUSER`。
    fn acl_deluser(&self, usernames: Vec<String>) -> AnyResult<usize> {
        Ok(self.get_conn()?.acl_deluser(&usernames)?)
    }

    /// `ACL WHOAMI`。
    fn acl_whoami(&self) -> AnyResult<String> {
        acl_whoami0(self.get_conn()?)
    }

    /// `ACL CAT`。
    fn acl_cat(&self, category: Option<String>) -> AnyResult<Vec<String>> {
        acl_cat0(self.get_conn()?, category)
    }

    /// `ACL GENPASS`。
    fn acl_genpass(&self, bits: Option<i64>) -> AnyResult<String> {
        acl_genpass0(self.get_conn()?, bits)
    }

    /// `ACL SAVE`。
    fn acl_save(&self) -> AnyResult<()> {
        let _: () = self.get_conn()?.acl_save()?;
        Ok(())
    }

    /// `ACL LOAD`。
    fn acl_load(&self) -> AnyResult<()> {
        let _: () = self.get_conn()?.acl_load()?;
        Ok(())
    }

    /// `ACL LOG`。
    fn acl_log(&self, count: Option<u64>) -> AnyResult<Vec<AclLogEntry>> {
        acl_log0(self.get_conn()?, count)
    }

    /// `ACL LOG RESET`。
    fn acl_log_reset(&self) -> AnyResult<()> {
        let _: () = self.get_conn()?.acl_log_reset()?;
        Ok(())
    }

    /// `ACL DRYRUN`。
    fn acl_dryrun(&self, username: String, command: String) -> AnyResult<String> {
        acl_dryrun0(self.get_conn()?, username, command)
    }

    implement_pipeline_commands!(Pipeline);
}

// 个性化方法
impl MeSingle {
    /// 建连并完成选库、客户端名和能力探测，返回可给前端用的客户端。
    pub fn init(
        redis_conn: &ConnConfig,
        connect_timeout: Duration,
        command_timeout: Duration,
    ) -> AnyResult<Box<dyn MeClient>> {
        let (client, _) = get_client_single(redis_conn, connect_timeout, false, None)?;
        let mut base = MeBase::from(redis_conn);
        base.connection_timeout = connect_timeout;
        base.command_timeout = command_timeout;
        let logger = base.command_logger.clone();
        // 阶段 1 建连验证 + 阶段 2 正式命令超时；验证通过后复用同一条 TCP（#155）
        let raw_conn = init_single_connection(
            &client,
            redis_conn.db,
            connect_timeout,
            command_timeout,
            redis_conn,
        )?;
        let mut conn = LoggingConnection::new(raw_conn, logger, redis_conn.db);
        set_client_name_unless_minimal(&mut conn, redis_conn);
        detect_server_capabilities(&mut conn, &mut base, false);

        info!("Redis单机连接初始化成功: {}", redis_conn.name);

        Ok(Box::new(MeSingle {
            base,
            client,
            conn: Mutex::new(conn),
        }))
    }

    /// 重连或辅助连接：按建连超时建一条 TCP，建好后再切到正式命令超时。
    fn new_raw_conn(
        client: &Client,
        db: u16,
        connect_timeout: Duration,
        command_timeout: Duration,
    ) -> AnyResult<Connection> {
        let mut conn = client.get_connection_with_timeout(connect_timeout)?;
        conn.set_read_timeout(Some(command_timeout))?;
        conn.set_write_timeout(Some(command_timeout))?;
        if db != 0 {
            info!("select {db}");
            let _: () = redis::cmd("select")
                .arg(db)
                .query(&mut conn)
                .unwrap_or_else(|_| warn!("select {db} 失败，使用默认数据库0"));
        }
        Ok(conn)
    }

    /// 丢掉当前连接，按当前库号重新建连并写回客户端名。
    fn reconnect(&self) -> AnyResult<()> {
        let raw_conn = Self::new_raw_conn(
            &self.client,
            self.db.load(Relaxed),
            self.connection_timeout,
            self.command_timeout,
        )?;
        let mut conn_guard = self.conn.lock();
        *conn_guard =
            LoggingConnection::new(raw_conn, self.command_logger.clone(), self.db.load(Relaxed));
        set_client_name_unless_minimal(&mut *conn_guard, &self.conf);
        self.last_check_time.store(Utc::now().timestamp(), Relaxed);
        info!("Redis单机连接重连成功: {}", self.conf.name);
        Ok(())
    }

    /// 拿当前连接。超过检查间隔或连接已断时先探测，失败则重连。加锁超过 10 秒报超时。
    fn get_conn(&'_ self) -> AnyResult<MutexGuard<'_, LoggingConnection>> {
        // match self.conn.lock() {
        //     Ok(conn) => Ok(conn),
        //     Err(_) => {
        //         bail!("获取连接加锁失败");
        //     }
        // }
        // 标准库的Mutex不支持重入及超时时间设置，因此引入parking_lot解决此问题
        // 备注: parking_lot的 ReentrantMutexGuard 不支持 deref_mut 所以暂不支持重入
        match self.conn.try_lock_for(Duration::from_secs(10)) {
            Some(mut conn) => Ok({
                let curr = Utc::now().timestamp();
                let last = self.last_check_time.load(Relaxed);
                if conn.is_open() && curr - last < CONNECTION_CHECK_SECONDS {
                    conn
                } else {
                    self.last_check_time.store(curr, Relaxed);
                    if self.check_connection_timeout(&mut conn).unwrap_or(false) {
                        conn
                    } else {
                        drop(conn); // 此处一定要释放锁
                        self.reconnect()?;
                        self.get_conn()?
                    }
                }
            }),
            None => bail!(AppError::ConnectionLockTimeout),
        }
    }

    /// 用较短超时做一次存活探测，通过后把读写超时改回正式命令超时。
    fn check_connection_timeout(&self, conn: &mut LoggingConnection) -> AnyResult<bool> {
        conn.set_read_timeout(Some(CONNECTION_CHECK_TIMEOUT))?;
        conn.set_write_timeout(Some(CONNECTION_CHECK_TIMEOUT))?;
        if conn.check_connection() {
            conn.set_read_timeout(Some(self.command_timeout))?;
            conn.set_write_timeout(Some(self.command_timeout))?;
            debug!("检查Redis单机连接正常: {}", self.conf.name);
            Ok(true)
        } else {
            warn!("检查Redis单机连接异常: {}", self.conf.name);
            Ok(false)
        }
    }

    /// 另建一条连接，给导入导出这类后台线程用，不经过命令日志包装。
    fn get_new_conn(&self) -> AnyResult<Connection> {
        let mut conn = Self::new_raw_conn(
            &self.client,
            self.db.load(Relaxed),
            self.connection_timeout,
            self.command_timeout,
        )?;
        set_client_name_unless_minimal(&mut conn, &self.conf);
        Ok(conn)
    }
}
