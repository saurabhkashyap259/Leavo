//! Random tokens and session lifetime rules (FR-005, research R4).
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use sha2::{Digest, Sha256};

/// A new 32-byte session token (base64url for the cookie) and its SHA-256 (for the database).
pub fn new_session_token() -> (String, [u8; 32]) {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let token = URL_SAFE_NO_PAD.encode(bytes);
    let hash = hash_token(&token);
    (token, hash)
}

pub fn hash_token(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

pub const SESSION_SHORT: i64 = 7;
pub const SESSION_LONG: i64 = 30;
pub const SESSION_MAX: i64 = 90;
/// Using a session within this window of its expiry extends it.
pub const SLIDE_WINDOW_HOURS: i64 = 24;

pub fn session_lifetime(keep_signed_in: bool) -> Duration {
    Duration::days(if keep_signed_in {
        SESSION_LONG
    } else {
        SESSION_SHORT
    })
}

/// (expires_at, absolute_expires_at) for a session created at `now`.
pub fn session_expiry(keep_signed_in: bool, now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    (
        now + session_lifetime(keep_signed_in),
        now + Duration::days(SESSION_MAX),
    )
}

/// New expiry if the session should slide at `now`; never past the absolute cap.
pub fn slide(
    expires_at: DateTime<Utc>,
    absolute_expires_at: DateTime<Utc>,
    keep_signed_in: bool,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    if expires_at - now > Duration::hours(SLIDE_WINDOW_HOURS) {
        return None;
    }
    let next = (now + session_lifetime(keep_signed_in)).min(absolute_expires_at);
    (next > expires_at).then_some(next)
}

/// Lifetime of a verification ticket (`/auth/verify` → `POST /orgs`).
pub const TICKET_TTL_SECONDS: i64 = 30 * 60;

fn ticket_mac(secret: &[u8], email: &str, purpose: &str, expires: i64) -> [u8; 32] {
    use hmac::{Hmac, Mac};
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts any key");
    mac.update(format!("ticket\n{purpose}\n{email}\n{expires}").as_bytes());
    mac.finalize().into_bytes().into()
}

/// A stateless proof that `email` was verified with a code for `purpose` at `now` (unix s):
/// `base64url(email).expiry.base64url(hmac)`.
pub fn sign_ticket(secret: &[u8], email: &str, purpose: &str, now: i64) -> String {
    let expires = now + TICKET_TTL_SECONDS;
    format!(
        "{}.{expires}.{}",
        URL_SAFE_NO_PAD.encode(email.as_bytes()),
        URL_SAFE_NO_PAD.encode(ticket_mac(secret, email, purpose, expires))
    )
}

/// The verified email if the ticket is authentic, for `purpose`, and not expired at `now`.
pub fn verify_ticket(secret: &[u8], ticket: &str, purpose: &str, now: i64) -> Option<String> {
    use subtle::ConstantTimeEq;
    let mut parts = ticket.split('.');
    let (email, expires, sig) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let email = String::from_utf8(URL_SAFE_NO_PAD.decode(email).ok()?).ok()?;
    let expires: i64 = expires.parse().ok()?;
    let sig = URL_SAFE_NO_PAD.decode(sig).ok()?;
    let expected = ticket_mac(secret, &email, purpose, expires);
    (bool::from(expected.as_slice().ct_eq(&sig)) && now < expires).then_some(email)
}

fn download_mac(secret: &[u8], org: uuid::Uuid, attachment: uuid::Uuid, expires: i64) -> [u8; 32] {
    use hmac::{Hmac, Mac};
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts any key");
    mac.update(format!("download\n{org}\n{attachment}\n{expires}").as_bytes());
    mac.finalize().into_bytes().into()
}

/// Signature for a short-lived attachment download link (local storage, research R14).
pub fn sign_download(
    secret: &[u8],
    org: uuid::Uuid,
    attachment: uuid::Uuid,
    expires: i64,
) -> String {
    URL_SAFE_NO_PAD.encode(download_mac(secret, org, attachment, expires))
}

/// True when `sig` is authentic for these values and `now` (unix s) is before `expires`.
pub fn verify_download(
    secret: &[u8],
    org: uuid::Uuid,
    attachment: uuid::Uuid,
    expires: i64,
    sig: &str,
    now: i64,
) -> bool {
    use subtle::ConstantTimeEq;
    let Ok(sig) = URL_SAFE_NO_PAD.decode(sig) else {
        return false;
    };
    let expected = download_mac(secret, org, attachment, expires);
    bool::from(expected.as_slice().ct_eq(&sig)) && now < expires
}

/// HMAC-SHA256 of `msg`, base64url without padding.
pub fn mac_b64(secret: &[u8], msg: &str) -> String {
    use hmac::{Hmac, Mac};
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts any key");
    mac.update(msg.as_bytes());
    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

/// Constant-time check of a [`mac_b64`] signature.
pub fn mac_matches(secret: &[u8], msg: &str, sig: &str) -> bool {
    use subtle::ConstantTimeEq;
    let Ok(sig) = URL_SAFE_NO_PAD.decode(sig) else {
        return false;
    };
    let Ok(expected) = URL_SAFE_NO_PAD.decode(mac_b64(secret, msg)) else {
        return false;
    };
    bool::from(expected.as_slice().ct_eq(&sig))
}
