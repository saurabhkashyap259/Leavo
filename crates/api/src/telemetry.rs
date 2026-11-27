//! Structured JSON logs with redaction (FR-065, constitution VII).
//!
//! Field values named like secrets (`code`, `token`, `reason`, …) are replaced with
//! `[redacted]`; `email` keeps only its domain (`***@acme.com`). Span fields such as
//! `request_id` and `org_id` are flattened onto every line.
use axum::http::Request;
use serde_json::{Map, Value};
use std::io::Write;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Span, Subscriber};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

const REDACT: &[&str] = &[
    "code",
    "token",
    "reason",
    "secret",
    "password",
    "setup_token",
    "attachment",
    "cookie",
];

fn redact(name: &str, value: Value) -> Value {
    if REDACT.contains(&name) {
        return Value::String("[redacted]".into());
    }
    if name == "email" || name.ends_with("_email") {
        if let Value::String(s) = &value {
            return Value::String(match s.split_once('@') {
                Some((_, domain)) => format!("***@{domain}"),
                None => "***".into(),
            });
        }
    }
    value
}

#[derive(Default)]
struct Fields {
    map: Map<String, Value>,
    message: Option<String>,
}

impl Fields {
    fn put(&mut self, field: &Field, value: Value) {
        if field.name() == "message" {
            self.message = Some(match value {
                Value::String(s) => s,
                other => other.to_string(),
            });
        } else {
            self.map
                .insert(field.name().to_string(), redact(field.name(), value));
        }
    }
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.put(field, Value::String(value.to_string()));
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.put(field, Value::from(value));
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.put(field, Value::from(value));
    }
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.put(field, Value::from(value));
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.put(field, Value::from(value));
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.put(field, Value::String(format!("{value:?}")));
    }
}

struct SpanFields(Map<String, Value>);

pub struct RedactingJsonLayer<W> {
    make_writer: W,
}

impl<W> RedactingJsonLayer<W> {
    pub fn new(make_writer: W) -> Self {
        Self { make_writer }
    }
}

impl<S, W> Layer<S> for RedactingJsonLayer<W>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    W: for<'a> MakeWriter<'a> + 'static,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut f = Fields::default();
        attrs.record(&mut f);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(SpanFields(f.map));
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            let mut f = Fields::default();
            values.record(&mut f);
            let mut ext = span.extensions_mut();
            if let Some(existing) = ext.get_mut::<SpanFields>() {
                existing.0.extend(f.map);
            }
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut f = Fields::default();
        event.record(&mut f);
        let meta = event.metadata();
        let mut line = Map::new();
        line.insert(
            "timestamp".into(),
            Value::String(chrono::Utc::now().to_rfc3339()),
        );
        line.insert("level".into(), Value::String(meta.level().to_string()));
        line.insert("target".into(), Value::String(meta.target().to_string()));
        if let Some(scope) = ctx.event_scope(event) {
            for span in scope.from_root() {
                if let Some(sf) = span.extensions().get::<SpanFields>() {
                    for (k, v) in &sf.0 {
                        line.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        line.insert(
            "message".into(),
            Value::String(f.message.unwrap_or_default()),
        );
        if !f.map.is_empty() {
            line.insert("fields".into(), Value::Object(f.map));
        }
        let mut w = self.make_writer.make_writer();
        let _ = serde_json::to_writer(&mut w, &Value::Object(line));
        let _ = w.write_all(b"\n");
    }
}

/// Install the global subscriber: redacting JSON to stdout, filtered by `level`.
pub fn init(level: &str) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(RedactingJsonLayer::new(std::io::stdout))
        .try_init();
}

/// Span for each HTTP request; carries the request id (org id is recorded once known).
pub fn request_span<B>(req: &Request<B>) -> Span {
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-");
    tracing::info_span!(
        "request",
        method = %req.method(),
        path = %req.uri().path(),
        request_id = %request_id,
        org_id = tracing::field::Empty,
    )
}
