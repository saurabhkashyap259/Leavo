//! Whitelisted email domains (FR-009): normalisation, hostname validation and the
//! public-provider blocklist.
use crate::error::ErrorCode;

/// Consumer email providers that can never be an organization's domain.
const PUBLIC_PROVIDERS: &[&str] = &[
    "aol.com",
    "fastmail.com",
    "gmail.com",
    "gmx.com",
    "gmx.net",
    "googlemail.com",
    "hey.com",
    "hotmail.com",
    "icloud.com",
    "live.com",
    "mac.com",
    "mail.com",
    "me.com",
    "msn.com",
    "outlook.com",
    "outlook.in",
    "pm.me",
    "proton.me",
    "protonmail.com",
    "rediffmail.com",
    "tutanota.com",
    "yahoo.co.in",
    "yahoo.co.uk",
    "yahoo.com",
    "yahoo.in",
    "yandex.com",
    "ymail.com",
    "zoho.com",
    "zohomail.com",
    "zohomail.in",
];

pub fn is_public_provider(domain: &str) -> bool {
    let d = domain.trim().to_ascii_lowercase();
    PUBLIC_PROVIDERS.contains(&d.as_str())
}

/// Trim, lower-case, strip a leading `@` and a trailing dot, then check it is a hostname with
/// at least two labels and an alphabetic top-level label, and not a public provider.
pub fn normalize_domain(raw: &str) -> Result<String, ErrorCode> {
    let d = raw.trim();
    let d = d.strip_prefix('@').unwrap_or(d);
    let d = d.strip_suffix('.').unwrap_or(d).to_ascii_lowercase();
    if !is_hostname(&d) {
        return Err(ErrorCode::InvalidDomain);
    }
    if is_public_provider(&d) {
        return Err(ErrorCode::PublicDomain);
    }
    Ok(d)
}

fn is_hostname(d: &str) -> bool {
    if d.is_empty() || d.len() > 253 {
        return false;
    }
    let labels: Vec<&str> = d.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    let label_ok = |l: &&str| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    let tld = labels[labels.len() - 1];
    labels.iter().all(label_ok) && tld.len() >= 2 && tld.bytes().all(|b| b.is_ascii_lowercase())
}
