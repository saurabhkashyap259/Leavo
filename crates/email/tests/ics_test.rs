//! T121 — calendar invite for approved leave (FR-046): an all-day event over the leave dates.
use chrono::{NaiveDate, TimeZone, Utc};
use leavo_email::ics::{leave_event, ICS_CONTENT_TYPE};

#[test]
fn all_day_event_ends_the_day_after_the_last_day() {
    let ics = leave_event(
        "3f1c2a",
        "Asha Rao — Casual Leave",
        NaiveDate::from_ymd_opt(2026, 11, 30).unwrap(),
        NaiveDate::from_ymd_opt(2026, 12, 2).unwrap(),
        Utc.with_ymd_and_hms(2026, 11, 24, 9, 0, 0).unwrap(),
    );
    assert!(ics.starts_with("BEGIN:VCALENDAR\r\n"), "{ics}");
    assert!(ics.contains("METHOD:PUBLISH"), "{ics}");
    assert!(ics.contains("UID:3f1c2a@leavo"), "{ics}");
    assert!(ics.contains("DTSTART;VALUE=DATE:20261130"), "{ics}");
    assert!(ics.contains("DTEND;VALUE=DATE:20261203"), "{ics}");
    assert!(ics.contains("SUMMARY:Asha Rao — Casual Leave"), "{ics}");
    assert!(ics.contains("TRANSP:TRANSPARENT"), "{ics}");
    assert!(ics.contains("DTSTAMP:20261124T090000Z"), "{ics}");
    assert!(ics.trim_end().ends_with("END:VCALENDAR"));
    assert_eq!(
        ICS_CONTENT_TYPE,
        "text/calendar; charset=utf-8; method=PUBLISH"
    );
}
