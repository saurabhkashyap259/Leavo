//! Email sending (EmailSender trait), templates and calendar attachments.
pub mod ics;
mod sender;
mod templates;

pub use sender::{
    Attachment, EmailError, EmailMessage, EmailSender, MemoryEmailSender, SmtpSender,
};
pub use templates::{Rendered, TemplateError, Templates};
