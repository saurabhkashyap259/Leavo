//! T024 — logs never contain codes, tokens, leave reasons or email local parts (FR-065).
use leavo_api::telemetry::RedactingJsonLayer;
use std::io::Write;
use std::sync::{Arc, Mutex};
use tracing_subscriber::layer::SubscriberExt;

#[derive(Clone, Default)]
struct Buf(Arc<Mutex<Vec<u8>>>);

impl Write for Buf {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn capture(f: impl FnOnce()) -> String {
    let buf = Buf::default();
    let out = buf.clone();
    let subscriber =
        tracing_subscriber::registry().with(RedactingJsonLayer::new(move || out.clone()));
    tracing::subscriber::with_default(subscriber, f);
    let bytes = buf.0.lock().unwrap().clone();
    String::from_utf8(bytes).unwrap()
}

#[test]
fn sensitive_fields_are_redacted() {
    let out = capture(|| {
        let span = tracing::info_span!("request", request_id = "req-1", org_id = "org-9");
        let _g = span.enter();
        tracing::info!(
            code = "123456",
            token = "tok_abcdef",
            reason = "family trip to Goa",
            email = "asha@acme.com",
            user_id = "user-7",
            "verify attempt"
        );
    });
    for secret in ["123456", "tok_abcdef", "family trip", "asha@"] {
        assert!(!out.contains(secret), "leaked {secret}: {out}");
    }
    assert!(out.contains("***@acme.com"), "{out}");
    assert!(out.contains("user-7"));
    assert!(out.contains("verify attempt"));
}

#[test]
fn each_line_is_json_with_span_context() {
    let out = capture(|| {
        let span = tracing::info_span!("request", request_id = "req-1", org_id = "org-9");
        let _g = span.enter();
        tracing::warn!(attempts = 3, "job failed");
    });
    let line: serde_json::Value = serde_json::from_str(out.lines().next().unwrap()).unwrap();
    assert_eq!(line["level"], "WARN");
    assert_eq!(line["message"], "job failed");
    assert_eq!(line["fields"]["attempts"], 3);
    assert_eq!(line["request_id"], "req-1");
    assert_eq!(line["org_id"], "org-9");
    assert!(line["timestamp"].is_string());
}
