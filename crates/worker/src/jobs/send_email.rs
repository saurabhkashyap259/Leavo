//! `send_email`: render an outbox email and send it (retried by the runner on failure).
use crate::runner::{JobHandler, JobResult};
use async_trait::async_trait;
use leavo_db::jobs::Job;
use leavo_db::outbox::{OutgoingEmail, SEND_EMAIL};
use leavo_email::{Attachment, EmailMessage, EmailSender, Templates};
use std::sync::Arc;

pub struct SendEmail {
    templates: Arc<Templates>,
    sender: Arc<dyn EmailSender>,
}

impl SendEmail {
    pub const KIND: &'static str = SEND_EMAIL;

    pub fn new(templates: Arc<Templates>, sender: Arc<dyn EmailSender>) -> Self {
        Self { templates, sender }
    }
}

#[async_trait]
impl JobHandler for SendEmail {
    async fn handle(&self, job: &Job) -> JobResult {
        let email: OutgoingEmail = serde_json::from_value(job.payload.clone())?;
        let rendered = self.templates.render(&email.template, &email.context)?;
        let msg = EmailMessage {
            to: email.to,
            subject: rendered.subject,
            html: rendered.html,
            text: rendered.text,
            attachments: email
                .attachments
                .into_iter()
                .map(|a| Attachment {
                    filename: a.filename,
                    content_type: a.content_type,
                    bytes: a.content.into_bytes(),
                })
                .collect(),
        };
        self.sender.send(&msg).await?;
        Ok(())
    }
}
