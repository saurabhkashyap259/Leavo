//! Calendar primitives: leave years, weekend masks, "today" in the org's time zone and
//! working-day counting (FR-016, FR-049, FR-055).
use chrono::{DateTime, Datelike, Months, NaiveDate, Utc, Weekday};
use chrono_tz::Tz;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// The leave year a date falls in, named after the calendar year in which that leave year starts.
pub fn leave_year_of(date: NaiveDate, start_month: u32) -> i32 {
    debug_assert!((1..=12).contains(&start_month));
    if date.month() >= start_month {
        date.year()
    } else {
        date.year() - 1
    }
}

/// First and last day (inclusive) of a leave year.
pub fn leave_year_bounds(leave_year: i32, start_month: u32) -> (NaiveDate, NaiveDate) {
    let start =
        NaiveDate::from_ymd_opt(leave_year, start_month, 1).expect("valid leave year start");
    let next = start + Months::new(12);
    (start, next.pred_opt().expect("valid date"))
}

/// The current date in the organization's time zone (FR-049).
pub fn today_in(tz: Tz, now: DateTime<Utc>) -> NaiveDate {
    now.with_timezone(&tz).date_naive()
}

/// Weekend days as a bitmask: Monday = 1 … Sunday = 64.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "i16", into = "i16")]
pub struct WeekendMask(u8);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WeekendMaskError {
    #[error("weekend mask {0} is out of range")]
    OutOfRange(i64),
    #[error("at least one working day is required")]
    NoWorkingDay,
}

fn bit(day: Weekday) -> u8 {
    1 << day.num_days_from_monday()
}

const ALL_DAYS: [Weekday; 7] = [
    Weekday::Mon,
    Weekday::Tue,
    Weekday::Wed,
    Weekday::Thu,
    Weekday::Fri,
    Weekday::Sat,
    Weekday::Sun,
];

impl WeekendMask {
    pub fn from_bits(bits: impl Into<i64>) -> Result<Self, WeekendMaskError> {
        let bits: i64 = bits.into();
        if !(0..=127).contains(&bits) {
            return Err(WeekendMaskError::OutOfRange(bits));
        }
        if bits == 127 {
            return Err(WeekendMaskError::NoWorkingDay);
        }
        Ok(Self(bits as u8))
    }

    pub fn from_weekdays(days: &[Weekday]) -> Self {
        Self(days.iter().fold(0, |acc, d| acc | bit(*d)))
    }

    pub fn bits(self) -> u8 {
        self.0
    }

    pub fn contains(self, day: Weekday) -> bool {
        self.0 & bit(day) != 0
    }

    pub fn weekdays(self) -> Vec<Weekday> {
        ALL_DAYS.into_iter().filter(|d| self.contains(*d)).collect()
    }
}

impl Default for WeekendMask {
    fn default() -> Self {
        Self::from_weekdays(&[Weekday::Sat, Weekday::Sun])
    }
}

impl TryFrom<i16> for WeekendMask {
    type Error = WeekendMaskError;
    fn try_from(v: i16) -> Result<Self, Self::Error> {
        Self::from_bits(v)
    }
}

impl From<WeekendMask> for i16 {
    fn from(m: WeekendMask) -> i16 {
        m.0.into()
    }
}

/// Which half of a day a half-day leave covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Half {
    First,
    Second,
}

impl Half {
    pub fn as_str(self) -> &'static str {
        match self {
            Half::First => "first",
            Half::Second => "second",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HolidayKind {
    Mandatory,
    Optional,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holiday {
    pub date: NaiveDate,
    pub name: String,
    pub kind: HolidayKind,
}

/// One day charged by a request. `leave_year` is the leave year the day falls in (FR-055).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BookedDay {
    pub day: NaiveDate,
    /// 1 or 0.5.
    pub portion: Decimal,
    pub half: Option<Half>,
    pub leave_year: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExcludedReason {
    Weekend,
    Holiday,
}

/// A date inside the range that is not charged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Excluded {
    pub date: NaiveDate,
    pub reason: ExcludedReason,
    /// The holiday's name, for holidays.
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DayCount {
    pub days: Vec<BookedDay>,
    pub excluded: Vec<Excluded>,
}

impl DayCount {
    pub fn total(&self) -> Decimal {
        self.days.iter().map(|d| d.portion).sum()
    }

    /// Days per leave year, in year order.
    pub fn by_leave_year(&self) -> Vec<(i32, Decimal)> {
        let mut out: Vec<(i32, Decimal)> = Vec::new();
        for d in &self.days {
            match out.iter_mut().find(|(y, _)| *y == d.leave_year) {
                Some((_, n)) => *n += d.portion,
                None => out.push((d.leave_year, d.portion)),
            }
        }
        out.sort_by_key(|(y, _)| *y);
        out
    }
}

/// Count the working days of `from..=to` (research R7).
///
/// - Weekend days (org mask) and **mandatory** holidays are excluded; optional holidays are
///   ordinary working days in v1 (FR-016). A mandatory holiday on a weekend is reported as a
///   holiday.
/// - `start_half` applies to `from` and `end_half` to `to`; a half on a date that is not a
///   working day is dropped. For a single-day range either half makes it 0.5.
/// - A reversed range counts nothing.
pub fn working_days(
    from: NaiveDate,
    to: NaiveDate,
    start_half: Option<Half>,
    end_half: Option<Half>,
    weekend: WeekendMask,
    holidays: &[Holiday],
    leave_year_start_month: u32,
) -> DayCount {
    let mut out = DayCount::default();
    if to < from {
        return out;
    }
    for day in from.iter_days().take_while(|d| *d <= to) {
        let holiday = holidays
            .iter()
            .find(|h| h.date == day && h.kind == HolidayKind::Mandatory);
        if let Some(h) = holiday {
            out.excluded.push(Excluded {
                date: day,
                reason: ExcludedReason::Holiday,
                name: Some(h.name.clone()),
            });
            continue;
        }
        if weekend.contains(day.weekday()) {
            out.excluded.push(Excluded {
                date: day,
                reason: ExcludedReason::Weekend,
                name: None,
            });
            continue;
        }
        let half = match (day == from, day == to) {
            (true, true) => start_half.or(end_half),
            (true, false) => start_half,
            (false, true) => end_half,
            (false, false) => None,
        };
        out.days.push(BookedDay {
            day,
            portion: if half.is_some() {
                Decimal::new(5, 1)
            } else {
                Decimal::ONE
            },
            half,
            leave_year: leave_year_of(day, leave_year_start_month),
        });
    }
    out
}
