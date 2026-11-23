//! Tenor parsing and business-day calendar date arithmetic.

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use chrono::{Datelike, Duration, Months, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};

use crate::error::QuantError;

/// Relative date offset matching `gs_quant` tenor syntax (`1b`, `1w`, `1m`, `3m`, `6m`, `1y`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Tenor {
    /// Positive number of weekday business days (`b`).
    BusinessDays(u32),
    /// Positive number of calendar weeks (`w`).
    Weeks(u32),
    /// Positive number of calendar months (`m`).
    Months(u32),
    /// Positive number of calendar years (`y`).
    Years(u32),
}

impl Tenor {
    /// Returns the approximate year fraction represented by this tenor using a 252-business-day / 365-day convention.
    #[must_use]
    pub fn approx_year_fraction(self) -> f64 {
        match self {
            Self::BusinessDays(n) => f64::from(n) / 252.0,
            Self::Weeks(n) => f64::from(n) * 7.0 / 365.0,
            Self::Months(n) => f64::from(n) / 12.0,
            Self::Years(n) => f64::from(n),
        }
    }

    /// Advances `date` by this tenor, rolling forward past weekends and any dates in `holidays`.
    ///
    /// For [`Tenor::BusinessDays`], each step advances to the next valid business day not in `holidays`.
    /// For calendar tenors (`Weeks`, `Months`, `Years`), the calendar offset is applied and then
    /// adjusted forward (following business-day convention) if it falls on a weekend or holiday.
    #[must_use]
    pub fn add_to_date(self, date: NaiveDate, holidays: &BTreeSet<NaiveDate>) -> NaiveDate {
        match self {
            Self::BusinessDays(0) | Self::Weeks(0) | Self::Months(0) | Self::Years(0) => {
                roll_to_business_day(date, holidays)
            }
            Self::BusinessDays(n) => {
                let mut current = date;
                for _ in 0..n {
                    current = next_business_day(current, holidays);
                }
                current
            }
            Self::Weeks(n) => {
                let raw = date + Duration::weeks(i64::from(n));
                roll_to_business_day(raw, holidays)
            }
            Self::Months(n) => {
                let raw = date
                    .checked_add_months(Months::new(n))
                    .unwrap_or(NaiveDate::MAX);
                roll_to_business_day(raw, holidays)
            }
            Self::Years(n) => {
                let months = n.saturating_mul(12);
                let raw = date
                    .checked_add_months(Months::new(months))
                    .unwrap_or(NaiveDate::MAX);
                roll_to_business_day(raw, holidays)
            }
        }
    }
}

impl fmt::Display for Tenor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BusinessDays(n) => write!(f, "{n}b"),
            Self::Weeks(n) => write!(f, "{n}w"),
            Self::Months(n) => write!(f, "{n}m"),
            Self::Years(n) => write!(f, "{n}y"),
        }
    }
}

impl FromStr for Tenor {
    type Err = QuantError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        if trimmed.len() < 2 {
            return Err(QuantError::InvalidTenor {
                input: s.to_owned(),
            });
        }
        let (num_part, unit_part) = trimmed.split_at(trimmed.len() - 1);
        let count: u32 = num_part.parse().map_err(|_| QuantError::InvalidTenor {
            input: s.to_owned(),
        })?;
        if count == 0 {
            return Err(QuantError::InvalidTenor {
                input: s.to_owned(),
            });
        }
        match unit_part.to_ascii_lowercase().as_str() {
            "b" => Ok(Self::BusinessDays(count)),
            "w" => Ok(Self::Weeks(count)),
            "m" => Ok(Self::Months(count)),
            "y" => Ok(Self::Years(count)),
            _ => Err(QuantError::InvalidTenor {
                input: s.to_owned(),
            }),
        }
    }
}

/// Returns `true` if `date` is Monday–Friday and not listed in `holidays`.
#[must_use]
pub fn is_business_day(date: NaiveDate, holidays: &BTreeSet<NaiveDate>) -> bool {
    !matches!(date.weekday(), Weekday::Sat | Weekday::Sun) && !holidays.contains(&date)
}

/// Moves `date` forward if it falls on a weekend or holiday; otherwise returns `date` unchanged.
#[must_use]
pub fn roll_to_business_day(date: NaiveDate, holidays: &BTreeSet<NaiveDate>) -> NaiveDate {
    let mut current = date;
    while !is_business_day(current, holidays) {
        current += Duration::days(1);
    }
    current
}

/// Returns the strictly next business day after `date`.
#[must_use]
pub fn next_business_day(date: NaiveDate, holidays: &BTreeSet<NaiveDate>) -> NaiveDate {
    roll_to_business_day(date + Duration::days(1), holidays)
}

/// Generates an ordered schedule of business dates within `[start, end]` spaced by `frequency`.
///
/// Matches `gs_quant`'s periodic schedule behavior where `1b` enumerates every business day
/// (excluding weekends and `holidays`), while coarser frequencies (`1w`, `1m`, etc.) step by
/// `frequency` from the first business day on or after `start` up to `end`.
///
/// # Errors
///
/// Returns [`QuantError::InvalidParameter`] if `start > end`.
pub fn business_day_schedule(
    start: NaiveDate,
    end: NaiveDate,
    frequency: Tenor,
    holidays: &BTreeSet<NaiveDate>,
) -> Result<Vec<NaiveDate>, QuantError> {
    if start > end {
        return Err(QuantError::invalid_parameter(format!(
            "start date {start} is after end date {end}"
        )));
    }

    let mut dates = Vec::new();
    let mut current = roll_to_business_day(start, holidays);
    while current <= end {
        dates.push(current);
        let next = match frequency {
            Tenor::BusinessDays(1) => next_business_day(current, holidays),
            _ => {
                let stepped = frequency.add_to_date(current, holidays);
                if stepped <= current {
                    next_business_day(current, holidays)
                } else {
                    stepped
                }
            }
        };
        current = next;
    }
    Ok(dates)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_display_tenors() {
        assert_eq!("1b".parse::<Tenor>().unwrap(), Tenor::BusinessDays(1));
        assert_eq!("2W".parse::<Tenor>().unwrap(), Tenor::Weeks(2));
        assert_eq!("3m".parse::<Tenor>().unwrap(), Tenor::Months(3));
        assert_eq!("1Y".parse::<Tenor>().unwrap(), Tenor::Years(1));
        assert_eq!(Tenor::Months(6).to_string(), "6m");
        assert!("0m".parse::<Tenor>().is_err());
        assert!("10x".parse::<Tenor>().is_err());
    }

    #[test]
    fn schedule_skips_weekends_and_holidays() {
        let holiday = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        let holidays = BTreeSet::from([holiday]);
        let start = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2025, 1, 6).unwrap();

        let sched = business_day_schedule(start, end, Tenor::BusinessDays(1), &holidays).unwrap();
        assert_eq!(
            sched,
            vec![
                NaiveDate::from_ymd_opt(2025, 1, 2).unwrap(),
                NaiveDate::from_ymd_opt(2025, 1, 3).unwrap(),
                NaiveDate::from_ymd_opt(2025, 1, 6).unwrap(),
            ]
        );
    }
}
