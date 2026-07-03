use crate::utils::error::AppError;
use crate::utils::model::{ConnConfig, SslOption};
use crate::utils::ssh_tunnel::SshTunnel;
use crate::utils::util::{AnyResult, CONNECTION_CHECK_TIMEOUT, parse_path};
use anyhow::{Context, bail};
use log::{info, warn};
use redis::cluster::{ClusterClient, ClusterConfig};
use redis::sentinel::{SentinelClientBuilder, SentinelServerType};
use redis::{
    Client, ClientTlsConfig, Commands, ConnectionAddr, ConnectionLike, TlsCertificates, TlsMode,
};
use std::fs;
use url::Url;

// 获取单机连接
pub fn get_client_single(conf: &ConnConfig) -> AnyResult<(Client, Option<SshTunnel>)> {
    // SSH 隧道不支持哨兵模式
    if conf.ssh && conf.sentinel {
        bail!(AppError::SentinelNotSupported);
    }

    // 如果启用 SSH 隧道，先建立隧道
    let ssh_tunnel = if conf.ssh {
        let tunnel = SshTunnel::start(&conf.ssh_option, &conf.host, conf.port)?;
        info!("SSH 隧道已建立，本地端口: {}", tunnel.local_port);
        Some(tunnel)
    } else {
        None
    };

    // 决定连接目标
    let (target_host, target_port) = if conf.ssh {
        ("127.0.0.1", ssh_tunnel.as_ref().unwrap().local_port)
    } else {
        (conf.host.as_str(), conf.port)
    };

    // 使用 url crate 构建 URL，自动处理密码中的特殊字符（如 &、@、: 等）
    let mut url = Url::parse(&format!(
        "{}://{}:{}",
        if conf.ssl { "rediss" } else { "redis" },
        target_host,
        target_port
    ))?;
    url.set_username(&conf.username).unwrap_or(());
    url.set_password(Some(&conf.password)).unwrap_or(());
    if conf.ssl {
        url.set_fragment(Some("insecure"));
    }

    let redis_url_log = format!(
        "{}://{}:******@{}:{}{}",
        url.scheme(),
        conf.username,
        target_host,
        target_port,
        url.fragment()
            .map(|f| format!("#{}", f))
            .unwrap_or_default()
    );
    info!("redis_url: {redis_url_log}");

    let certs = get_tls_certs(conf.ssl_option.clone())?;
    let mut client = if conf.ssl
        && let Some(tls) = certs
    {
        Client::build_with_tls(url.to_string(), tls)?
    } else {
        Client::open(url.to_string())?
    };
    // 测试连接是否可以成功，注意超时时间比较短，用户可以快速感知到。此连接使用后丢弃即可
    let mut conn = client.get_connection_with_timeout(CONNECTION_CHECK_TIMEOUT)?;
    let _: () = conn.ping()?;
    info!("Redis单机测试连接成功");

    // 哨兵模式 ==> 沿用上面的逻辑（避免默认超时时间太长，影响用户体验）
    if conf.sentinel {
        client = get_client_sentinel(conf)?;
    }
    Ok((client, ssh_tunnel))
}

fn get_client_sentinel(conf: &ConnConfig) -> AnyResult<Client> {
    let certs = get_tls_certs(conf.ssl_option.clone())?;
    let conf = conf.clone();
    let sentinel_option = conf.sentinel_option.clone();
    let client = if conf.ssl
        && let Some(tls) = certs
    {
        let addr = ConnectionAddr::TcpTls {
            host: conf.host,
            port: conf.port,
            insecure: true,
            tls_params: None,
        };
        let mut builder = SentinelClientBuilder::new(
            vec![addr],
            sentinel_option.master_name,
            SentinelServerType::Master,
        )?
        .set_client_to_redis_db(conf.db as i64)
        .set_client_to_redis_tls_mode(TlsMode::Secure)
        .set_client_to_redis_certificates(tls.clone())
        .set_client_to_sentinel_tls_mode(TlsMode::Secure)
        .set_client_to_sentinel_certificates(tls);
        // TODO ==> danger_accept_invalid_hostnames 改为 true（目前没有这个属性）
        // https://github.com/redis-rs/redis-rs/issues/1931

        if !conf.username.is_empty() {
            builder = builder.set_client_to_sentinel_username(conf.username);
        };
        if !conf.password.is_empty() {
            builder = builder.set_client_to_sentinel_password(conf.password);
        };
        if !sentinel_option.master_username.is_empty() {
            builder = builder.set_client_to_redis_username(sentinel_option.master_username);
        }
        if !sentinel_option.master_password.is_empty() {
            builder = builder.set_client_to_redis_password(sentinel_option.master_password);
        }
        builder.build()?.get_client()?
    } else {
        let addr = ConnectionAddr::Tcp(conf.host, conf.port);
        let mut builder = SentinelClientBuilder::new(
            vec![addr],
            sentinel_option.master_name,
            SentinelServerType::Master,
        )?
        .set_client_to_redis_db(conf.db as i64);
        if !conf.username.is_empty() {
            builder = builder.set_client_to_sentinel_username(conf.username);
        };
        if !conf.password.is_empty() {
            builder = builder.set_client_to_sentinel_password(conf.password);
        };
        if !sentinel_option.master_username.is_empty() {
            builder = builder.set_client_to_redis_username(sentinel_option.master_username);
        }
        if !sentinel_option.master_password.is_empty() {
            builder = builder.set_client_to_redis_password(sentinel_option.master_password);
        }
        builder.build()?.get_client()?
    };
    Ok(client)
}

// 获取集群连接
pub fn get_client_cluster(conf: &ConnConfig) -> AnyResult<ClusterClient> {
    // SSH 隧道不支持集群模式
    if conf.ssh {
        bail!(AppError::ClusterNotSupported);
    }

    // 使用 url crate 构建 URL，自动处理密码中的特殊字符
    let mut url = Url::parse(&format!(
        "{}://{}:{}",
        if conf.ssl { "rediss" } else { "redis" },
        conf.host,
        conf.port
    ))?;
    url.set_username(&conf.username).unwrap_or(());
    url.set_password(Some(&conf.password)).unwrap_or(());
    if conf.ssl {
        url.set_fragment(Some("insecure"));
    }

    info!(
        "redis_url: {}://{}:******@{}:{}{}",
        url.scheme(),
        conf.username,
        conf.host,
        conf.port,
        url.fragment()
            .map(|f| format!("#{}", f))
            .unwrap_or_default()
    );

    let mut builder = ClusterClient::builder(vec![url.to_string()]);
    if !conf.username.is_empty() {
        builder = builder.username(conf.username.clone());
    }
    if !conf.password.is_empty() {
        builder = builder.password(conf.password.clone());
    }
    if conf.ssl {
        builder = builder.danger_accept_invalid_hostnames(true);
        let certs = get_tls_certs(conf.ssl_option.clone())?;
        if let Some(certs) = certs {
            builder = builder.certs(certs);
        };
    }
    builder = builder.database_id(conf.db as i64);
    let client = builder.build()?;
    let cc = ClusterConfig::new().set_connection_timeout(CONNECTION_CHECK_TIMEOUT);
    let mut conn = client.get_connection_with_config(cc)?;
    let _: () = conn.ping()?;
    info!("测试集群测试连接成功");
    Ok(client)
}

// 获取证书
fn get_tls_certs(ssl_option: SslOption) -> AnyResult<Option<TlsCertificates>> {
    if ssl_option.key.is_empty() && ssl_option.cert.is_empty() && ssl_option.ca.is_empty() {
        return Ok(None);
    };
    let cert_vec8 = fs::read(parse_path(&ssl_option.cert)).context("公钥文件读取失败")?;
    let key_vec8 = fs::read(parse_path(&ssl_option.key)).context("私钥文件读取失败")?;
    let root_cert = if ssl_option.ca.is_empty() {
        None
    } else {
        Some(fs::read(parse_path(&ssl_option.ca)).context("授权文件读取失败")?)
    };
    let certs = TlsCertificates {
        client_tls: Some(ClientTlsConfig {
            client_cert: cert_vec8,
            client_key: key_vec8,
        }),
        root_cert,
    };
    Ok(Some(certs))
}

/// 设置客户端名称；无 CLIENT 权限时跳过，不影响连接
pub fn set_client_name(conn: &mut dyn ConnectionLike) {
    match redis::cmd("client")
        .arg("setname")
        .arg("RedisME")
        .query::<()>(conn)
    {
        Ok(()) => info!("client setname RedisME"),
        Err(e) => warn!("client setname 不可用，跳过: {e}"),
    }
}
