//! Calendar invite for approved leave (FR-046): one all-day VEVENT (RFC 5545).
//! Written by hand: a single event needs escaping, CRLF line endings and line folding only.
use chrono::{DateTime, Days, NaiveDate, Utc};

pub const ICS_CONTENT_TYPE: &str = "text/calendar; charset=utf-8; method=PUBLISH";
pub const ICS_FILENAME: &str = "leave.ics";

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// Fold a content line at 75 octets (continuations start with a space), on char boundaries.
fn fold(line: &str, out: &mut String) {
    let mut width = 0;
    for c in line.chars() {
        let len = c.len_utf8();
        if width + len > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += len;
    }
    out.push_str("\r\n");
}

/// An all-day event from `from` to `to` inclusive (DTEND is the day after, as RFC 5545 wants).
pub fn leave_event(
    uid: &str,
    summary: &str,
    from: NaiveDate,
    to: NaiveDate,
    now: DateTime<Utc>,
) -> String {
    let end = to.checked_add_days(Days::new(1)).unwrap_or(to);
    let lines = [
        "BEGIN:VCALENDAR".to_string(),
        "VERSION:2.0".into(),
        "PRODID:-//Leavo//Leave//EN".into(),
        "CALSCALE:GREGORIAN".into(),
        "METHOD:PUBLISH".into(),
        "BEGIN:VEVENT".into(),
        format!("UID:{}@leavo", escape(uid)),
        format!("DTSTAMP:{}", now.format("%Y%m%dT%H%M%SZ")),
        format!("DTSTART;VALUE=DATE:{}", from.format("%Y%m%d")),
        format!("DTEND;VALUE=DATE:{}", end.format("%Y%m%d")),
        format!("SUMMARY:{}", escape(summary)),
        "TRANSP:TRANSPARENT".into(),
        "END:VEVENT".into(),
        "END:VCALENDAR".into(),
    ];
    let mut out = String::new();
    for l in lines {
        fold(&l, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_and_folds() {
        assert_eq!(escape("a,b;c\\d\ne"), "a\\,b\\;c\\\\d\\ne");
        let mut out = String::new();
        fold(&format!("SUMMARY:{}", "é".repeat(60)), &mut out);
        for line in out.split("\r\n").filter(|l| !l.is_empty()) {
            assert!(line.len() <= 75, "{} octets", line.len());
        }
        assert_eq!(
            out.replace("\r\n ", "").trim_end(),
            format!("SUMMARY:{}", "é".repeat(60))
        );
    }
}
