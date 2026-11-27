//! T017 — configuration from environment variables (contracts/configuration.md).
use leavo_api::config::{Config, ConfigError, InstanceMode, StorageConfig};
use std::collections::HashMap;

fn base() -> HashMap<String, String> {
    [
        ("DATABASE_URL", "postgres://leavo:x@db:5432/leavo"),
        ("LEAVO_BASE_URL", "https://leave.acme.com"),
        ("LEAVO_SECRET_KEY", &"k".repeat(64)),
        ("SMTP_URL", "smtp://localhost:1025"),
        ("EMAIL_FROM", "Leavo <leave@acme.com>"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

#[test]
fn defaults_apply_when_optional_vars_missing() {
    let c = Config::from_map(&base()).unwrap();
    assert_eq!(c.mode, InstanceMode::Single);
    assert!(c.run_jobs);
    assert_eq!(c.bind.to_string(), "0.0.0.0:8080");
    assert!(matches!(c.storage, StorageConfig::Local { ref path } if path == "/data/uploads"));
    assert!(c.update_check);
    assert!(c.metrics_token.is_none());
    assert_eq!(c.database_max_connections, 10);
    assert!(c.trusted_proxies.is_empty());
}

#[test]
fn every_missing_required_var_is_named() {
    for key in [
        "DATABASE_URL",
        "LEAVO_BASE_URL",
        "LEAVO_SECRET_KEY",
        "SMTP_URL",
        "EMAIL_FROM",
    ] {
        let mut env = base();
        env.remove(key);
        match Config::from_map(&env) {
            Err(ConfigError::Missing(k)) => assert_eq!(k, key),
            other => panic!("{key}: expected Missing, got {other:?}"),
        }
    }
}

#[test]
fn short_secret_key_is_rejected() {
    let mut env = base();
    env.insert("LEAVO_SECRET_KEY".into(), "short".into());
    assert!(matches!(
        Config::from_map(&env),
        Err(ConfigError::SecretTooShort)
    ));
}

#[test]
fn optional_values_are_parsed() {
    let mut env = base();
    for (k, v) in [
        ("LEAVO_MODE", "multi"),
        ("LEAVO_RUN_JOBS", "false"),
        ("LEAVO_BIND", "127.0.0.1:9000"),
        ("LEAVO_TRUSTED_PROXIES", "10.0.0.0/8, 192.168.1.0/24"),
        ("STORAGE", "s3"),
        ("S3_ENDPOINT", "http://minio:9000"),
        ("S3_REGION", "us-east-1"),
        ("S3_BUCKET", "leavo"),
        ("S3_ACCESS_KEY", "ak"),
        ("S3_SECRET_KEY", "sk"),
        ("LEAVO_UPDATE_CHECK", "false"),
        ("LEAVO_METRICS_TOKEN", "m"),
        ("DATABASE_MAX_CONNECTIONS", "4"),
    ] {
        env.insert(k.into(), v.into());
    }
    let c = Config::from_map(&env).unwrap();
    assert_eq!(c.mode, InstanceMode::Multi);
    assert!(!c.run_jobs);
    assert_eq!(c.bind.to_string(), "127.0.0.1:9000");
    assert_eq!(c.trusted_proxies.len(), 2);
    assert!(matches!(c.storage, StorageConfig::S3 { ref bucket, .. } if bucket == "leavo"));
    assert!(!c.update_check);
    assert_eq!(c.metrics_token.as_deref(), Some("m"));
    assert_eq!(c.database_max_connections, 4);
}

#[test]
fn s3_storage_requires_its_variables() {
    let mut env = base();
    env.insert("STORAGE".into(), "s3".into());
    assert!(matches!(
        Config::from_map(&env),
        Err(ConfigError::Missing(_))
    ));
}

#[test]
fn invalid_values_are_reported() {
    let mut env = base();
    env.insert("LEAVO_MODE".into(), "cluster".into());
    assert!(matches!(
        Config::from_map(&env),
        Err(ConfigError::Invalid { .. })
    ));
}

#[test]
fn debug_output_never_contains_secrets() {
    let c = Config::from_map(&base()).unwrap();
    let dbg = format!("{c:?}");
    assert!(!dbg.contains(&"k".repeat(64)));
    assert!(!dbg.contains("leavo:x@"));
}
