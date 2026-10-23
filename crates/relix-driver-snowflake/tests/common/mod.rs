/*
 * @author t0bx
 * @since 23.10.2026
 */

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use relix_core::{ConnectionConfig, ConnectionSettings, Secret, TlsMode, TlsSettings};
use relix_testing::rsa_keys::TEST_RSA_PRIVATE_KEY_PEM;
use rsa::RsaPrivateKey;
use rsa::pkcs1::EncodeRsaPublicKey;
use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey, LineEnding};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::OnceLock;
use wiremock::{MockServer, Request};

pub const PASSPHRASE: &str = "correct horse battery staple";

pub fn private_key() -> &'static RsaPrivateKey {
    static KEY: OnceLock<RsaPrivateKey> = OnceLock::new();
    KEY.get_or_init(|| RsaPrivateKey::from_pkcs8_pem(TEST_RSA_PRIVATE_KEY_PEM).unwrap())
}

pub fn expected_fingerprint() -> String {
    let der = private_key().to_public_key().to_public_key_der().unwrap();
    format!("SHA256:{}", STANDARD.encode(Sha256::digest(der.as_bytes())))
}

pub fn public_key_pkcs1_der() -> Vec<u8> {
    private_key()
        .to_public_key()
        .to_pkcs1_der()
        .unwrap()
        .as_bytes()
        .to_vec()
}

pub fn write_key_file(encrypted: bool) -> PathBuf {
    let pem = if encrypted {
        let salt = [7u8; 16];
        let iv = [9u8; 16];
        let params =
            pkcs8::pkcs5::pbes2::Parameters::pbkdf2_sha256_aes256cbc(2048, &salt, &iv).unwrap();
        let document = private_key().to_pkcs8_der().unwrap();
        let info = pkcs8::PrivateKeyInfo::try_from(document.as_bytes()).unwrap();
        info.encrypt_with_params(params, PASSPHRASE)
            .unwrap()
            .to_pem("ENCRYPTED PRIVATE KEY", LineEnding::LF)
            .unwrap()
            .to_string()
    } else {
        TEST_RSA_PRIVATE_KEY_PEM.to_owned()
    };
    write_pem(&pem)
}

pub fn write_pem(pem: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("relix-snowflake-{}.p8", uuid::Uuid::new_v4()));
    std::fs::write(&path, pem).unwrap();
    path
}

pub fn config(server: &MockServer, properties: &[(&str, &str)]) -> ConnectionConfig {
    let address = server.address();
    let mut settings = ConnectionSettings {
        host: Some(address.ip().to_string()),
        port: Some(address.port()),
        username: Some("analyst".into()),
        database: Some("sales".into()),
        connect_timeout_secs: 5,
        tls: TlsSettings {
            mode: TlsMode::Disable,
            ca_certificate: None,
        },
        ..ConnectionSettings::default()
    };
    settings.properties.insert("account".into(), "acme".into());
    for (name, value) in properties {
        settings
            .properties
            .insert((*name).to_owned(), (*value).to_owned());
    }
    ConnectionConfig::new(settings, None)
}

pub fn key_pair_config(server: &MockServer) -> ConnectionConfig {
    let path = write_key_file(false);
    config(
        server,
        &[
            ("private_key_file", path.to_str().unwrap()),
            ("warehouse", "compute_wh"),
            ("role", "analyst_role"),
        ],
    )
}

pub fn with_password(mut config: ConnectionConfig, password: &str) -> ConnectionConfig {
    config.password = Some(Secret::new(password));
    config
}

pub fn column(name: &str, type_name: &str) -> Value {
    json!({"name": name, "type": type_name, "nullable": true, "scale": 0, "precision": 38})
}

/// A `ResultSet` body shaped like the SQL API v2 documentation.
pub fn result_set(columns: Vec<Value>, rows: Value, num_rows: u64, partitions: usize) -> Value {
    let partition_info: Vec<Value> = (0..partitions)
        .map(|_| json!({"rowCount": 2, "uncompressedSize": 64}))
        .collect();
    json!({
        "resultSetMetaData": {
            "numRows": num_rows,
            "format": "jsonv2",
            "partitionInfo": partition_info,
            "rowType": columns,
        },
        "data": rows,
        "code": "090001",
        "statementStatusUrl": "/api/v2/statements/01b2-handle?requestId=r",
        "requestId": "r",
        "sqlState": "00000",
        "statementHandle": "01b2-handle",
        "message": "Statement executed successfully.",
        "createdOn": 1_700_000_000_000u64,
    })
}

pub fn version_result() -> Value {
    result_set(
        vec![column("CURRENT_VERSION()", "text")],
        json!([["8.40.1"]]),
        1,
        1,
    )
}

pub fn running(handle: &str) -> Value {
    json!({
        "code": "333334",
        "message": "Asynchronous execution in progress. Use provided query id to perform query monitoring and management.",
        "statementHandle": handle,
        "statementStatusUrl": format!("/api/v2/statements/{handle}"),
    })
}

pub fn body(request: &Request) -> Value {
    serde_json::from_slice(&request.body).unwrap()
}

pub fn header<'a>(request: &'a Request, name: &str) -> &'a str {
    request
        .headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
}

pub fn query_param(request: &Request, name: &str) -> Option<String> {
    request
        .url
        .query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}
