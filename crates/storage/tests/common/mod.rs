//! Behaviour every storage backend must share (T096): sniffing, the 5 MB cap, round trip and
//! deletion. Called by `storage_test.rs` (memory, local disk) and `s3_compat_test.rs`.
#![allow(dead_code)]
use bytes::Bytes;
use leavo_storage::{Storage, StorageError, MAX_ATTACHMENT_BYTES};

pub fn pdf() -> Vec<u8> {
    b"%PDF-1.4\n1 0 obj << >> endobj\ntrailer << >>\n%%EOF\n".to_vec()
}
pub fn png() -> Vec<u8> {
    let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    v.extend_from_slice(&[0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0, 1, 0, 0, 0, 1]);
    v
}
pub fn jpeg() -> Vec<u8> {
    vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F', 0, 1]
}
pub fn webp() -> Vec<u8> {
    let mut v = b"RIFF".to_vec();
    v.extend_from_slice(&[0x24, 0, 0, 0]);
    v.extend_from_slice(b"WEBPVP8 ");
    v.extend_from_slice(&[0; 16]);
    v
}
pub fn heic() -> Vec<u8> {
    let mut v = vec![0, 0, 0, 0x18];
    v.extend_from_slice(b"ftypheic");
    v.extend_from_slice(&[0, 0, 0, 0]);
    v.extend_from_slice(b"mif1heic");
    v.extend_from_slice(&[0; 8]);
    v
}
/// A Windows executable, whatever its file name says.
pub fn exe() -> Vec<u8> {
    let mut v = b"MZ".to_vec();
    v.extend_from_slice(&[0x90, 0, 3, 0, 0, 0, 4, 0, 0, 0, 0xFF, 0xFF]);
    v
}

pub fn sized_pdf(len: usize) -> Vec<u8> {
    let mut v = pdf();
    v.resize(len, b' ');
    v
}

pub async fn check_backend(store: &Storage, prefix: &str) {
    for (name, bytes, expected) in [
        ("pdf", pdf(), "application/pdf"),
        ("png", png(), "image/png"),
        ("jpeg", jpeg(), "image/jpeg"),
        ("webp", webp(), "image/webp"),
        ("heic", heic(), "image/heif"),
    ] {
        let key = format!("{prefix}/{name}");
        let ct = store
            .put_checked(&key, Bytes::from(bytes.clone()))
            .await
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(ct, expected, "{name}");
        assert_eq!(store.get(&key).await.unwrap(), Bytes::from(bytes), "{name}");
    }

    let err = store
        .put_checked(&format!("{prefix}/evil.pdf"), Bytes::from(exe()))
        .await
        .unwrap_err();
    assert!(matches!(err, StorageError::TypeNotAllowed), "{err:?}");
    assert!(matches!(
        store.get(&format!("{prefix}/evil.pdf")).await.unwrap_err(),
        StorageError::NotFound
    ));

    let exact = format!("{prefix}/exact");
    store
        .put_checked(&exact, Bytes::from(sized_pdf(MAX_ATTACHMENT_BYTES)))
        .await
        .expect("exactly 5 MB is allowed");
    let err = store
        .put_checked(
            &format!("{prefix}/big"),
            Bytes::from(sized_pdf(MAX_ATTACHMENT_BYTES + 1)),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, StorageError::TooLarge), "{err:?}");

    store.delete(&exact).await.unwrap();
    assert!(matches!(
        store.get(&exact).await.unwrap_err(),
        StorageError::NotFound
    ));
}
