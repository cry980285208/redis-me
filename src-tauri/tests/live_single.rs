//! 单机往返。地址在 `common/conn.rs` 的 `single()`。连不上或断言失败则失败。
//!
//! 测这些（具体断言在 `common/check.rs`）：
//! - 中文键和值，以及非 UTF-8 用 Base64 读回原字节
//! - 键 SCAN 翻页，多页拼起来不丢不重
//! - Hash、List、Set、ZSet、Stream 的字段翻页；List、Stream 还核对顺序
//! - JSON、TimeSeries、Array、VectorSet：服务端没有对应命令就跳过
//! - RedisSearch：`FT.ALTER` 给 0 号库的临时索引加字段，没装模块就跳过
//!
//! 新增一种基础键类型或翻页规则时，在 `check.rs` 加断言，并在这里挂上。

#[path = "common/check.rs"]
#[allow(dead_code)]
mod check;
#[path = "common/live.rs"]
#[allow(dead_code)]
mod live;

use redis_me_lib::client::me_client::MeClient;

/// `single()` 返回 `None` 才跳过。当前配置总会去连，建连失败则测试失败。
fn client() -> Option<Box<dyn MeClient>> {
    match live::single_client() {
        Ok(None) => {
            eprintln!("skip: no redis single config");
            None
        }
        Ok(Some(client)) => Some(client),
        Err(err) => panic!("redis configured but not usable: {err}"),
    }
}

/// 中文和非 UTF-8 字符串能写进再读出。
#[test]
fn chinese_and_binary_string() {
    let Some(client) = client() else { return };
    check::chinese_and_binary_string(client.as_ref());
}

/// 键扫描要翻页，五把测试键都能收到。
#[test]
fn key_scan_collects_five() {
    let Some(client) = client() else { return };
    check::key_scan_collects_five(client.as_ref());
}

/// Hash、List、Set、ZSet、Stream 的字段页能翻完，顺序类型要看到未结束的第一页。
#[test]
fn hash_list_set_zset_stream_pages() {
    let Some(client) = client() else { return };
    check::hash_list_set_zset_stream_pages(client.as_ref());
}

/// JSON、TimeSeries、Array、Vector 只在服务端有对应模块时才测。
#[test]
fn optional_modules_when_present() {
    let Some(client) = client() else { return };
    check::optional_modules_when_present(client.as_ref());
}

/// 没装 RedisSearch 就跳过。结束时删掉临时索引，文档键本来就没写。
struct AlterIndexGuard<'a> {
    client: &'a dyn MeClient,
    index: &'a str,
}

impl Drop for AlterIndexGuard<'_> {
    fn drop(&mut self) {
        let _ = self.client.search_index_drop(self.index.to_string(), false);
    }
}

/// 索引修改：一条 FT.ALTER 加上字段；其它命令在本地拒绝，不发到服务器。
#[test]
fn search_index_alter_adds_field() {
    let Some(client) = client() else { return };
    if let Err(err) = client.search_index_names() {
        if live::unknown_command(err.as_ref()) {
            eprintln!("skip: RedisSearch is not loaded");
            return;
        }
        panic!("FT._LIST: {err}");
    }
    // RediSearch 只允许在 0 号库建索引。活测试默认连的是 15 号库。
    client.select_db(0).expect("SELECT 0");

    let index = "idx:redis-me:test:alter";
    let _guard = AlterIndexGuard {
        client: client.as_ref(),
        index,
    };
    client
        .search_index_create(
            [
                format!("FT.CREATE {index}"),
                "    ON HASH".into(),
                "    PREFIX 1 redis-me:test:alter:".into(),
                "    SCHEMA".into(),
                "      name TEXT".into(),
            ]
            .join("\n"),
        )
        .expect("FT.CREATE");

    client
        .search_index_alter(
            [
                format!("FT.ALTER {index}"),
                "    SCHEMA ADD".into(),
                "      city TEXT".into(),
                "      year NUMERIC".into(),
            ]
            .join("\n"),
        )
        .expect("FT.ALTER");

    let info = client
        .search_index_list()
        .expect("FT.INFO")
        .into_iter()
        .find(|row| row.name == index)
        .expect("index missing after alter");
    let attrs: Vec<&str> = info
        .fields
        .iter()
        .map(|field| field.attribute.as_str())
        .collect();
    assert!(attrs.contains(&"name"), "{attrs:?}");
    assert!(attrs.contains(&"city"), "{attrs:?}");
    assert!(attrs.contains(&"year"), "{attrs:?}");
    assert!(
        info.fields
            .iter()
            .any(|field| field.attribute == "year"
                && field.field_type.eq_ignore_ascii_case("NUMERIC")),
        "{:?}",
        info.fields
            .iter()
            .map(|field| (field.attribute.as_str(), field.field_type.as_str()))
            .collect::<Vec<_>>()
    );

    let rejected = client
        .search_index_alter(format!("FT.CREATE {index} ON HASH SCHEMA extra TEXT"))
        .expect_err("non FT.ALTER must be rejected locally");
    assert!(
        rejected.to_string().contains("search_alter_not_ft_alter"),
        "{rejected}"
    );

    let duplicate = client
        .search_index_alter(format!("FT.ALTER {index} SCHEMA ADD city TEXT"))
        .expect_err("duplicate field");
    let duplicate = duplicate.to_string().to_lowercase();
    assert!(
        duplicate.contains("duplicate") || duplicate.contains("already exists"),
        "{duplicate}"
    );

    client
        .search_index_drop(index.to_string(), false)
        .expect("FT.DROPINDEX");
    let names = client.search_index_names().expect("FT._LIST");
    assert!(!names.iter().any(|name| name == index), "{names:?}");
}
