//! Email transport behind a trait: SMTP for real installs, in-memory for tests (FR-053).
use async_trait::async_trait;
use lettre::message::header::ContentType;
use lettre::message::{Mailbox, MultiPart, SinglePart};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub filename: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailMessage {
    pub to: String,
    pub subject: String,
    pub html: String,
    pub text: String,
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, thiserror::Error)]
pub enum EmailError {
    #[error("invalid email configuration: {0}")]
    Config(String),
    #[error("invalid address {0}")]
    Address(String),
    #[error("could not build message: {0}")]
    Build(String),
    #[error("send failed: {0}")]
    Send(String),
}

#[async_trait]
pub trait EmailSender: Send + Sync {
    async fn send(&self, msg: &EmailMessage) -> Result<(), EmailError>;
}

/// Sends through any SMTP server (SES, Postmark, Resend, Google Workspace relay, Mailpit…).
pub struct SmtpSender {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl SmtpSender {
    /// `url`: `smtp://host:port`, `smtps://user:pass@host:465`, `smtp://…:587?tls=required`.
    pub fn from_url(url: &str, from: &str) -> Result<Self, EmailError> {
        let transport = AsyncSmtpTransport::<Tokio1Executor>::from_url(url)
            .map_err(|e| EmailError::Config(e.to_string()))?
            .build();
        let from = from
            .parse()
            .map_err(|_| EmailError::Address(from.to_string()))?;
        Ok(Self { transport, from })
    }

    /// Check the server accepts a connection (used by the Instance page and setup wizard).
    pub async fn test_connection(&self) -> Result<(), EmailError> {
        match self.transport.test_connection().await {
            Ok(true) => Ok(()),
            Ok(false) => Err(EmailError::Send(
                "SMTP server did not accept the connection".into(),
            )),
            Err(e) => Err(EmailError::Send(e.to_string())),
        }
    }
}

#[async_trait]
impl EmailSender for SmtpSender {
    async fn send(&self, msg: &EmailMessage) -> Result<(), EmailError> {
        let to: Mailbox = msg
            .to
            .parse()
            .map_err(|_| EmailError::Address(msg.to.clone()))?;
        let alternative = MultiPart::alternative()
            .singlepart(SinglePart::plain(msg.text.clone()))
            .singlepart(SinglePart::html(msg.html.clone()));
        let body = if msg.attachments.is_empty() {
            alternative
        } else {
            let mut mixed = MultiPart::mixed().multipart(alternative);
            for a in &msg.attachments {
                let ct = ContentType::parse(&a.content_type)
                    .map_err(|e| EmailError::Build(e.to_string()))?;
                mixed = mixed.singlepart(
                    lettre::message::Attachment::new(a.filename.clone()).body(a.bytes.clone(), ct),
                );
            }
            mixed
        };
        let email = Message::builder()
            .from(self.from.clone())
            .to(to)
            .subject(msg.subject.clone())
            .multipart(body)
            .map_err(|e| EmailError::Build(e.to_string()))?;
        self.transport
            .send(email)
            .await
            .map_err(|e| EmailError::Send(e.to_string()))?;
        Ok(())
    }
}

/// Captures messages instead of sending them. Used by tests and local demos.
#[derive(Debug, Default)]
pub struct MemoryEmailSender {
    sent: Mutex<Vec<EmailMessage>>,
    fail_next: Mutex<Option<String>>,
}

impl MemoryEmailSender {
    /// All captured messages, clearing the store.
    pub fn take(&self) -> Vec<EmailMessage> {
        std::mem::take(&mut *self.sent.lock().expect("sent lock"))
    }

    /// A copy of captured messages without clearing.
    pub fn sent(&self) -> Vec<EmailMessage> {
        self.sent.lock().expect("sent lock").clone()
    }

    /// Make the next send fail with `error`.
    pub fn fail_next(&self, error: &str) {
        *self.fail_next.lock().expect("fail lock") = Some(error.to_string());
    }
}

#[async_trait]
impl EmailSender for MemoryEmailSender {
    async fn send(&self, msg: &EmailMessage) -> Result<(), EmailError> {
        if let Some(err) = self.fail_next.lock().expect("fail lock").take() {
            return Err(EmailError::Send(err));
        }
        self.sent.lock().expect("sent lock").push(msg.clone());
        Ok(())
    }
}
