//! Holiday CSV (T161, contracts/csv-formats.md §6): `date,name[,kind][,location]`, columns found
//! by header name in any order. Row numbers are 1-based and count the header line.
use chrono::NaiveDate;
use leavo_core::calendar::HolidayKind;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HolidayRow {
    pub row: usize,
    pub date: NaiveDate,
    pub name: String,
    pub kind: HolidayKind,
    pub location: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowError {
    pub row: usize,
    /// `MISSING_COLUMN`, `MISSING_VALUE`, `INVALID_DATE`, `INVALID_KIND` or `DUPLICATE_ROW`.
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parsed {
    pub rows: Vec<HolidayRow>,
    pub errors: Vec<RowError>,
}

fn err(row: usize, code: &'static str, message: impl Into<String>) -> RowError {
    RowError {
        row,
        code,
        message: message.into(),
    }
}

/// Parse a holiday CSV. Blank lines are skipped; an empty name (or one over 120 characters) is
/// `MISSING_VALUE`. The same date and name (ignoring case) twice is a `DUPLICATE_ROW` on the
/// later row.
pub fn parse_holidays(bytes: &[u8]) -> Parsed {
    let mut out = Parsed::default();
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(bytes);
    let headers: Vec<String> = match reader.headers() {
        Ok(h) => h.iter().map(|c| c.trim().to_ascii_lowercase()).collect(),
        Err(_) => Vec::new(),
    };
    let col = |name: &str| headers.iter().position(|h| h == name);
    let (Some(date_col), Some(name_col)) = (col("date"), col("name")) else {
        let missing = if col("date").is_none() {
            "date"
        } else {
            "name"
        };
        out.errors.push(err(
            1,
            "MISSING_COLUMN",
            format!("The file needs a \"{missing}\" column."),
        ));
        return out;
    };
    let (kind_col, location_col) = (col("kind"), col("location"));
    let mut seen = HashSet::new();
    for (i, record) in reader.records().enumerate() {
        let row = i + 2;
        let Ok(record) = record else {
            out.errors
                .push(err(row, "INVALID_DATE", "This row could not be read."));
            continue;
        };
        if record.iter().all(|c| c.trim().is_empty()) {
            continue;
        }
        let get = |c: Option<usize>| c.and_then(|c| record.get(c)).unwrap_or("").trim();
        let Ok(date) = NaiveDate::parse_from_str(get(Some(date_col)), "%Y-%m-%d") else {
            out.errors
                .push(err(row, "INVALID_DATE", "Use a real date as YYYY-MM-DD."));
            continue;
        };
        let name = get(Some(name_col));
        if name.is_empty() || name.chars().count() > 120 {
            out.errors.push(err(
                row,
                "MISSING_VALUE",
                "Give the holiday a name of up to 120 characters.",
            ));
            continue;
        }
        let kind = match get(kind_col).to_ascii_lowercase().as_str() {
            "" | "mandatory" => HolidayKind::Mandatory,
            "optional" => HolidayKind::Optional,
            _ => {
                out.errors
                    .push(err(row, "INVALID_KIND", "Kind is mandatory or optional."));
                continue;
            }
        };
        if !seen.insert((date, name.to_lowercase())) {
            out.errors.push(err(
                row,
                "DUPLICATE_ROW",
                "This holiday is already in the file.",
            ));
            continue;
        }
        let location = Some(get(location_col))
            .filter(|l| !l.is_empty())
            .map(str::to_string);
        out.rows.push(HolidayRow {
            row,
            date,
            name: name.to_string(),
            kind,
            location,
        });
    }
    out
}
