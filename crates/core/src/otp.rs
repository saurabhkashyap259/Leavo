//! Six-digit email sign-in codes (FR-001, FR-002, research R4).
//!
//! Codes come from the OS CSPRNG, are stored only as HMAC-SHA256(secret, email:code), and are
//! compared in constant time.
use hmac::{Hmac, Mac};
use rand::Rng;
use sha2::Sha256;
use subtle::ConstantTimeEq;

pub const CODE_TTL_MINUTES: i64 = 10;
pub const MAX_WRONG_ATTEMPTS: i32 = 5;
pub const LOCK_MINUTES: i64 = 15;

/// A uniformly random code from 000000 to 999999.
pub fn generate_code() -> String {
    format!("{:06}", rand::rng().random_range(0..1_000_000u32))
}

pub fn hash_code(secret: &[u8], email: &str, code: &str) -> [u8; 32] {
    let email = email.trim().to_lowercase();
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(email.as_bytes());
    mac.update(b":");
    mac.update(code.trim().as_bytes());
    mac.finalize().into_bytes().into()
}

pub fn verify_code(secret: &[u8], email: &str, code: &str, stored: &[u8]) -> bool {
    let code = code.trim();
    if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let expected = hash_code(secret, email, code);
    stored.len() == expected.len() && bool::from(expected.ct_eq(stored))
}

/// Trim and lower-case; `None` unless it looks like `local@domain.tld`.
pub fn normalize_email(raw: &str) -> Option<String> {
    let e = raw.trim().to_lowercase();
    let (local, domain) = e.split_once('@')?;
    let ok = !local.is_empty()
        && local.len() <= 64
        && e.len() <= 254
        && !domain.contains('@')
        && !e.chars().any(char::is_whitespace)
        && domain.split('.').count() >= 2
        && domain.split('.').all(|l| !l.is_empty());
    ok.then_some(e)
}

pub fn domain_of(email: &str) -> Option<&str> {
    email.split_once('@').map(|(_, d)| d)
}
