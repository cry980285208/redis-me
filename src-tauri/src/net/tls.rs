use x509_parser::certificate::X509Certificate;
use x509_parser::pem::parse_x509_pem;
use x509_parser::prelude::FromDer;
use x509_parser::x509::X509Version;

/// PEM 是否为 X.509 v1（rustls/webpki 无法装入 RootCertStore 或 with_client_auth_cert 解析）
pub fn is_x509_v1_pem(pem: &[u8]) -> bool {
    let Ok((_, block)) = parse_x509_pem(pem) else {
        return false;
    };
    let Ok((_, cert)) = X509Certificate::from_der(&block.contents) else {
        return false;
    };
    cert.version() == X509Version::V1
}

#[cfg(test)]
mod tests {
    use super::*;

    const V1_PEM: &str = include_str!("testdata/x509_v1.pem");
    const V3_PEM: &str = include_str!("testdata/x509_v3.pem");

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
}
