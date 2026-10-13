//! Date parsing for the `[d]` query modifier. Seeded with absolute
//! ISO dates so Phase 2 queries can compare date values; the full
//! grammar (keywords, times, durations, combinations) lands in
//! Phase 3 per the roadmap.

/// A parsed date value: days since 1970-01-01. Date-level
/// comparisons only; times arrive with Phase 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DateValue(pub i64);

/// Parse an absolute date: `YYYY`, `YYYY-MM`, or `YYYY-MM-DD`,
/// resolving to the first of the period. Anything else is
/// unparseable (and comparisons against it will be false).
pub fn parse_date(text: &str) -> Option<DateValue> {
    let parts: Vec<&str> = text.trim().split('-').collect();
    let (y, m, d) = match parts.as_slice() {
        [y] => (*y, "1", "1"),
        [y, m] => (*y, *m, "1"),
        [y, m, d] => (*y, *m, *d),
        _ => return None,
    };
    let year: i64 = y.parse().ok()?;
    if !(1..=12).contains(&m.parse::<u32>().ok()?) {
        return None;
    }
    let month: u32 = m.parse().ok()?;
    let day: u32 = d.parse().ok()?;
    if !(1..=days_in_month(year, month)).contains(&day) {
        return None;
    }
    Some(DateValue(days_from_civil(year, month, day)))
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Howard Hinnant's `days_from_civil`: days since 1970-01-01 for a
/// proleptic Gregorian date.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (if m > 2 { m - 3 } else { m + 9 }) as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::{DateValue, days_from_civil, parse_date};

    #[test]
    fn iso_forms_parse_to_first_of_period() {
        assert_eq!(
            parse_date("2026"),
            Some(DateValue(days_from_civil(2026, 1, 1)))
        );
        assert_eq!(
            parse_date("2026-10"),
            Some(DateValue(days_from_civil(2026, 10, 1)))
        );
        assert_eq!(
            parse_date("2026-10-14"),
            Some(DateValue(days_from_civil(2026, 10, 14)))
        );
    }

    #[test]
    fn epoch_checks() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(1970, 1, 2), 1);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
        assert_eq!(
            days_from_civil(2026, 10, 14),
            days_from_civil(2026, 10, 13) + 1
        );
    }

    #[test]
    fn garbage_and_invalid_dates_are_none() {
        assert_eq!(parse_date("tomorrow"), None);
        assert_eq!(parse_date("2026-13-01"), None);
        assert_eq!(parse_date("2026-02-30"), None);
        assert_eq!(parse_date(""), None);
        assert_eq!(parse_date("2026-10-14x"), None);
    }

    #[test]
    fn leap_years() {
        assert!(parse_date("2024-02-29").is_some());
        assert!(parse_date("2026-02-29").is_none());
    }
}
