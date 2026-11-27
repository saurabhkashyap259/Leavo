//! T161 — holiday CSV (contracts/csv-formats.md §6): header by name, optional kind/location,
//! row-level errors with 1-based row numbers counting the header.
use chrono::NaiveDate;
use leavo_core::calendar::HolidayKind;
use leavo_importers::holidays::{parse_holidays, HolidayRow, RowError};

fn d(s: &str) -> NaiveDate {
    s.parse().unwrap()
}

#[test]
fn reads_columns_by_name_with_defaults() {
    let out = parse_holidays(
        b"name,date,location,kind\r\nRepublic Day,2027-01-26,,\r\nGandhi Jayanti, 2027-10-02 ,Delhi,optional\r\n\r\n",
    );
    assert_eq!(out.errors, []);
    assert_eq!(
        out.rows,
        [
            HolidayRow {
                row: 2,
                date: d("2027-01-26"),
                name: "Republic Day".into(),
                kind: HolidayKind::Mandatory,
                location: None,
            },
            HolidayRow {
                row: 3,
                date: d("2027-10-02"),
                name: "Gandhi Jayanti".into(),
                kind: HolidayKind::Optional,
                location: Some("Delhi".into()),
            },
        ]
    );
}

#[test]
fn reports_every_bad_row() {
    let out = parse_holidays(
        "date,name,kind\n2027-02-30,Nope,\n2027-03-01,,mandatory\n2027-03-02,Holi,sometimes\n2027-01-26,Republic Day,\n2027-01-26,republic day,\n"
            .as_bytes(),
    );
    let codes: Vec<(usize, &str)> = out.errors.iter().map(|e| (e.row, e.code)).collect();
    assert_eq!(
        codes,
        [
            (2, "INVALID_DATE"),
            (3, "MISSING_VALUE"),
            (4, "INVALID_KIND"),
            (6, "DUPLICATE_ROW"),
        ]
    );
    assert_eq!(out.rows.len(), 1);
}

#[test]
fn a_missing_required_column_is_one_error() {
    let out = parse_holidays(b"date,kind\n2027-01-26,mandatory\n");
    assert_eq!(
        out.errors,
        [RowError {
            row: 1,
            code: "MISSING_COLUMN",
            message: "The file needs a \"name\" column.".into(),
        }]
    );
    assert!(out.rows.is_empty());
    assert_eq!(parse_holidays(b"").errors[0].code, "MISSING_COLUMN");
}
