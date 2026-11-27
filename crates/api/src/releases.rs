//! The latest published release, for the daily update check (FR-052). One anonymous GET to the
//! GitHub releases API; nothing about the instance is sent. Never called when
//! `LEAVO_UPDATE_CHECK=false`.
use async_trait::async_trait;
use leavo_worker::jobs::update_check::ReleaseSource;

/// The project's GitHub releases endpoint. Set to the final repository before the first
/// public release (the product name is still being decided).
pub const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/leavo-hq/leavo/releases/latest";

pub struct GitHubReleases;

#[async_trait]
impl ReleaseSource for GitHubReleases {
    async fn latest_version(&self) -> Result<String, String> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("leavo/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| e.to_string())?;
        let body: serde_json::Value = client
            .get(LATEST_RELEASE_URL)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        body["tag_name"]
            .as_str()
            .map(|t| t.trim_start_matches('v').to_string())
            .ok_or_else(|| "release without a tag_name".to_string())
    }
}
