//! `is_x509_v1_pem` 用固定证书样例区分 X.509 v1 和 v3。样例在 `fixtures/`。

use redis_me_lib::net::tls::is_x509_v1_pem;

const V1_PEM: &str = include_str!("fixtures/x509_v1.pem");
const V3_PEM: &str = include_str!("fixtures/x509_v3.pem");

/// X.509 v1 证书要认出来，rustls 装不进去。
#[test]
fn v1_pem_is_detected() {
    assert!(is_x509_v1_pem(V1_PEM.as_bytes()));
}

/// v3 证书不是 v1。
#[test]
fn v3_pem_is_not_v1() {
    assert!(!is_x509_v1_pem(V3_PEM.as_bytes()));
}

/// 非法 PEM 不当成 v1。
#[test]
fn invalid_pem_is_not_v1() {
    assert!(!is_x509_v1_pem(b"not a cert"));
}

/// 空内容不当成 v1。
#[test]
fn empty_is_not_v1() {
    assert!(!is_x509_v1_pem(b""));
}
