//! Configuration from environment variables only (contracts/configuration.md, FR-053).
use ipnet::IpNet;
use std::collections::HashMap;
use std::net::SocketAddr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceMode {
    Single,
    Multi,
}

#[derive(Clone, PartialEq, Eq)]
pub enum StorageConfig {
    Local {
        path: String,
    },
    S3 {
        endpoint: String,
        region: String,
        bucket: String,
        access_key: String,
        secret_key: String,
    },
}

impl std::fmt::Debug for StorageConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local { path } => f.debug_struct("Local").field("path", path).finish(),
            Self::S3 {
                endpoint,
                region,
                bucket,
                ..
            } => f
                .debug_struct("S3")
                .field("endpoint", endpoint)
                .field("region", region)
                .field("bucket", bucket)
                .finish_non_exhaustive(),
        }
    }
}

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub base_url: String,
    pub secret_key: String,
    pub smtp_url: String,
    pub email_from: String,
    pub mode: InstanceMode,
    pub run_jobs: bool,
    pub bind: SocketAddr,
    pub trusted_proxies: Vec<IpNet>,
    pub storage: StorageConfig,
    pub update_check: bool,
    pub metrics_token: Option<String>,
    pub sentry_dsn: Option<String>,
    pub log_level: String,
    pub database_max_connections: u32,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("database_url", &"<redacted>")
            .field("base_url", &self.base_url)
            .field("secret_key", &"<redacted>")
            .field("smtp_url", &"<redacted>")
            .field("email_from", &self.email_from)
            .field("mode", &self.mode)
            .field("run_jobs", &self.run_jobs)
            .field("bind", &self.bind)
            .field("trusted_proxies", &self.trusted_proxies)
            .field("storage", &self.storage)
            .field("update_check", &self.update_check)
            .field(
                "metrics_token",
                &self.metrics_token.as_ref().map(|_| "<redacted>"),
            )
            .field("log_level", &self.log_level)
            .field("database_max_connections", &self.database_max_connections)
            .finish()
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("required environment variable {0} is not set")]
    Missing(String),
    #[error("LEAVO_SECRET_KEY must be at least 64 characters")]
    SecretTooShort,
    #[error("{key} has an invalid value: {value}")]
    Invalid { key: String, value: String },
}

pub const MIN_SECRET_LEN: usize = 64;

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_map(&std::env::vars().collect())
    }

    pub fn from_map(env: &HashMap<String, String>) -> Result<Self, ConfigError> {
        let get = |k: &str| {
            env.get(k)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let required = |k: &str| get(k).ok_or_else(|| ConfigError::Missing(k.to_string()));
        let invalid = |k: &str, v: &str| ConfigError::Invalid {
            key: k.to_string(),
            value: v.to_string(),
        };
        let boolean = |k: &str, default: bool| -> Result<bool, ConfigError> {
            match get(k).as_deref() {
                None => Ok(default),
                Some("true" | "1" | "yes") => Ok(true),
                Some("false" | "0" | "no") => Ok(false),
                Some(v) => Err(invalid(k, v)),
            }
        };

        let database_url = required("DATABASE_URL")?;
        let base_url = required("LEAVO_BASE_URL")?
            .trim_end_matches('/')
            .to_string();
        let secret_key = required("LEAVO_SECRET_KEY")?;
        if secret_key.len() < MIN_SECRET_LEN {
            return Err(ConfigError::SecretTooShort);
        }
        let smtp_url = required("SMTP_URL")?;
        let email_from = required("EMAIL_FROM")?;

        let mode = match get("LEAVO_MODE").as_deref() {
            None | Some("single") => InstanceMode::Single,
            Some("multi") => InstanceMode::Multi,
            Some(v) => return Err(invalid("LEAVO_MODE", v)),
        };

        let bind_raw = get("LEAVO_BIND").unwrap_or_else(|| "0.0.0.0:8080".into());
        let bind = bind_raw
            .parse()
            .map_err(|_| invalid("LEAVO_BIND", &bind_raw))?;

        let trusted_proxies = match get("LEAVO_TRUSTED_PROXIES") {
            None => Vec::new(),
            Some(list) => list
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.parse().map_err(|_| invalid("LEAVO_TRUSTED_PROXIES", s)))
                .collect::<Result<_, _>>()?,
        };

        let storage = match get("STORAGE").as_deref() {
            None | Some("local") => StorageConfig::Local {
                path: get("STORAGE_LOCAL_PATH").unwrap_or_else(|| "/data/uploads".into()),
            },
            Some("s3") => StorageConfig::S3 {
                endpoint: required("S3_ENDPOINT")?,
                region: get("S3_REGION").unwrap_or_else(|| "us-east-1".into()),
                bucket: required("S3_BUCKET")?,
                access_key: required("S3_ACCESS_KEY")?,
                secret_key: required("S3_SECRET_KEY")?,
            },
            Some(v) => return Err(invalid("STORAGE", v)),
        };

        let max_raw = get("DATABASE_MAX_CONNECTIONS").unwrap_or_else(|| "10".into());
        let database_max_connections = max_raw
            .parse::<u32>()
            .ok()
            .filter(|n| *n > 0)
            .ok_or_else(|| invalid("DATABASE_MAX_CONNECTIONS", &max_raw))?;

        Ok(Self {
            database_url,
            base_url,
            secret_key,
            smtp_url,
            email_from,
            mode,
            run_jobs: boolean("LEAVO_RUN_JOBS", true)?,
            bind,
            trusted_proxies,
            storage,
            update_check: boolean("LEAVO_UPDATE_CHECK", true)?,
            metrics_token: get("LEAVO_METRICS_TOKEN"),
            sentry_dsn: get("SENTRY_DSN"),
            log_level: get("LOG_LEVEL").unwrap_or_else(|| "info".into()),
            database_max_connections,
        })
    }
}
