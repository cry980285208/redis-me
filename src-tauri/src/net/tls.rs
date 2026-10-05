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
