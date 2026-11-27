//! Validation shared by setup, `POST /orgs` and `PATCH /admin/org` (FR-010).
use crate::error::{field_error, ApiResult};
use chrono::Weekday;
use leavo_core::calendar::WeekendMask;

pub fn name(field: &str, raw: &str) -> ApiResult<String> {
    let n = raw.trim();
    if n.is_empty() || n.chars().count() > 120 {
        return Err(field_error(field));
    }
    Ok(n.to_string())
}

pub fn timezone(field: &str, raw: &str) -> ApiResult<String> {
    raw.trim()
        .parse::<chrono_tz::Tz>()
        .map(|tz| tz.name().to_string())
        .map_err(|_| field_error(field))
}

pub fn start_month(field: &str, m: i64) -> ApiResult<i16> {
    if (1..=12).contains(&m) {
        Ok(m as i16)
    } else {
        Err(field_error(field))
    }
}

fn weekday(s: &str) -> Option<Weekday> {
    Some(match s.trim().to_ascii_lowercase().as_str() {
        "mon" => Weekday::Mon,
        "tue" => Weekday::Tue,
        "wed" => Weekday::Wed,
        "thu" => Weekday::Thu,
        "fri" => Weekday::Fri,
        "sat" => Weekday::Sat,
        "sun" => Weekday::Sun,
        _ => return None,
    })
}

/// `["sat","sun"]` → mask. At least one working day must remain.
pub fn weekend(field: &str, days: &[String]) -> ApiResult<i16> {
    let parsed: Option<Vec<Weekday>> = days.iter().map(|d| weekday(d)).collect();
    let parsed = parsed.ok_or_else(|| field_error(field))?;
    let mask = WeekendMask::from_weekdays(&parsed);
    if mask.weekdays().len() >= 7 {
        return Err(field_error(field));
    }
    Ok(i16::from(mask.bits()))
}

/// Mask → `["sat","sun"]` (Monday first).
pub fn weekend_names(mask: i16) -> Vec<String> {
    WeekendMask::from_bits(mask)
        .unwrap_or_default()
        .weekdays()
        .into_iter()
        .map(|d| d.to_string().to_lowercase())
        .collect()
}
