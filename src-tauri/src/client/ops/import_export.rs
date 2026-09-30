use crate::client::ops::as_cmd::key_as_command_lines;
use crate::model::*;
use crate::support::error::AppError;
use crate::support::format::format_expire_command;
use crate::support::util::*;
use Ordering::Relaxed;
use anyhow::bail;
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use log::{info, warn};
use redis::Commands;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};

/// 已有导入或导出在跑时拒绝再开一个，并把自己标成进行中。
pub fn export_import_check_running(running: Arc<AtomicBool>) -> AnyResult<()> {
    if running.load(Relaxed) {
        bail!(AppError::ExportImportRunning)
    }
    running.store(true, Relaxed);
    Ok(())
}

/// 后台把键 DUMP 成 CSV。进度通过事件发给前端。
pub fn export_csv_0_thread(
    conn: &mut impl Commands,
    key_list: Vec<RedisKey>,
    file: String,
    with_ttl: bool,
    running: Arc<AtomicBool>,
    app_handle: AppHandle,
    id: String,
) {
    info!("export keys count: {}", key_list.len());
    let result = export_keys(
        conn,
        key_list,
        &file,
        with_ttl,
        running.clone(),
        app_handle,
        id,
    );
    match result {
        Ok(_) => info!("export keys ok"),
        Err(e) => warn!("export keys err: {e}"),
    }
    running.store(false, Relaxed);
}

/// 后台把键写成 redis-cli 可执行的命令文件。
pub fn export_cmd_0_thread(
    conn: &mut impl Commands,
    key_list: Vec<RedisKey>,
    file: String,
    with_ttl: bool,
    running: Arc<AtomicBool>,
    app_handle: AppHandle,
    id: String,
) {
    info!("export cmd keys count: {}", key_list.len());
    let result = export_keys_as_command(
        conn,
        key_list,
        &file,
        with_ttl,
        running.clone(),
        app_handle,
        id,
    );
    match result {
        Ok(_) => info!("export cmd keys ok"),
        Err(e) => warn!("export cmd keys err: {e}"),
    }
    running.store(false, Relaxed);
}

/// 后台从 DUMP CSV 恢复键。
pub fn import_csv_0_thread(
    conn: &mut impl Commands,
    param: RedisImportCsv,
    running: Arc<AtomicBool>,
    app_handle: AppHandle,
    id: String,
) {
    info!("import csv file: {}", param.file);
    let result = import_keys(conn, param, running.clone(), app_handle, id);
    match result {
        Ok(_) => info!("import csv file ok"),
        Err(e) => warn!("import csv file err: {e}"),
    }
    running.store(false, Relaxed);
}

/// 后台逐行执行命令文件。
pub fn import_cmd_0_thread(
    conn: &mut impl Commands,
    file: String,
    running: Arc<AtomicBool>,
    app_handle: AppHandle,
    id: String,
) {
    info!("import cmd file: {}", file);
    let result = import_cmds(conn, file, running.clone(), app_handle, id);
    match result {
        Ok(_) => info!("import cmd file ok"),
        Err(e) => warn!("import cmd file err: {e}"),
    }
    running.store(false, Relaxed);
}

// ------------------------------ 仅本文件使用 ------------------------------

/// 逐键写成命令。单个键失败记入错误数，不中断整次导出。
fn export_keys_as_command(
    mut conn: impl Commands,
    key_list: Vec<RedisKey>,
    file: &str,
    with_ttl: bool,
    running: Arc<AtomicBool>,
    app_handle: AppHandle,
    id: String,
) -> AnyResult<()> {
    info!("export cmd file: {}", file);
    let mut writer = BufWriter::new(File::create(file)?);
    let mut ok_count = 0;
    let mut err_count = 0;
    let total_count = key_list.len() as u64;
    for key in key_list {
        if running.load(Relaxed) {
            let result = export_key_as_command(&mut conn, &mut writer, key, with_ttl);
            match result {
                Ok(true) => ok_count += 1,
                Ok(false) => err_count += 1,
                Err(e) => {
                    warn!("export cmd key err: {e}");
                    err_count += 1;
                }
            }
            let event = ExportImportEvent {
                id: id.clone(),
                ok_count,
                err_count,
                total_count,
                ignore_count: 0,
                finished: false,
            };
            let _ = &app_handle.emit(EVENT_EXPORT, event);
        }
    }

    let event = ExportImportEvent {
        id: id.clone(),
        ok_count,
        err_count,
        total_count,
        ignore_count: 0,
        finished: true,
    };
    let _ = &app_handle.emit(EVENT_EXPORT, event);
    writer.flush()?;
    Ok(())
}

/// 写入单键命令行；返回 Ok(true) 表示有内容写出
fn export_key_as_command(
    conn: &mut impl Commands,
    writer: &mut BufWriter<File>,
    key: RedisKey,
    with_ttl: bool,
) -> AnyResult<bool> {
    let key_bytes = key.to_bytes();
    let lines = key_as_command_lines(conn, &key)?;
    if lines.is_empty() {
        return Ok(false);
    }
    for line in &lines {
        writeln!(writer, "{line}")?;
    }
    if with_ttl {
        let ttl = conn.ttl(&key)?;
        if ttl > 0 {
            writeln!(writer, "{}", format_expire_command(key_bytes, ttl))?;
        }
    }
    Ok(true)
}

/// 逐键 DUMP 成 CSV 的一行。单个键失败记入错误数。
fn export_keys(
    mut conn: impl Commands,
    key_list: Vec<RedisKey>,
    file: &str,
    with_ttl: bool,
    running: Arc<AtomicBool>,
    app_handle: AppHandle,
    id: String,
) -> AnyResult<()> {
    info!("export keys file: {}", file);
    let mut writer = BufWriter::new(File::create(file)?);
    let mut ok_count = 0;
    let mut err_count = 0;
    let total_count = key_list.len() as u64;
    for key in key_list {
        if running.load(Relaxed) {
            let result = export_key(&mut conn, &mut writer, key, with_ttl);
            match result {
                Ok(_) => ok_count += 1,
                Err(e) => {
                    warn!("export key err: {e}");
                    err_count += 1;
                }
            };
            // 通知导出进度
            let event = ExportImportEvent {
                id: id.clone(),
                ok_count,
                err_count,
                total_count,
                ignore_count: 0,
                finished: false,
            };
            let _ = &app_handle.emit(EVENT_EXPORT, event);
        }
    }

    let event = ExportImportEvent {
        id: id.clone(),
        ok_count,
        err_count,
        total_count,
        ignore_count: 0,
        finished: true,
    };
    let _ = &app_handle.emit(EVENT_EXPORT, event);
    writer.flush()?;
    Ok(())
}

/// 一个键写成 `base64(key),base64(dump),ttl`。TTL 用秒，不需要时写 -1。
fn export_key(
    conn: &mut impl Commands,
    writer: &mut BufWriter<File>,
    key: RedisKey,
    with_ttl: bool,
) -> AnyResult<()> {
    let ttl = if with_ttl { conn.ttl(&key)? } else { -1 };

    // https://redis.ac.cn/docs/latest/commands/dump/
    // DUMP key
    let bytes: Vec<u8> = redis::cmd("dump").arg(&key).query(conn)?;
    let key = BASE64_STANDARD.encode(key.to_bytes());
    let value = BASE64_STANDARD.encode(&bytes);
    // 文件写入一行
    writeln!(writer, "{key},{value},{ttl}")?;
    Ok(())
}

/// 读 CSV。冲突和 TTL 按参数处理，坏行记入错误数。
fn import_keys(
    conn: &mut impl Commands,
    param: RedisImportCsv,
    running: Arc<AtomicBool>,
    app_handle: AppHandle,
    id: String,
) -> AnyResult<()> {
    let reader = BufReader::new(File::open(&param.file)?);
    let total_count = reader.lines().count() as u64;
    info!("import keys count: {}", total_count);

    let mut ok_count = 0;
    let mut err_count = 0;
    let mut ignore_count = 0;

    let reader = BufReader::new(File::open(&param.file)?);
    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if running.load(Relaxed) {
            let result = import_key(
                conn,
                line,
                param.ttl,
                &param.handle_ttl,
                &param.handle_conflict,
            );
            match result {
                Ok(_) => ok_count += 1,
                Err(e) => {
                    // 文档说明: RESTORE will return a "Target key name is busy" error when key already exists unless you use the REPLACE modifier.
                    // 实际测试: Redis 8.4.0返回的错误: "BUSYKEY": Target key name already exists.
                    if e.to_string().contains("Target key name") {
                        ignore_count += 1;
                    } else {
                        warn!("import key err: {e}");
                        err_count += 1
                    }
                }
            };
            // 通知导入进度
            let event = ExportImportEvent {
                id: id.clone(),
                ok_count,
                err_count,
                total_count,
                ignore_count,
                finished: false,
            };
            let _ = &app_handle.emit(EVENT_IMPORT, event);
        }
    }

    let event = ExportImportEvent {
        id: id.clone(),
        ok_count,
        err_count,
        total_count,
        ignore_count,
        finished: true,
    };
    let _ = &app_handle.emit(EVENT_IMPORT, event);
    Ok(())
}

/// 用 `RESTORE` 写回一个键。`replace` 时覆盖已有键。
fn import_key(
    conn: &mut impl Commands,
    line: &str,
    ttl: i64,
    handle_ttl: &str,
    handle_conflict: &str,
) -> AnyResult<()> {
    let parts: Vec<&str> = line.split(',').collect();
    if parts.len() != 2 && parts.len() != 3 {
        bail!(AppError::ImportInvalidLine { line: line.into() })
    }

    let ttl_part = if parts.len() == 3 { parts[2] } else { "-1" };

    // https://redis.ac.cn/docs/latest/commands/restore/
    // RESTORE key ttl serialized-value [REPLACE] [ABSTTL] [IDLETIME seconds] [FREQ frequency]
    // 如果 ttl 为 0，则创建键时不设置过期时间；否则，设置指定的过期时间（以毫秒为单位）。
    // 除非使用 REPLACE 修饰符，否则当 key 已存在时，RESTORE 将返回“Target key name is busy”错误。
    let key = BASE64_STANDARD.decode(parts[0])?;
    let value = BASE64_STANDARD.decode(parts[1])?;
    let ttl = import_restore_ttl(ttl_part, ttl, handle_ttl);

    let mut cmd = redis::cmd("restore");
    cmd.arg(&key).arg(ttl).arg(value);
    if handle_conflict == "replace" {
        cmd.arg("replace");
    }
    let _: () = cmd.query(conn)?;
    Ok(())
}

/// 决定 `RESTORE` 的 TTL，单位是毫秒。忽略、已过期或解析失败时写成 0，表示永久。
fn import_restore_ttl(part_ttl: &str, ttl: i64, handle_ttl: &str) -> i64 {
    let ttl = match handle_ttl {
        "custom" => ttl,
        "parse" => part_ttl.parse::<i64>().unwrap_or(-1),
        _ => -1,
    };

    // 注意: 导出时TTL命令返回的单位是秒, restore的ttl参数是毫秒
    if ttl <= 0 { 0 } else { ttl * 1000 }
}

/// 读命令文件。空行跳过，单行失败记入错误数，不中断后面的行。
fn import_cmds(
    conn: &mut impl Commands,
    file: String,
    running: Arc<AtomicBool>,
    app_handle: AppHandle,
    id: String,
) -> AnyResult<()> {
    let reader = BufReader::new(File::open(&file)?);
    let total_count = reader.lines().count() as u64;
    info!("import cmds lines: {}", total_count);

    let mut ok_count = 0;
    let mut err_count = 0;

    let reader = BufReader::new(File::open(&file)?);
    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if running.load(Relaxed) {
            let result = import_cmd(conn, line);
            match result {
                Ok(_) => ok_count += 1,
                Err(e) => {
                    warn!("import cmd err: {e}");
                    err_count += 1
                }
            }
            // 通知导入进度
            let event = ExportImportEvent {
                id: id.clone(),
                ok_count,
                err_count,
                total_count,
                ignore_count: 0,
                finished: false,
            };
            let _ = &app_handle.emit(EVENT_IMPORT, event);
        }
    }

    let event = ExportImportEvent {
        id: id.clone(),
        ok_count,
        err_count,
        total_count,
        ignore_count: 0,
        finished: true,
    };
    let _ = &app_handle.emit(EVENT_IMPORT, event);
    Ok(())
}

/// 执行文件里的一行命令。
fn import_cmd(mut conn: &mut impl Commands, line: &str) -> AnyResult<()> {
    // 命令日志已经输出，这里不再输出
    //info!("line: {}", line);
    let (cmd, args) = parse_command(line)?;
    redis::cmd(cmd.as_str()).arg(args).exec(&mut conn)?;
    Ok(())
}

#[cfg(test)]
mod import_restore_ttl_tests {
    use super::*;

    /// ignore 和未知策略把键收成永久；自定义秒数换成毫秒，过期和解析失败落成 0。
    #[test]
    fn permanent_expired_and_ignore() {
        // ignore 以及其它未知策略：RESTORE ttl 0，键永久
        assert_eq!(import_restore_ttl("60", 30, "ignore"), 0);
        assert_eq!(import_restore_ttl("60", 30, ""), 0);
        // 自定义秒数换成毫秒
        assert_eq!(import_restore_ttl("60", 30, "custom"), 30_000);
        // 已过期、无 TTL、解析失败都落成 0
        assert_eq!(import_restore_ttl("-2", 0, "parse"), 0);
        assert_eq!(import_restore_ttl("0", -1, "custom"), 0);
        assert_eq!(import_restore_ttl("nope", 99, "parse"), 0);
        assert_eq!(import_restore_ttl("15", 0, "parse"), 15_000);
    }
}
