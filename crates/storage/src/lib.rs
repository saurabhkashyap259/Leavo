//! Attachment storage: local disk, any S3-compatible bucket, or memory for tests (research R14).
//!
//! Uploads are capped at 5 MB and typed by their content, never by their name. Downloads from
//! S3 use 5-minute pre-signed URLs; local and memory storage return `None` from
//! [`Storage::presigned_get`] and the API serves the file behind its own signed link.
use bytes::{Bytes, BytesMut};
use object_store::aws::AmazonS3Builder;
use object_store::local::LocalFileSystem;
use object_store::memory::InMemory;
use object_store::path::Path;
use object_store::signer::Signer;
use object_store::{ObjectStore, ObjectStoreExt, PutPayload};
use std::sync::Arc;
use std::time::Duration;

pub const MAX_ATTACHMENT_BYTES: usize = 5 * 1024 * 1024;
pub const SIGNED_URL_TTL: Duration = Duration::from_secs(5 * 60);

/// Content types accepted for attachments (FR-030).
pub const ALLOWED_TYPES: &[&str] = &[
    "application/pdf",
    "image/png",
    "image/jpeg",
    "image/webp",
    "image/heif",
];

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("file is larger than 5 MB")]
    TooLarge,
    #[error("file type is not allowed")]
    TypeNotAllowed,
    #[error("object not found")]
    NotFound,
    #[error("storage backend error: {0}")]
    Backend(String),
}

impl From<object_store::Error> for StorageError {
    fn from(e: object_store::Error) -> Self {
        match e {
            object_store::Error::NotFound { .. } => StorageError::NotFound,
            other => StorageError::Backend(other.to_string()),
        }
    }
}

pub type Result<T> = std::result::Result<T, StorageError>;

/// The file's content type from its first bytes; only [`ALLOWED_TYPES`] pass.
pub fn sniff(bytes: &[u8]) -> Result<&'static str> {
    let kind = infer::get(bytes).ok_or(StorageError::TypeNotAllowed)?;
    ALLOWED_TYPES
        .iter()
        .copied()
        .find(|t| *t == kind.mime_type())
        .ok_or(StorageError::TypeNotAllowed)
}

/// Collects an upload chunk by chunk and fails as soon as it passes the cap.
pub struct CappedBuffer {
    buf: BytesMut,
    max: usize,
}

impl CappedBuffer {
    pub fn new(max: usize) -> Self {
        Self {
            buf: BytesMut::new(),
            max,
        }
    }
    pub fn push(&mut self, chunk: &[u8]) -> Result<()> {
        if self.buf.len() + chunk.len() > self.max {
            return Err(StorageError::TooLarge);
        }
        self.buf.extend_from_slice(chunk);
        Ok(())
    }
    pub fn into_bytes(self) -> Bytes {
        self.buf.freeze()
    }
}

#[derive(Debug, Clone)]
pub struct S3Settings {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: String,
}

#[derive(Clone)]
pub struct Storage {
    store: Arc<dyn ObjectStore>,
    signer: Option<Arc<dyn Signer>>,
    kind: &'static str,
}

impl std::fmt::Debug for Storage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Storage")
            .field("store", &self.store.to_string())
            .field("presigned", &self.signer.is_some())
            .finish()
    }
}

impl Storage {
    /// `local`, `s3` or `memory`.
    pub fn kind(&self) -> &'static str {
        self.kind
    }

    pub fn memory() -> Self {
        Self {
            store: Arc::new(InMemory::new()),
            signer: None,
            kind: "memory",
        }
    }

    /// Files under `path`, created if missing.
    pub fn local(path: &str) -> Result<Self> {
        std::fs::create_dir_all(path).map_err(|e| StorageError::Backend(e.to_string()))?;
        let fs = LocalFileSystem::new_with_prefix(path)?.with_automatic_cleanup(true);
        Ok(Self {
            store: Arc::new(fs),
            signer: None,
            kind: "local",
        })
    }

    pub fn s3(s: &S3Settings) -> Result<Self> {
        let s3 = Arc::new(
            AmazonS3Builder::new()
                .with_endpoint(&s.endpoint)
                .with_region(&s.region)
                .with_bucket_name(&s.bucket)
                .with_access_key_id(&s.access_key)
                .with_secret_access_key(&s.secret_key)
                .with_allow_http(s.endpoint.starts_with("http://"))
                .build()?,
        );
        Ok(Self {
            store: s3.clone(),
            signer: Some(s3),
            kind: "s3",
        })
    }

    /// Store an upload after checking its size and content type; returns the content type.
    pub async fn put_checked(&self, key: &str, bytes: Bytes) -> Result<&'static str> {
        if bytes.len() > MAX_ATTACHMENT_BYTES {
            return Err(StorageError::TooLarge);
        }
        let content_type = sniff(&bytes)?;
        self.store
            .put(&Path::from(key), PutPayload::from_bytes(bytes))
            .await?;
        Ok(content_type)
    }

    pub async fn get(&self, key: &str) -> Result<Bytes> {
        Ok(self.store.get(&Path::from(key)).await?.bytes().await?)
    }

    pub async fn delete(&self, key: &str) -> Result<()> {
        match self.store.delete(&Path::from(key)).await {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// A pre-signed GET URL valid for [`SIGNED_URL_TTL`], when the backend supports it.
    pub async fn presigned_get(&self, key: &str) -> Result<Option<String>> {
        let Some(signer) = &self.signer else {
            return Ok(None);
        };
        let url = signer
            .signed_url(http::Method::GET, &Path::from(key), SIGNED_URL_TTL)
            .await?;
        Ok(Some(url.to_string()))
    }
}
