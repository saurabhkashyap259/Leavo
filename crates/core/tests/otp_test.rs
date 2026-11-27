//! T054 — sign-in code primitives (FR-001, FR-002, research R4).
use leavo_core::otp::{domain_of, generate_code, hash_code, normalize_email, verify_code};
use proptest::prelude::*;

const SECRET: &[u8] = b"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[test]
fn codes_are_six_ascii_digits() {
    for _ in 0..2000 {
        let c = generate_code();
        assert_eq!(c.len(), 6, "{c}");
        assert!(c.bytes().all(|b| b.is_ascii_digit()), "{c}");
    }
}

#[test]
fn codes_include_leading_zeros_sometimes() {
    // With 20k draws, P(no code < 100000) ≈ 0.9^20000 ≈ 0.
    assert!((0..20_000)
        .map(|_| generate_code())
        .any(|c| c.starts_with('0')));
}

#[test]
fn hash_is_keyed_and_bound_to_email() {
    let h = hash_code(SECRET, "asha@acme.com", "123456");
    assert_eq!(
        h,
        hash_code(SECRET, "  ASHA@Acme.com ", "123456"),
        "email is normalised"
    );
    assert_ne!(h, hash_code(SECRET, "ravi@acme.com", "123456"));
    assert_ne!(h, hash_code(SECRET, "asha@acme.com", "123457"));
    assert_ne!(
        h,
        hash_code(
            b"another-secret-another-secret-another-secret-another-secret-0000",
            "asha@acme.com",
            "123456"
        )
    );
}

#[test]
fn verify_accepts_only_the_right_code() {
    let stored = hash_code(SECRET, "asha@acme.com", "042917");
    assert!(verify_code(SECRET, "asha@acme.com", "042917", &stored));
    assert!(!verify_code(SECRET, "asha@acme.com", "042918", &stored));
    assert!(!verify_code(SECRET, "asha@acme.com", "42917", &stored));
    assert!(
        !verify_code(SECRET, "asha@acme.com", "042917", &stored[..31]),
        "wrong length never matches"
    );
}

#[test]
fn surrounding_whitespace_in_code_is_ignored() {
    let stored = hash_code(SECRET, "asha@acme.com", "042917");
    assert!(verify_code(SECRET, "asha@acme.com", " 042917 ", &stored));
}

#[test]
fn emails_are_normalised_and_validated() {
    assert_eq!(
        normalize_email("  Asha.Rao@Acme.COM ").as_deref(),
        Some("asha.rao@acme.com")
    );
    for bad in [
        "",
        "asha",
        "@acme.com",
        "asha@",
        "asha@acme",
        "a b@acme.com",
        "a@@acme.com",
        "asha@.com",
    ] {
        assert_eq!(normalize_email(bad), None, "{bad:?}");
    }
    assert_eq!(domain_of("asha@acme.com"), Some("acme.com"));
}

proptest! {
    #[test]
    fn verify_round_trips(local in "[a-z]{1,10}", code in "[0-9]{6}") {
        let email = format!("{local}@acme.com");
        let stored = hash_code(SECRET, &email, &code);
        prop_assert!(verify_code(SECRET, &email, &code, &stored));
    }
}
