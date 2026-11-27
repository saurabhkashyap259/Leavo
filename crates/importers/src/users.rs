//! Employee invite CSV (T182, contracts/csv-formats.md §1): `email,full_name[,employee_code]
//! [,department][,date_of_joining][,role][,approver_group]`, columns by header name. Row numbers
//! count the header line. Domain and existing-user checks need the database and are done by the
//! caller.
use chrono::NaiveDate;
use std::collections::HashSet;

pub use crate::holidays::RowError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InviteRow {
    pub row: usize,
    /// Lower-cased.
    pub email: String,
    pub full_name: String,
    pub employee_code: Option<String>,
    pub department: Option<String>,
    pub date_of_joining: Option<NaiveDate>,
    /// `employee`, `approver` or `admin`.
    pub role: String,
    pub approver_group: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedInvites {
    pub rows: Vec<InviteRow>,
    pub errors: Vec<RowError>,
}

fn err(row: usize, code: &'static str, message: impl Into<String>) -> RowError {
    RowError {
        row,
        code,
        message: message.into(),
    }
}

fn valid_email(e: &str) -> bool {
    let mut parts = e.split('@');
    matches!((parts.next(), parts.next(), parts.next()),
        (Some(l), Some(d), None) if !l.is_empty() && d.contains('.') && !d.starts_with('.') && !d.ends_with('.'))
        && !e.contains(char::is_whitespace)
}

pub fn parse_invites(bytes: &[u8]) -> ParsedInvites {
    let mut out = ParsedInvites::default();
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(bytes);
    let headers: Vec<String> = match reader.headers() {
        Ok(h) => h.iter().map(|c| c.trim().to_ascii_lowercase()).collect(),
        Err(_) => Vec::new(),
    };
    let col = |name: &str| headers.iter().position(|h| h == name);
    let (Some(email_col), Some(name_col)) = (col("email"), col("full_name")) else {
        let missing = if col("email").is_none() {
            "email"
        } else {
            "full_name"
        };
        out.errors.push(err(
            1,
            "MISSING_COLUMN",
            format!("The file needs a \"{missing}\" column."),
        ));
        return out;
    };
    let (code_col, dept_col, doj_col, role_col, group_col) = (
        col("employee_code"),
        col("department"),
        col("date_of_joining"),
        col("role"),
        col("approver_group"),
    );
    let mut seen = HashSet::new();
    let mut codes = HashSet::new();
    for (i, record) in reader.records().enumerate() {
        let row = i + 2;
        let Ok(record) = record else {
            out.errors
                .push(err(row, "MISSING_VALUE", "This row could not be read."));
            continue;
        };
        if record.iter().all(|c| c.trim().is_empty()) {
            continue;
        }
        let get = |c: Option<usize>| {
            c.and_then(|c| record.get(c))
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let opt = |c: Option<usize>| Some(get(c)).filter(|v| !v.is_empty());
        let email = get(Some(email_col)).to_lowercase();
        let full_name = get(Some(name_col));
        if email.is_empty() || full_name.is_empty() || full_name.chars().count() > 120 {
            out.errors.push(err(
                row,
                "MISSING_VALUE",
                "Each row needs an email and a full name (up to 120 characters).",
            ));
            continue;
        }
        if !valid_email(&email) {
            out.errors.push(err(
                row,
                "INVALID_EMAIL",
                "This is not a valid email address.",
            ));
            continue;
        }
        let date_of_joining = match opt(doj_col) {
            None => None,
            Some(d) => match NaiveDate::parse_from_str(&d, "%Y-%m-%d") {
                Ok(d) => Some(d),
                Err(_) => {
                    out.errors
                        .push(err(row, "INVALID_DATE", "Use a real date as YYYY-MM-DD."));
                    continue;
                }
            },
        };
        let role = opt(role_col)
            .unwrap_or_else(|| "employee".into())
            .to_lowercase();
        if !["employee", "approver", "admin"].contains(&role.as_str()) {
            out.errors.push(err(
                row,
                "INVALID_ROLE",
                "Role is employee, approver or admin.",
            ));
            continue;
        }
        if !seen.insert(email.clone()) {
            out.errors.push(err(
                row,
                "DUPLICATE_ROW",
                "This email is already in the file.",
            ));
            continue;
        }
        let employee_code = opt(code_col);
        if let Some(c) = &employee_code {
            if !codes.insert(c.clone()) {
                out.errors.push(err(
                    row,
                    "DUPLICATE_ROW",
                    "This employee code is already in the file.",
                ));
                continue;
            }
        }
        out.rows.push(InviteRow {
            row,
            email,
            full_name,
            employee_code,
            department: opt(dept_col),
            date_of_joining,
            role,
            approver_group: opt(group_col),
        });
    }
    out
}
