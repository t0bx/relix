/*
 * @author t0bx
 * @since 24.10.2026
 */

/// RSA-2048 key generated only for tests that sign JWTs against local mock servers
/// (Snowflake key-pair authentication, Google service accounts). It protects nothing and
/// must never be registered or trusted anywhere. Shipping it avoids slow key generation
/// in unoptimized test builds.
pub const TEST_RSA_PRIVATE_KEY_PEM: &str = include_str!("fixtures/test_rsa_key.pem");

/// The same key encrypted exactly like Snowflake's documentation does it
/// (`openssl pkcs8 -topk8 -v2 des3`), protected by [`TEST_RSA_KEY_PASSPHRASE`].
pub const TEST_RSA_PRIVATE_KEY_DES3_PEM: &str = include_str!("fixtures/test_rsa_key_des3.p8");

pub const TEST_RSA_KEY_PASSPHRASE: &str = "relix-test-passphrase";
