//! Canonical import files (T196, contracts/csv-formats.md §2, §3, §5): opening balances and
//! informational leave history, and the dry-run plan against the organization.
//!
//! Parsing catches what the file alone shows (`MISSING_COLUMN`, `INVALID_DATE`, `INVALID_DAYS`).
//! [`plan`] adds what needs the organization: domains, leave types, existing people, earlier
//! imports and booked days. Row numbers count the header line. Rows with an error are left out
//! of the plan; any error blocks the apply.
use chrono::NaiveDate;
use leavo_core::calendar::leave_year_of;
use rust_decimal::Decimal;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::str::FromStr;
use uuid::Uuid;

/// An error or warning on one row of one file (`balances` or `history`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub file: &'static str,
    pub row: usize,
    pub code: &'static str,
    pub message: String,
}

pub(crate) fn issue(
    file: &'static str,
    row: usize,
    code: &'static str,
    message: impl Into<String>,
) -> Issue {
    Issue {
        file,
        row,
        code,
        message: message.into(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalanceRow {
    pub row: usize,
    /// Lower-cased.
    pub email: String,
    pub full_name: Option<String>,
    pub date_of_joining: Option<NaiveDate>,
    /// Upper-cased.
    pub leave_type_code: String,
    pub leave_year: i32,
    /// The balance to set; may be negative.
    pub available_days: Decimal,
    pub employee_code: Option<String>,
    pub department: Option<String>,
    pub approver_group: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRow {
    pub row: usize,
    pub email: String,
    pub leave_type_code: String,
    pub from_date: NaiveDate,
    pub to_date: NaiveDate,
    /// Trusted as given (the source's count).
    pub days: Decimal,
    pub reason: String,
    pub approved_by_email: Option<String>,
}

/// Dates as `YYYY-MM-DD`, or the day-first forms Keka exports.
pub fn parse_date(s: &str) -> Option<NaiveDate> {
    [
        "%Y-%m-%d", "%d-%b-%Y", "%d %b %Y", "%d/%m/%Y", "%d-%m-%Y", "%d-%B-%Y", "%d %B %Y",
    ]
    .iter()
    .find_map(|f| NaiveDate::parse_from_str(s.trim(), f).ok())
}

/// A multiple of 0.5 within ±400.
pub fn parse_days(s: &str) -> Option<Decimal> {
    let d = Decimal::from_str(s.trim()).ok()?;
    ((d * Decimal::TWO).fract().is_zero() && d.abs() <= Decimal::from(400)).then(|| d.normalize())
}

struct Table {
    headers: Vec<String>,
    rows: Vec<(usize, csv::StringRecord)>,
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
                .map(|c| c.trim().trim_start_matches('\u{feff}').to_ascii_lowercase())
                .collect()
        })
        .unwrap_or_default();
    let rows = reader
        .records()
        .enumerate()
        .filter_map(|(i, r)| r.ok().map(|r| (i + 2, r)))
        .filter(|(_, r)| r.iter().any(|c| !c.trim().is_empty()))
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

fn get(r: &csv::StringRecord, c: Option<usize>) -> Option<String> {
    c.and_then(|c| r.get(c))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn missing(file: &'static str, columns: &[&str]) -> Issue {
    issue(
        file,
        1,
        "MISSING_COLUMN",
        format!("The file needs these columns: {}.", columns.join(", ")),
    )
}

/// Opening balances (§2).
pub fn parse_balances(bytes: &[u8]) -> (Vec<BalanceRow>, Vec<Issue>) {
    const F: &str = "balances";
    let t = read(bytes);
    let required = ["email", "leave_type_code", "leave_year", "available_days"];
    let cols: Vec<Option<usize>> = required.iter().map(|c| t.col(&[c])).collect();
    let absent: Vec<&str> = required
        .iter()
        .zip(&cols)
        .filter(|(_, c)| c.is_none())
        .map(|(n, _)| *n)
        .collect();
    if !absent.is_empty() {
        return (vec![], vec![missing(F, &absent)]);
    }
    let (email, code, year, days) = (cols[0], cols[1], cols[2], cols[3]);
    let (name, doj, emp, dept, group) = (
        t.col(&["full_name"]),
        t.col(&["date_of_joining"]),
        t.col(&["employee_code"]),
        t.col(&["department"]),
        t.col(&["approver_group"]),
    );
    let mut rows = Vec::new();
    let mut issues = Vec::new();
    for (row, r) in &t.rows {
        let row = *row;
        let (Some(email), Some(code)) = (get(r, email), get(r, code)) else {
            issues.push(issue(
                F,
                row,
                "MISSING_VALUE",
                "Each row needs an email and a leave type code.",
            ));
            continue;
        };
        let Some(leave_year) = get(r, year)
            .and_then(|y| y.parse::<i32>().ok())
            .filter(|y| (2000..=2200).contains(y))
        else {
            issues.push(issue(
                F,
                row,
                "INVALID_DATE",
                "leave_year must be a year such as 2026.",
            ));
            continue;
        };
        let Some(available_days) = get(r, days).as_deref().and_then(parse_days) else {
            issues.push(issue(
                F,
                row,
                "INVALID_DAYS",
                "available_days must be a number of whole or half days.",
            ));
            continue;
        };
        let date_of_joining = match get(r, doj) {
            None => None,
            Some(s) => match parse_date(&s) {
                Some(d) => Some(d),
                None => {
                    issues.push(issue(
                        F,
                        row,
                        "INVALID_DATE",
                        "date_of_joining is not a date (YYYY-MM-DD).",
                    ));
                    continue;
                }
            },
        };
        rows.push(BalanceRow {
            row,
            email: email.to_lowercase(),
            full_name: get(r, name),
            date_of_joining,
            leave_type_code: code.to_uppercase(),
            leave_year,
            available_days,
            employee_code: get(r, emp),
            department: get(r, dept),
            approver_group: get(r, group),
        });
    }
    (rows, issues)
}

/// Leave history (§3).
pub fn parse_history(bytes: &[u8]) -> (Vec<HistoryRow>, Vec<Issue>) {
    const F: &str = "history";
    let t = read(bytes);
    let required = ["email", "leave_type_code", "from_date", "to_date", "days"];
    let cols: Vec<Option<usize>> = required.iter().map(|c| t.col(&[c])).collect();
    let absent: Vec<&str> = required
        .iter()
        .zip(&cols)
        .filter(|(_, c)| c.is_none())
        .map(|(n, _)| *n)
        .collect();
    if !absent.is_empty() {
        return (vec![], vec![missing(F, &absent)]);
    }
    let (reason, approver) = (t.col(&["reason"]), t.col(&["approved_by_email"]));
    let mut rows = Vec::new();
    let mut issues = Vec::new();
    for (row, r) in &t.rows {
        let row = *row;
        let (Some(email), Some(code)) = (get(r, cols[0]), get(r, cols[1])) else {
            issues.push(issue(
                F,
                row,
                "MISSING_VALUE",
                "Each row needs an email and a leave type code.",
            ));
            continue;
        };
        let from = get(r, cols[2]).as_deref().and_then(parse_date);
        let to = get(r, cols[3]).as_deref().and_then(parse_date);
        let (Some(from_date), Some(to_date)) = (from, to) else {
            issues.push(issue(
                F,
                row,
                "INVALID_DATE",
                "from_date and to_date must be dates (YYYY-MM-DD).",
            ));
            continue;
        };
        if to_date < from_date {
            issues.push(issue(
                F,
                row,
                "INVALID_DATE",
                "to_date is before from_date.",
            ));
            continue;
        }
        let Some(days) = get(r, cols[4])
            .as_deref()
            .and_then(parse_days)
            .filter(|d| *d > Decimal::ZERO)
        else {
            issues.push(issue(
                F,
                row,
                "INVALID_DAYS",
                "days must be a positive number of whole or half days.",
            ));
            continue;
        };
        rows.push(HistoryRow {
            row,
            email: email.to_lowercase(),
            leave_type_code: code.to_uppercase(),
            from_date,
            to_date,
            days,
            reason: get(r, reason)
                .map(|s| s.chars().take(500).collect())
                .unwrap_or_else(|| "Imported from Keka".to_string()),
            approved_by_email: get(r, approver).map(|s| s.to_lowercase()),
        });
    }
    (rows, issues)
}

/// A person already in the organization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingUser {
    pub id: Uuid,
    /// `(leave type code, leave year)` that already have `import` ledger rows.
    pub imported: BTreeSet<(String, i32)>,
    /// Days already booked by their pending or approved leave.
    pub booked: BTreeSet<NaiveDate>,
}

/// What the plan needs to know about the organization.
#[derive(Debug, Clone, Default)]
pub struct OrgContext {
    /// Lower-cased.
    pub domains: BTreeSet<String>,
    /// Upper-cased code → id.
    pub leave_types: BTreeMap<String, Uuid>,
    /// Lower-cased email → person. People of other organizations are not listed; importing
    /// them fails when applied (emails are unique per instance).
    pub users: BTreeMap<String, ExistingUser>,
    /// Lower-cased names.
    pub groups: BTreeSet<String>,
    pub current_leave_year: i32,
    pub start_month: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewUser {
    pub email: String,
    pub full_name: String,
    pub date_of_joining: Option<NaiveDate>,
    pub employee_code: Option<String>,
    pub department: Option<String>,
    pub approver_group: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub users_to_create: Vec<NewUser>,
    pub groups_to_create: Vec<String>,
    pub balances: Vec<BalanceRow>,
    pub history: Vec<HistoryRow>,
    pub errors: Vec<Issue>,
    pub warnings: Vec<Issue>,
}

fn domain(email: &str) -> &str {
    email.rsplit_once('@').map(|(_, d)| d).unwrap_or("")
}

/// The dry-run plan (§5).
pub fn plan(
    balances: Vec<BalanceRow>,
    history: Vec<HistoryRow>,
    parse_issues: Vec<Issue>,
    ctx: &OrgContext,
) -> Plan {
    let mut p = Plan {
        errors: parse_issues,
        ..Plan::default()
    };
    let mut seen_rows = HashSet::new();
    let mut warned_users = HashSet::new();
    let mut creating: BTreeMap<String, NewUser> = BTreeMap::new();
    let mut new_groups: Vec<String> = Vec::new();
    for b in balances {
        const F: &str = "balances";
        if !ctx.domains.contains(domain(&b.email)) {
            p.errors.push(issue(
                F,
                b.row,
                "UNKNOWN_DOMAIN",
                "This email's domain is not registered for the organization.",
            ));
            continue;
        }
        if !ctx.leave_types.contains_key(&b.leave_type_code) {
            p.errors.push(issue(
                F,
                b.row,
                "UNKNOWN_LEAVE_TYPE",
                format!("There is no leave type with code {}.", b.leave_type_code),
            ));
            continue;
        }
        if !seen_rows.insert((b.email.clone(), b.leave_type_code.clone(), b.leave_year)) {
            p.errors.push(issue(
                F,
                b.row,
                "DUPLICATE_ROW",
                "This person, leave type and year are already in the file.",
            ));
            continue;
        }
        match ctx.users.get(&b.email) {
            Some(u) => {
                if warned_users.insert(b.email.clone()) {
                    p.warnings.push(issue(
                        F,
                        b.row,
                        "USER_EXISTS",
                        "Already a user: the balance is set on their account.",
                    ));
                }
                if u.imported
                    .contains(&(b.leave_type_code.clone(), b.leave_year))
                {
                    p.warnings.push(issue(
                        F,
                        b.row,
                        "BALANCE_REPLACES_EARLIER_IMPORT",
                        "Replaces the balance imported earlier for this leave type and year.",
                    ));
                }
            }
            None if !creating.contains_key(&b.email) => {
                let Some(full_name) = b.full_name.clone() else {
                    p.errors.push(issue(
                        F,
                        b.row,
                        "MISSING_VALUE",
                        "A new person needs a full_name.",
                    ));
                    continue;
                };
                if let Some(g) = &b.approver_group {
                    let known = ctx.groups.contains(&g.to_lowercase())
                        || new_groups.iter().any(|n| n.eq_ignore_ascii_case(g));
                    if !known {
                        new_groups.push(g.clone());
                        p.warnings.push(issue(
                            F,
                            b.row,
                            "GROUP_WILL_BE_CREATED",
                            format!("Group \"{g}\" will be created."),
                        ));
                    }
                }
                creating.insert(
                    b.email.clone(),
                    NewUser {
                        email: b.email.clone(),
                        full_name,
                        date_of_joining: b.date_of_joining,
                        employee_code: b.employee_code.clone(),
                        department: b.department.clone(),
                        approver_group: b.approver_group.clone(),
                    },
                );
            }
            None => {}
        }
        p.balances.push(b);
    }

    // History: known people only; no overlap with booked days or other rows in the file.
    let mut taken: BTreeMap<String, Vec<(NaiveDate, NaiveDate)>> = BTreeMap::new();
    for h in history {
        const F: &str = "history";
        if !ctx.domains.contains(domain(&h.email)) {
            p.errors.push(issue(
                F,
                h.row,
                "UNKNOWN_DOMAIN",
                "This email's domain is not registered for the organization.",
            ));
            continue;
        }
        if !ctx.leave_types.contains_key(&h.leave_type_code) {
            p.errors.push(issue(
                F,
                h.row,
                "UNKNOWN_LEAVE_TYPE",
                format!("There is no leave type with code {}.", h.leave_type_code),
            ));
            continue;
        }
        let existing = ctx.users.get(&h.email);
        if existing.is_none() && !creating.contains_key(&h.email) {
            p.errors.push(issue(
                F,
                h.row,
                "UNKNOWN_USER",
                "This person is neither a user nor in the balances file.",
            ));
            continue;
        }
        let booked =
            existing.is_some_and(|u| u.booked.range(h.from_date..=h.to_date).next().is_some());
        let ranges = taken.entry(h.email.clone()).or_default();
        let clashes = ranges
            .iter()
            .any(|(f, t)| h.from_date <= *t && *f <= h.to_date);
        if booked || clashes {
            p.errors.push(issue(
                F,
                h.row,
                "OVERLAP",
                "Overlaps leave already booked for this person.",
            ));
            continue;
        }
        ranges.push((h.from_date, h.to_date));
        let in_year = |d: NaiveDate| leave_year_of(d, ctx.start_month) == ctx.current_leave_year;
        if !in_year(h.from_date) || !in_year(h.to_date) {
            p.warnings.push(issue(
                F,
                h.row,
                "HISTORY_OUTSIDE_LEAVE_YEAR",
                "Not in the current leave year; imported anyway.",
            ));
        }
        p.history.push(h);
    }
    let order = |i: &Issue| (i.file != "balances", i.row);
    p.errors.sort_by_key(order);
    p.warnings.sort_by_key(order);
    p.users_to_create = creating.into_values().collect();
    p.users_to_create.sort_by(|a, b| a.email.cmp(&b.email));
    p.groups_to_create = new_groups;
    p
}
