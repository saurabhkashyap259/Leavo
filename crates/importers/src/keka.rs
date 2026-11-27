//! Keka exports (T197, contracts/csv-formats.md §4) mapped to the canonical rows of
//! [`crate::csv`].
//!
//! Keka's export column names vary between report types and versions, so each column is found
//! by any of its usual names (case-insensitive). Keka leave type names map to Leavo codes through
//! a map the admin confirms in the dry run. The balance export has no year; the caller passes the
//! leave year the balances belong to. Only approved history rows are imported.
use crate::csv::{issue, parse_date, parse_days, BalanceRow, HistoryRow, Issue};
use std::collections::{BTreeMap, BTreeSet};

/// Lower-cased Keka leave type name → Leavo leave type code.
pub type KekaMap = BTreeMap<String, String>;

const EMAIL: &[&str] = &[
    "email",
    "work email",
    "email id",
    "official email",
    "official email id",
    "employee email",
];
const NAME: &[&str] = &["employee name", "full name", "name"];
const NUMBER: &[&str] = &["employee number", "employee id", "employee code", "emp id"];
const DOJ: &[&str] = &["date of joining", "joining date", "doj"];
const DEPT: &[&str] = &["department"];
const TYPE: &[&str] = &["leave type", "leave type name", "leave"];
const BALANCE: &[&str] = &[
    "available balance",
    "closing balance",
    "balance",
    "available",
    "leave balance",
];
const FROM: &[&str] = &["from date", "start date", "from", "leave from"];
const TO: &[&str] = &["to date", "end date", "to", "leave to"];
const DAYS: &[&str] = &[
    "no of days",
    "no. of days",
    "number of days",
    "total days",
    "days",
    "leave days",
    "duration",
];
const STATUS: &[&str] = &["status", "leave status", "request status"];
const REASON: &[&str] = &["reason", "note", "notes", "remarks", "comment"];
const APPROVER: &[&str] = &["approved by email", "approver email", "approved by"];

struct Table {
    headers: Vec<String>,
    rows: Vec<(usize, Vec<String>)>,
}

fn read(bytes: &[u8]) -> Table {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(bytes);
    let headers = reader
        .headers()
        .map(|h| {
            h.iter()
                .map(|c| c.trim_start_matches('\u{feff}').trim().to_lowercase())
                .collect()
        })
        .unwrap_or_default();
    let rows = reader
        .records()
        .enumerate()
        .filter_map(|(i, r)| {
            r.ok().map(|r| {
                (
                    i + 2,
                    r.iter().map(|c| c.trim().to_string()).collect::<Vec<_>>(),
                )
            })
        })
        .filter(|(_, r)| r.iter().any(|c| !c.is_empty()))
        .collect();
    Table { headers, rows }
}

impl Table {
    fn col(&self, names: &[&str]) -> Option<usize> {
        names
            .iter()
            .find_map(|n| self.headers.iter().position(|h| h == n))
    }
}

fn val(r: &[String], c: Option<usize>) -> Option<String> {
    c.and_then(|c| r.get(c)).filter(|v| !v.is_empty()).cloned()
}

/// Distinct Keka leave type names in the files, sorted, for the mapping screen.
pub fn keka_leave_types(balances: &[u8], history: Option<&[u8]>) -> Vec<String> {
    let mut names = BTreeSet::new();
    for bytes in std::iter::once(balances).chain(history) {
        let t = read(bytes);
        if let Some(c) = t.col(TYPE) {
            names.extend(t.rows.iter().filter_map(|(_, r)| val(r, Some(c))));
        }
    }
    names.into_iter().collect()
}

#[derive(Debug, Default)]
pub struct KekaOutput {
    pub balances: Vec<BalanceRow>,
    pub history: Vec<HistoryRow>,
    /// Row numbers refer to the Keka files.
    pub issues: Vec<Issue>,
    /// History rows not approved in Keka (rejected, cancelled, pending): not imported.
    pub skipped_history: usize,
}

fn missing(file: &'static str, what: &[&str]) -> Issue {
    issue(
        file,
        1,
        "MISSING_COLUMN",
        format!("The Keka file needs a column for: {}.", what.join(", ")),
    )
}

pub fn map_keka(
    balances: &[u8],
    history: Option<&[u8]>,
    map: &KekaMap,
    leave_year: i32,
) -> KekaOutput {
    let mut out = KekaOutput::default();
    let code_of = |name: &str| map.get(&name.to_lowercase()).map(|c| c.to_uppercase());

    let t = read(balances);
    let (email, ty, bal) = (t.col(EMAIL), t.col(TYPE), t.col(BALANCE));
    let absent: Vec<&str> = [
        ("Email", email),
        ("Leave Type", ty),
        ("Available Balance", bal),
    ]
    .iter()
    .filter(|(_, c)| c.is_none())
    .map(|(n, _)| *n)
    .collect();
    if !absent.is_empty() {
        out.issues.push(missing("balances", &absent));
    } else {
        let (name, number, doj, dept) = (t.col(NAME), t.col(NUMBER), t.col(DOJ), t.col(DEPT));
        for (row, r) in &t.rows {
            let row = *row;
            let (Some(e), Some(keka_type)) = (val(r, email), val(r, ty)) else {
                out.issues.push(issue(
                    "balances",
                    row,
                    "MISSING_VALUE",
                    "Each row needs an email and a leave type.",
                ));
                continue;
            };
            let Some(code) = code_of(&keka_type) else {
                out.issues.push(issue(
                    "balances",
                    row,
                    "UNKNOWN_LEAVE_TYPE",
                    format!("Keka leave type \"{keka_type}\" is not mapped to a Leavo leave type."),
                ));
                continue;
            };
            let Some(days) = val(r, bal).as_deref().and_then(parse_days) else {
                out.issues.push(issue(
                    "balances",
                    row,
                    "INVALID_DAYS",
                    "The balance must be a number of whole or half days.",
                ));
                continue;
            };
            let date_of_joining = match val(r, doj) {
                None => None,
                Some(s) => match parse_date(&s) {
                    Some(d) => Some(d),
                    None => {
                        out.issues.push(issue(
                            "balances",
                            row,
                            "INVALID_DATE",
                            "Date of joining is not a date.",
                        ));
                        continue;
                    }
                },
            };
            out.balances.push(BalanceRow {
                row,
                email: e.to_lowercase(),
                full_name: val(r, name),
                date_of_joining,
                leave_type_code: code,
                leave_year,
                available_days: days,
                employee_code: val(r, number),
                department: val(r, dept),
                approver_group: None,
            });
        }
    }

    let Some(history) = history else {
        return out;
    };
    let t = read(history);
    let cols = [
        ("Email", t.col(EMAIL)),
        ("Leave Type", t.col(TYPE)),
        ("From Date", t.col(FROM)),
        ("To Date", t.col(TO)),
        ("No Of Days", t.col(DAYS)),
    ];
    let absent: Vec<&str> = cols
        .iter()
        .filter(|(_, c)| c.is_none())
        .map(|(n, _)| *n)
        .collect();
    if !absent.is_empty() {
        out.issues.push(missing("history", &absent));
        return out;
    }
    let [email, ty, from, to, days] = cols.map(|(_, c)| c);
    let (status, reason, approver) = (t.col(STATUS), t.col(REASON), t.col(APPROVER));
    for (row, r) in &t.rows {
        let row = *row;
        if let Some(s) = val(r, status) {
            if !s.eq_ignore_ascii_case("approved") {
                out.skipped_history += 1;
                continue;
            }
        }
        let (Some(e), Some(keka_type)) = (val(r, email), val(r, ty)) else {
            out.issues.push(issue(
                "history",
                row,
                "MISSING_VALUE",
                "Each row needs an email and a leave type.",
            ));
            continue;
        };
        let Some(code) = code_of(&keka_type) else {
            out.issues.push(issue(
                "history",
                row,
                "UNKNOWN_LEAVE_TYPE",
                format!("Keka leave type \"{keka_type}\" is not mapped to a Leavo leave type."),
            ));
            continue;
        };
        let (Some(from_date), Some(to_date)) = (
            val(r, from).as_deref().and_then(parse_date),
            val(r, to).as_deref().and_then(parse_date),
        ) else {
            out.issues.push(issue(
                "history",
                row,
                "INVALID_DATE",
                "From and to must be dates.",
            ));
            continue;
        };
        if to_date < from_date {
            out.issues.push(issue(
                "history",
                row,
                "INVALID_DATE",
                "The leave ends before it starts.",
            ));
            continue;
        }
        let Some(n) = val(r, days)
            .as_deref()
            .and_then(parse_days)
            .filter(|d| !d.is_sign_negative() && !d.is_zero())
        else {
            out.issues.push(issue(
                "history",
                row,
                "INVALID_DAYS",
                "Days must be a positive number of whole or half days.",
            ));
            continue;
        };
        out.history.push(HistoryRow {
            row,
            email: e.to_lowercase(),
            leave_type_code: code,
            from_date,
            to_date,
            days: n,
            reason: val(r, reason)
                .map(|s| s.chars().take(500).collect())
                .unwrap_or_else(|| "Imported from Keka".into()),
            approved_by_email: val(r, approver)
                .filter(|a| a.contains('@'))
                .map(|a| a.to_lowercase()),
        });
    }
    out
}
