//! Signed, short-lived verification tickets: proof that an email was verified with a code,
//! carried from `/auth/verify` (purpose `register_org`) to `POST /orgs` (FR-008).
use leavo_core::tokens::{sign_ticket, verify_ticket, TICKET_TTL_SECONDS};

const SECRET: &[u8] = b"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[test]
fn round_trips_the_email_before_expiry() {
    let t = sign_ticket(SECRET, "ceo@newco.test", "register_org", 1_000);
    assert_eq!(
        verify_ticket(SECRET, &t, "register_org", 1_000 + TICKET_TTL_SECONDS - 1).as_deref(),
        Some("ceo@newco.test")
    );
}

#[test]
fn rejects_expired_tampered_wrong_purpose_or_wrong_key() {
    let t = sign_ticket(SECRET, "ceo@newco.test", "register_org", 1_000);
    assert_eq!(
        verify_ticket(SECRET, &t, "register_org", 1_000 + TICKET_TTL_SECONDS),
        None
    );
    assert_eq!(verify_ticket(SECRET, &t, "setup", 1_001), None);
    assert_eq!(
        verify_ticket(b"another-key", &t, "register_org", 1_001),
        None
    );
    let forged = sign_ticket(SECRET, "ceo@other.test", "register_org", 1_000);
    let (email_part, rest) = forged.split_once('.').unwrap();
    let (_, sig) = t.split_once('.').unwrap().1.split_once('.').unwrap();
    let swapped = format!("{email_part}.{}.{sig}", rest.split_once('.').unwrap().0);
    assert_eq!(verify_ticket(SECRET, &swapped, "register_org", 1_001), None);
    assert_eq!(
        verify_ticket(SECRET, "garbage", "register_org", 1_001),
        None
    );
}

mod download_links {
    use leavo_core::tokens::{sign_download, verify_download};
    use uuid::Uuid;

    const SECRET: &[u8] = b"secret-secret-secret-secret-secret-secret-secret-secret-secret-00";

    #[test]
    fn valid_until_expiry_and_bound_to_org_and_attachment() {
        let (org, file) = (Uuid::now_v7(), Uuid::now_v7());
        let sig = sign_download(SECRET, org, file, 1_000);
        assert!(verify_download(SECRET, org, file, 1_000, &sig, 999));
        assert!(!verify_download(SECRET, org, file, 1_000, &sig, 1_000));
        assert!(!verify_download(SECRET, org, file, 1_001, &sig, 999));
        assert!(!verify_download(
            SECRET,
            Uuid::now_v7(),
            file,
            1_000,
            &sig,
            999
        ));
        assert!(!verify_download(
            SECRET,
            org,
            Uuid::now_v7(),
            1_000,
            &sig,
            999
        ));
        assert!(!verify_download(b"other", org, file, 1_000, &sig, 999));
        assert!(!verify_download(
            SECRET,
            org,
            file,
            1_000,
            &format!("0{sig}"),
            999
        ));
        assert!(!verify_download(SECRET, org, file, 1_000, "!!", 999));
    }
}

mod macs {
    use leavo_core::tokens::{mac_b64, mac_matches};

    #[test]
    fn mac_round_trip() {
        let sig = mac_b64(b"k", "msg");
        assert!(mac_matches(b"k", "msg", &sig));
        assert!(!mac_matches(b"k", "msg2", &sig));
        assert!(!mac_matches(b"k2", "msg", &sig));
        assert!(!mac_matches(b"k", "msg", "%%"));
    }
}
