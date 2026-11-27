//! T021 — email senders and templates.
use leavo_email::{
    Attachment, EmailMessage, EmailSender, MemoryEmailSender, SmtpSender, Templates,
};
use serde_json::json;

fn msg(to: &str) -> EmailMessage {
    EmailMessage {
        to: to.into(),
        subject: "Hi".into(),
        html: "<p>Hi</p>".into(),
        text: "Hi".into(),
        attachments: vec![Attachment {
            filename: "leave.ics".into(),
            content_type: "text/calendar".into(),
            bytes: b"BEGIN:VCALENDAR".to_vec(),
        }],
    }
}

#[tokio::test]
async fn memory_sender_captures_messages() {
    let s = MemoryEmailSender::default();
    s.send(&msg("a@acme.test")).await.unwrap();
    s.send(&msg("b@acme.test")).await.unwrap();
    let sent = s.take();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1].to, "b@acme.test");
    assert_eq!(sent[0].attachments[0].filename, "leave.ics");
    assert!(s.take().is_empty(), "take drains");
}

#[tokio::test]
async fn memory_sender_can_simulate_failures() {
    let s = MemoryEmailSender::default();
    s.fail_next("connection refused");
    assert!(s
        .send(&msg("a@acme.test"))
        .await
        .unwrap_err()
        .to_string()
        .contains("connection refused"));
    assert!(s.send(&msg("a@acme.test")).await.is_ok());
}

#[test]
fn every_template_renders_subject_html_and_text() {
    let t = Templates::new();
    let names = t.names();
    assert!(names.contains(&"test_email".to_string()));
    for name in names {
        let r = t
            .render(&name, &Templates::sample_context(&name))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!r.subject.trim().is_empty(), "{name}: subject");
        assert!(
            !r.subject.contains('\n'),
            "{name}: subject must be one line"
        );
        assert!(r.html.contains("<html"), "{name}: html layout");
        assert!(!r.text.trim().is_empty(), "{name}: text part");
        assert!(
            !r.text.contains("<p>"),
            "{name}: text part must not contain HTML"
        );
    }
}

#[test]
fn html_is_escaped_but_text_is_not() {
    let t = Templates::new();
    let r = t
        .render("test_email", &json!({"org_name": "<b>Acme & Co</b>"}))
        .unwrap();
    assert!(r.html.contains("&lt;b&gt;Acme &amp; Co&lt;"), "{}", r.html);
    assert!(!r.html.contains("<b>Acme"));
    assert!(r.text.contains("<b>Acme & Co</b>"));
}

#[test]
fn unknown_template_is_an_error() {
    assert!(Templates::new().render("nope", &json!({})).is_err());
}

#[test]
fn smtp_sender_builds_from_supported_urls() {
    for url in [
        "smtp://localhost:1025",
        "smtps://user:pass@email-smtp.ap-south-1.amazonaws.com:465",
        "smtp://user:pass@smtp.example.com:587?tls=required",
    ] {
        SmtpSender::from_url(url, "Leavo <leave@acme.test>")
            .unwrap_or_else(|e| panic!("{url}: {e}"));
    }
}

#[test]
fn smtp_sender_rejects_bad_config() {
    assert!(SmtpSender::from_url("http://localhost", "Leavo <leave@acme.test>").is_err());
    assert!(SmtpSender::from_url("smtp://localhost:1025", "not an address").is_err());
}

#[test]
fn every_template_file_on_disk_is_registered() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("templates");
    let mut on_disk: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|n| n.strip_suffix(".txt.j2").map(str::to_string))
        .filter(|n| !n.starts_with("layout"))
        .collect();
    on_disk.sort();
    let mut registered = Templates::new().names();
    registered.sort();
    assert_eq!(
        registered, on_disk,
        "add new templates to SOURCES in src/templates.rs"
    );
}
