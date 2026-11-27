//! T069 — whitelisted domain normalisation and the public-provider blocklist (FR-009).
use leavo_core::domains::{is_public_provider, normalize_domain};
use leavo_core::error::ErrorCode;

#[test]
fn normalises_case_whitespace_and_leading_at() {
    assert_eq!(normalize_domain("acme.com").unwrap(), "acme.com");
    assert_eq!(normalize_domain("  Acme.COM ").unwrap(), "acme.com");
    assert_eq!(normalize_domain("@acme.co.in").unwrap(), "acme.co.in");
    assert_eq!(
        normalize_domain(" @Mail.Acme-Corp.io").unwrap(),
        "mail.acme-corp.io"
    );
    assert_eq!(
        normalize_domain("acme.com.").unwrap(),
        "acme.com",
        "trailing dot"
    );
}

#[test]
fn rejects_things_that_are_not_hostnames() {
    for bad in [
        "",
        " ",
        "@",
        "acme",
        "acme.",
        ".acme.com",
        "acme..com",
        "-acme.com",
        "acme-.com",
        "ac me.com",
        "acme.c",
        "acme.123",
        "asha@acme.com",
        "acme.com/path",
        "http://acme.com",
        "acme_corp.com",
    ] {
        assert_eq!(
            normalize_domain(bad),
            Err(ErrorCode::InvalidDomain),
            "{bad:?}"
        );
    }
    let long_label = format!("{}.com", "a".repeat(64));
    assert_eq!(normalize_domain(&long_label), Err(ErrorCode::InvalidDomain));
    let long_name = format!("{}.com", ["abcdefghij"; 25].join("."));
    assert!(long_name.len() > 253);
    assert_eq!(normalize_domain(&long_name), Err(ErrorCode::InvalidDomain));
}

#[test]
fn refuses_public_email_providers() {
    for public in [
        "gmail.com",
        "GMail.com",
        "googlemail.com",
        "yahoo.com",
        "yahoo.co.in",
        "outlook.com",
        "hotmail.com",
        "live.com",
        "icloud.com",
        "proton.me",
        "protonmail.com",
        "rediffmail.com",
        "aol.com",
        "zoho.com",
    ] {
        assert_eq!(
            normalize_domain(public),
            Err(ErrorCode::PublicDomain),
            "{public}"
        );
    }
    assert!(is_public_provider("gmail.com"));
    assert!(!is_public_provider("acme.com"));
    // A company subdomain of a provider name is not the provider itself.
    assert_eq!(
        normalize_domain("gmail.acme.com").unwrap(),
        "gmail.acme.com"
    );
}
