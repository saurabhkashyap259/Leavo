//! T096 (local half) — attachment storage on local disk and in memory.
mod common;

use bytes::Bytes;
use common::*;
use leavo_storage::{sniff, CappedBuffer, Storage, StorageError, MAX_ATTACHMENT_BYTES};

#[tokio::test]
async fn memory_backend() {
    let store = Storage::memory();
    check_backend(&store, "org/user").await;
    // No presigned URLs: the API signs its own short-lived download link.
    assert_eq!(store.presigned_get("org/user/pdf").await.unwrap(), None);
}

#[tokio::test]
async fn local_backend() {
    let dir = std::env::temp_dir().join(format!("leavo-storage-{}", std::process::id()));
    let store = Storage::local(dir.to_str().unwrap()).unwrap();
    check_backend(&store, "org/user").await;
    assert_eq!(store.presigned_get("org/user/pdf").await.unwrap(), None);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn sniffing_ignores_the_file_name_and_rejects_unknown_content() {
    assert_eq!(sniff(&pdf()).unwrap(), "application/pdf");
    assert!(matches!(sniff(&exe()), Err(StorageError::TypeNotAllowed)));
    assert!(matches!(sniff(b"hello"), Err(StorageError::TypeNotAllowed)));
    assert!(matches!(sniff(&[]), Err(StorageError::TypeNotAllowed)));
}

#[test]
fn capped_buffer_stops_at_one_byte_over() {
    let mut buf = CappedBuffer::new(MAX_ATTACHMENT_BYTES);
    buf.push(&Bytes::from(vec![0u8; MAX_ATTACHMENT_BYTES - 1]))
        .unwrap();
    buf.push(&Bytes::from_static(b"x")).unwrap();
    assert!(matches!(
        buf.push(&Bytes::from_static(b"y")),
        Err(StorageError::TooLarge)
    ));
    assert_eq!(buf.into_bytes().len(), MAX_ATTACHMENT_BYTES);
}
