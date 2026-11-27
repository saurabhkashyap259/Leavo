//! Start-up steps after migrations (configuration.md "Start-up and upgrade behaviour").
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgPool;

/// One-way fingerprint of the secret key; the key itself is never stored.
fn fingerprint(secret: &str) -> Vec<u8> {
    Sha256::digest(format!("leavo:secret-fingerprint:v1:{secret}").as_bytes()).to_vec()
}

/// Compare `LEAVO_SECRET_KEY` with the one this database last ran with (FR-063). When it has
/// changed: end every session on the instance, void unused sign-in and recovery codes and
/// unused email action links. Codes and links are HMAC-keyed already; sessions are not, so this
/// is what signs everyone out. Returns whether a rotation was applied. The first start only
/// records the fingerprint.
pub async fn apply_secret_rotation(
    owner: &PgPool,
    secret: &str,
    now: DateTime<Utc>,
) -> sqlx::Result<bool> {
    let new = fingerprint(secret);
    let mut tx = owner.begin().await?;
    let old: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT secret_fingerprint FROM instance_settings WHERE id = 1 FOR UPDATE",
    )
    .fetch_one(&mut *tx)
    .await?;
    let rotated = matches!(&old, Some(o) if *o != new);
    if rotated {
        sqlx::query("UPDATE sessions SET revoked_at = $1 WHERE revoked_at IS NULL")
            .bind(now)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE login_codes SET consumed_at = $1 WHERE consumed_at IS NULL")
            .bind(now)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE action_tokens SET used_at = $1 WHERE used_at IS NULL")
            .bind(now)
            .execute(&mut *tx)
            .await?;
    }
    if old.as_deref() != Some(new.as_slice()) {
        sqlx::query("UPDATE instance_settings SET secret_fingerprint = $1 WHERE id = 1")
            .bind(&new)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    if rotated {
        tracing::warn!("LEAVO_SECRET_KEY changed: everyone was signed out; unused codes and email links no longer work");
    }
    Ok(rotated)
}
