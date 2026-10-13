//! The date system: the full TaskPaper grammar plus a strict ISO
//! 8601 fallback, per the spec's Dates section.
//!
//! `parse` tries the grammar first, then strict ISO (week dates and
//! `T`-separated timestamps are the forms the grammar does not
//! reach). Values are milliseconds on the local wall-clock timeline;
//! keywords and durations resolve against an injectable reference
//! instant (`parse(text, reference)`; `now()` for the real clock).
//! Everything is local time, midnight resolution for pure dates,
//! Monday-start ISO weeks. Deliberate deviations from the reference
//! grammar, documented in spec.md: the empty string does not parse,
//! and `12am`/`12pm` keep the faithful noon/next-midnight arithmetic
//! (pm adds twelve, unconditionally).

use crate::tz;

/// A moment in milliseconds on the local wall-clock timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DateTime(pub i64);

const MS: i64 = 1;
const SECOND: i64 = 1000;
const MINUTE: i64 = 60 * SECOND;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;
const WEEK: i64 = 7 * DAY;

/// Milliseconds on the local timeline for "right now".
pub fn now() -> i64 {
    tz::now_millis()
}

/// Parse a date expression against a reference instant.
pub fn parse(text: &str, reference: i64) -> Option<DateTime> {
    parse_grammar(text, reference).or_else(|| parse_iso_strict(text))
}

// ---------------------------------------------------------------------
// Civil calendar math (proleptic Gregorian; Howard Hinnant's algorithms)
// ---------------------------------------------------------------------

pub(crate) fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (if m > 2 { m - 3 } else { m + 9 }) as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Weekday index, Monday = 0.
fn weekday_mon0(days: i64) -> i64 {
    (days + 3).rem_euclid(7)
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap(y) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

/// Calendar month arithmetic with the reference behavior's clamp:
/// Jan 31 + 1 month lands on Feb 28/29, not March 3.
fn add_months(y: i64, m: u32, d: u32, delta: i64) -> i64 {
    let total = y * 12 + m as i64 - 1 + delta;
    let ny = total.div_euclid(12);
    let nm = (total.rem_euclid(12) + 1) as u32;
    let nd = d.min(days_in_month(ny, nm));
    days_from_civil(ny, nm, nd)
}

fn midnight(ms: i64) -> i64 {
    ms.div_euclid(DAY) * DAY
}

/// A moment as (days since epoch, milliseconds of the day).
fn split(ms: i64) -> (i64, i64) {
    (ms.div_euclid(DAY), ms.rem_euclid(DAY))
}

fn year_of(ms: i64) -> i64 {
    civil_from_days(split(ms).0).0
}

fn join(days: i64, ms_of_day: i64) -> i64 {
    days * DAY + ms_of_day
}

// ---------------------------------------------------------------------
// Units and names
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    Millisecond,
    Second,
    Minute,
    Hour,
    Day,
    Week,
    IsoWeek,
    Month,
    Quarter,
    Year,
}

/// The unit alias table, whole-run matched case-insensitively. The
/// single-letter shorthands follow the reference grammar exactly:
/// `m` is minutes, months' letter is `o`. Whole-run matching
/// reproduces the grammar's prefix guards (`sa` is not `s`, `mo` is
/// not `m`, and so on) because an over-long run matches nothing.
const UNITS: &[(&str, Unit)] = &[
    ("milliseconds", Unit::Millisecond),
    ("millisecond", Unit::Millisecond),
    ("seconds", Unit::Second),
    ("second", Unit::Second),
    ("minutes", Unit::Minute),
    ("minute", Unit::Minute),
    ("hours", Unit::Hour),
    ("hour", Unit::Hour),
    ("days", Unit::Day),
    ("day", Unit::Day),
    ("weeks", Unit::Week),
    ("week", Unit::Week),
    ("isoweeks", Unit::IsoWeek),
    ("isoweek", Unit::IsoWeek),
    ("months", Unit::Month),
    ("month", Unit::Month),
    ("quarters", Unit::Quarter),
    ("quarter", Unit::Quarter),
    ("years", Unit::Year),
    ("year", Unit::Year),
    ("ms", Unit::Millisecond),
    ("sec", Unit::Second),
    ("min", Unit::Minute),
    ("mins", Unit::Minute),
    ("s", Unit::Second),
    ("m", Unit::Minute),
    ("h", Unit::Hour),
    ("d", Unit::Day),
    ("w", Unit::Week),
    ("isow", Unit::IsoWeek),
    ("o", Unit::Month),
    ("q", Unit::Quarter),
    ("y", Unit::Year),
];

const MONTHS: &[&str] = &[
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

const WEEKDAYS: &[&str] = &[
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];

/// Look up the whole alphanumeric run at `pos` in a name table,
/// where each entry matches either its full form or exactly its
/// first three letters (`sept`, `tues`, `thur` are correctly
/// rejected). Returns the table index and the run length.
fn match_name(text: &str, pos: usize, table: &[&str]) -> Option<(usize, usize)> {
    let run = alpha_run(text, pos);
    if run.is_empty() {
        return None;
    }
    let lower = run.to_lowercase();
    for (i, name) in table.iter().enumerate() {
        if lower == *name || (lower.len() == 3 && name.starts_with(&lower)) {
            return Some((i, run.len()));
        }
    }
    None
}

fn alpha_run(text: &str, pos: usize) -> &str {
    let rest = &text[pos..];
    let end = rest
        .char_indices()
        .find(|(_, c)| !c.is_ascii_alphabetic())
        .map(|(i, _)| i)
        .unwrap_or(rest.len());
    &rest[..end]
}

fn digit_run(text: &str, pos: usize) -> Option<(i64, usize)> {
    let rest = &text[pos..];
    let end = rest
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit())
        .map(|(i, _)| i)
        .unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    rest[..end].parse().ok().map(|v| (v, end))
}

// ---------------------------------------------------------------------
// Grammar
// ---------------------------------------------------------------------

struct Scanner<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> Scanner<'a> {
    fn peek_byte(&self) -> Option<u8> {
        self.text.as_bytes().get(self.pos).copied()
    }

    fn at_end(&self) -> bool {
        self.pos >= self.text.len()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek_byte(), Some(b) if (b as char).is_whitespace()) {
            self.pos += 1;
        }
    }

    fn eat_ci_word(&mut self, word: &str) -> bool {
        let run = alpha_run(self.text, self.pos);
        if run.eq_ignore_ascii_case(word) {
            self.pos += run.len();
            true
        } else {
            false
        }
    }
}

/// One parsed duration: a fixed millisecond offset or a calendar
/// shift in months (quarters are three months, years twelve).
#[derive(Debug, Clone, Copy)]
enum Duration {
    Fixed(i64),
    Months(i64),
}

fn parse_grammar(text: &str, reference: i64) -> Option<DateTime> {
    if text.trim().is_empty() {
        // Deviation: the reference grammar resolves empty input to
        // now; Docket rejects it as a likely user mistake.
        return None;
    }
    let mut p = Scanner { text, pos: 0 };
    let mut base: Option<i64> = None;
    let mut time_of_day: Option<i64> = None;
    let mut durations: Vec<Duration> = Vec::new();

    p.skip_ws();
    if let Some(d) = try_date(&mut p, reference) {
        base = Some(d);
        p.skip_ws();
    }
    if let Some(tod) = try_time(&mut p) {
        time_of_day = Some(tod);
        p.skip_ws();
    }
    while let Some(d) = try_duration(&mut p) {
        durations.push(d);
        p.skip_ws();
    }
    if !p.at_end() {
        return None;
    }
    if base.is_none() && time_of_day.is_none() && durations.is_empty() {
        return None;
    }

    // Evaluation: date (midnight) or now; a time floors the base to
    // midnight first; then durations apply in order. Duration-only
    // expressions keep the reference's time of day.
    let mut ms = match (base, time_of_day) {
        (Some(b), Some(tod)) => midnight(b) + tod,
        (Some(b), None) => b,
        (None, Some(tod)) => midnight(reference) + tod,
        (None, None) => reference,
    };
    for d in durations {
        ms = match d {
            Duration::Fixed(f) => ms + f,
            Duration::Months(delta) => {
                let (days, tod) = split(ms);
                let (y, m, d) = civil_from_days(days);
                join(add_months(y, m, d, delta), tod)
            }
        };
    }
    Some(DateTime(ms))
}

/// Try the date forms: keywords, absolute dates, month names and
/// weekday names (optionally with this/next/last), and
/// specifier-plus-unit forms. A pure date lands on midnight; the
/// `now` keyword keeps the reference's time of day.
fn try_date(p: &mut Scanner, reference: i64) -> Option<i64> {
    let save = p.pos;

    // Keywords, whole words only.
    for (word, value) in [
        ("now", Some(reference)),
        ("today", Some(midnight(reference))),
        ("tomorrow", Some(midnight(reference) + DAY)),
        ("yesterday", Some(midnight(reference) - DAY)),
    ] {
        if p.eat_ci_word(word) {
            return value;
        }
        p.pos = save;
    }

    // Absolute: YYYY or YYYY-MM or YYYY-MM-DD. The year is rejected
    // as a date when followed by a duration unit, an am/pm period, or
    // a colon, so "2012 seconds" stays a duration.
    if let Some((year, len)) = digit_run(p.text, p.pos) {
        let after_year = p.pos + len;
        let mut probe = Scanner {
            text: p.text,
            pos: after_year,
        };
        probe.skip_ws();
        let next = alpha_run(probe.text, probe.pos).to_lowercase();
        let guarded = UNITS.iter().any(|(name, _)| *name == next)
            || next == "am"
            || next == "pm"
            || probe.peek_byte() == Some(b':');
        if !guarded {
            p.pos = after_year;
            p.skip_ws();
            let parsed = (|| -> Option<i64> {
                let mut month = 1u32;
                let mut day = 1u32;
                if p.peek_byte() == Some(b'-') {
                    p.pos += 1;
                    let (m, l) = digit_run(p.text, p.pos)?;
                    month = u32::try_from(m).ok()?;
                    p.pos += l;
                    if p.peek_byte() == Some(b'-') {
                        p.pos += 1;
                        let (d, l) = digit_run(p.text, p.pos)?;
                        day = u32::try_from(d).ok()?;
                        p.pos += l;
                    }
                }
                // Out-of-range fields normalize by carry, like the
                // reference constructor (month 13, day 32, ...).
                let total = year * 12 + month as i64 - 1;
                let y2 = total.div_euclid(12);
                let m2 = (total.rem_euclid(12) + 1) as u32;
                Some(days_from_civil(y2, m2, 1) + (day as i64 - 1))
            })();
            match parsed {
                Some(days) => return Some(days * DAY),
                None => p.pos = save,
            }
        } else {
            p.pos = save;
        }
    }

    // Optional this/next/last specifier, shared by month names,
    // weekday names, and unit forms.
    let save2 = p.pos;
    let relative = ["this", "next", "last"]
        .iter()
        .find_map(|w| {
            if p.eat_ci_word(w) {
                Some(match *w {
                    "this" => 0i64,
                    "next" => 1i64,
                    _ => -1i64,
                })
            } else {
                None
            }
        })
        .or_else(|| {
            p.pos = save2;
            None
        });
    p.skip_ws();

    // Month name with optional day.
    if let Some((mi, len)) = match_name(p.text, p.pos, MONTHS) {
        p.pos += len;
        let mut day = 1u32;
        let day_pos = p.pos
            + if matches!(p.peek_byte(), Some(b) if (b as char).is_whitespace()) {
                1
            } else {
                0
            };
        if let Some((d, l)) = digit_run(p.text, day_pos) {
            day = u32::try_from(d).ok()?;
            p.pos = day_pos + l;
        }
        // next/last on a month name shifts a whole year.
        let shift = relative.unwrap_or(0) * 12;
        return Some(add_months(year_of(reference), (mi + 1) as u32, day, shift) * DAY);
    }

    // Weekday name: that weekday of the current ISO week, shifted a
    // week by next/last.
    if let Some((wi, len)) = match_name(p.text, p.pos, WEEKDAYS) {
        p.pos += len;
        let today = split(reference).0;
        let week_start = today - weekday_mon0(today);
        return Some((week_start + wi as i64 + relative.unwrap_or(0) * 7) * DAY);
    }
    // A consumed specifier must survive the month/weekday misses.
    if relative.is_none() {
        p.pos = save2;
    }

    // this/next/last + unit (specifier required): start of that
    // unit, then shifted by the specifier.
    let shift = relative?;
    let (unit, len) = unit_at(p)?;
    p.pos += len;
    let start = start_of_unit(reference, unit);
    Some(match shift {
        0 => start,
        1 => add_unit(start, unit, 1),
        _ => add_unit(start, unit, -1),
    })
}

fn unit_at(p: &Scanner) -> Option<(Unit, usize)> {
    let run = alpha_run(p.text, p.pos);
    if run.is_empty() {
        return None;
    }
    let lower = run.to_lowercase();
    UNITS
        .iter()
        .find(|(name, _)| *name == lower.as_str())
        .map(|(_, unit)| (*unit, run.len()))
}

fn start_of_unit(ms: i64, unit: Unit) -> i64 {
    let (days, tod) = split(ms);
    match unit {
        Unit::Millisecond => ms,
        Unit::Second => ms - tod % SECOND,
        Unit::Minute => ms - tod % MINUTE,
        Unit::Hour => ms - tod % HOUR,
        Unit::Day => midnight(ms),
        Unit::Week | Unit::IsoWeek => (days - weekday_mon0(days)) * DAY,
        Unit::Month => {
            let (y, m, _) = civil_from_days(days);
            days_from_civil(y, m, 1) * DAY
        }
        Unit::Quarter => {
            let (y, m, _) = civil_from_days(days);
            days_from_civil(y, ((m - 1) / 3) * 3 + 1, 1) * DAY
        }
        Unit::Year => {
            let (y, _, _) = civil_from_days(days);
            days_from_civil(y, 1, 1) * DAY
        }
    }
}

fn add_unit(ms: i64, unit: Unit, n: i64) -> i64 {
    match unit {
        Unit::Month | Unit::Quarter | Unit::Year => {
            let (days, tod) = split(ms);
            let (y, m, d) = civil_from_days(days);
            let months = match unit {
                Unit::Month => n,
                Unit::Quarter => 3 * n,
                _ => 12 * n,
            };
            join(add_months(y, m, d, months), tod)
        }
        Unit::Week | Unit::IsoWeek => ms + n * WEEK,
        Unit::Day => ms + n * DAY,
        Unit::Hour => ms + n * HOUR,
        Unit::Minute => ms + n * MINUTE,
        Unit::Second => ms + n * SECOND,
        Unit::Millisecond => ms + n * MS,
    }
}

/// Try the time forms: `at? H am|pm` (period required) or
/// `H:MM[:SS[:SSS]]` with an optional period. `pm` adds twelve
/// hours, unconditionally, exactly like the reference grammar.
fn try_time(p: &mut Scanner) -> Option<i64> {
    let save = p.pos;

    // at-form
    p.eat_ci_word("at");
    p.skip_ws();
    if let Some((h, l)) = digit_run(p.text, p.pos) {
        p.pos += l;
        p.skip_ws();
        let run = alpha_run(p.text, p.pos).to_lowercase();
        let period = match run.as_str() {
            "am" => Some(0),
            "pm" => Some(12 * HOUR),
            _ => None,
        };
        if let Some(period) = period {
            p.pos += run.len();
            return Some(h * HOUR + period);
        }
    }
    p.pos = save;

    // colon-form
    if let Some((h, hl)) = digit_run(p.text, p.pos) {
        let after_h = p.pos + hl;
        if p.text.as_bytes().get(after_h) == Some(&b':') {
            p.pos = after_h + 1;
            let (m, ml) = digit_run(p.text, p.pos)?;
            p.pos += ml;
            let mut tod = h * HOUR + m * MINUTE;
            if p.peek_byte() == Some(b':') {
                p.pos += 1;
                let (s, sl) = digit_run(p.text, p.pos)?;
                p.pos += sl;
                tod += s * SECOND;
                if p.peek_byte() == Some(b':') {
                    p.pos += 1;
                    let (ms, msl) = digit_run(p.text, p.pos)?;
                    p.pos += msl;
                    tod += ms;
                }
            }
            p.skip_ws();
            let run = alpha_run(p.text, p.pos).to_lowercase();
            match run.as_str() {
                "pm" => {
                    p.pos += run.len();
                    tod += 12 * HOUR;
                }
                "am" => p.pos += run.len(),
                _ => {}
            }
            return Some(tod);
        }
    }
    p.pos = save;
    None
}

/// Try one duration: optional sign, integer, unit. Whitespace is
/// optional between all three.
fn try_duration(p: &mut Scanner) -> Option<Duration> {
    let save = p.pos;
    let mut sign = 1i64;
    match p.peek_byte() {
        Some(b'+') => p.pos += 1,
        Some(b'-') => {
            p.pos += 1;
            sign = -1;
        }
        _ => {}
    }
    p.skip_ws();
    p.skip_ws();
    let Some((n, l)) = digit_run(p.text, p.pos) else {
        p.pos = save;
        return None;
    };
    p.pos += l;
    p.skip_ws();
    let Some((unit, ul)) = unit_at(p) else {
        // No unit: not a duration. Restore fully, sign and all.
        p.pos = save;
        return None;
    };
    p.pos += ul;
    Some(match unit {
        Unit::Month => Duration::Months(sign * n),
        Unit::Quarter => Duration::Months(sign * n * 3),
        Unit::Year => Duration::Months(sign * n * 12),
        Unit::Millisecond => Duration::Fixed(sign * n * MS),
        Unit::Second => Duration::Fixed(sign * n * SECOND),
        Unit::Minute => Duration::Fixed(sign * n * MINUTE),
        Unit::Hour => Duration::Fixed(sign * n * HOUR),
        Unit::Day => Duration::Fixed(sign * n * DAY),
        Unit::Week | Unit::IsoWeek => Duration::Fixed(sign * n * WEEK),
    })
}

// ---------------------------------------------------------------------
// Strict ISO 8601 fallback
// ---------------------------------------------------------------------

fn parse_iso_strict(text: &str) -> Option<DateTime> {
    parse_iso_week_date(text).or_else(|| parse_iso_datetime(text))
}

/// `YYYY-Www` and `YYYY-Www-D`, with or without dashes.
fn parse_iso_week_date(text: &str) -> Option<DateTime> {
    let b = text.as_bytes();
    let (year, l) = digit_run(text, 0)?;
    if l != 4 {
        return None;
    }
    let mut i = l;
    if b.get(i) == Some(&b'-') {
        i += 1;
    }
    if b.get(i) != Some(&b'W') {
        return None;
    }
    i += 1;
    let (week, l) = digit_run(text, i)?;
    if l != 2 {
        return None;
    }
    i += l;
    let day = if b.get(i) == Some(&b'-') {
        i += 1;
        let (d, dl) = digit_run(text, i)?;
        if dl != 1 {
            return None;
        }
        i += dl;
        d
    } else if let Some((d, dl)) = digit_run(text, i) {
        if dl != 1 {
            return None;
        }
        i += dl;
        d
    } else {
        1
    };
    if i != b.len() || !(1..=53).contains(&week) || !(1..=7).contains(&day) {
        return None;
    }
    // ISO week 1 is the week containing the first Thursday.
    let jan1 = days_from_civil(year, 1, 1);
    let week1_monday = jan1 - weekday_mon0(jan1) + if weekday_mon0(jan1) <= 3 { 0 } else { 7 };
    Some(DateTime((week1_monday + (week - 1) * 7 + (day - 1)) * DAY))
}

/// `YYYY-MM-DD[T ]HH:MM[:SS[.fff|:fff]]` with an optional zone
/// suffix (`Z`, `±HH`, `±HHMM`, `±HH:MM`). Zoneless input is local
/// wall time, matching the reference fallback's convention.
fn parse_iso_datetime(text: &str) -> Option<DateTime> {
    let b = text.as_bytes();
    let (y, l) = digit_run(text, 0)?;
    if l != 4 {
        return None;
    }
    let mut i = l;
    if b.get(i) != Some(&b'-') {
        return None;
    }
    i += 1;
    let (mo, l) = digit_run(text, i)?;
    if l != 2 {
        return None;
    }
    i += l;
    if b.get(i) != Some(&b'-') {
        return None;
    }
    i += 1;
    let (d, l) = digit_run(text, i)?;
    if l != 2 {
        return None;
    }
    i += l;
    if !matches!(b.get(i), Some(b'T') | Some(b' ')) {
        return None;
    }
    i += 1;
    let (hh, l) = digit_run(text, i)?;
    if l != 2 {
        return None;
    }
    i += l;
    if b.get(i) != Some(&b':') {
        return None;
    }
    i += 1;
    let (mm, l) = digit_run(text, i)?;
    if l != 2 {
        return None;
    }
    i += l;
    let mut ss = 0i64;
    let mut ms_of_ss = 0i64;
    if b.get(i) == Some(&b':') {
        i += 1;
        let (v, l) = digit_run(text, i)?;
        if l != 2 {
            return None;
        }
        ss = v;
        i += l;
        if matches!(b.get(i), Some(b'.') | Some(b':')) {
            i += 1;
            let (v, l) = digit_run(text, i)?;
            if !(1..=3).contains(&l) {
                return None;
            }
            ms_of_ss = v * 10i64.pow(3 - l as u32);
            i += l;
        }
    }
    if !(0..=23).contains(&hh) || !(0..=59).contains(&mm) || !(0..=59).contains(&ss) {
        return None;
    }

    let wall = days_from_civil(y, mo as u32, d as u32) * DAY
        + hh * HOUR
        + mm * MINUTE
        + ss * SECOND
        + ms_of_ss;
    let offset_ms = match b.get(i) {
        None => return Some(DateTime(wall)),
        Some(b'Z') => {
            i += 1;
            if i != b.len() {
                return None;
            }
            0i64
        }
        Some(sign @ (b'+' | b'-')) => {
            eprintln!("[off] sign at {i}");
            let sign = if *sign == b'-' { -1i64 } else { 1i64 };
            i += 1;
            let (oh, l) = digit_run(text, i)?;
            let (oh, om) = if l == 4 {
                i += l;
                (oh / 100, oh % 100)
            } else {
                if l != 2 {
                    return None;
                }
                i += l;
                if b.get(i) == Some(&b':') {
                    i += 1;
                }
                let (omv, l) = digit_run(text, i)?;
                if l != 2 {
                    return None;
                }
                i += l;
                (oh, omv)
            };
            if i != b.len() {
                return None;
            }
            if !(0..=14).contains(&oh) || om > 59 {
                return None;
            }
            sign * (oh * 3600 + om * 60) * 1000
        }
        _ => return None,
    };
    // UTC instant on the left of the offset, local wall clock on the
    // right.
    let utc_ms = wall - offset_ms;
    let local = tz::local_offset_seconds(utc_ms.div_euclid(1000)) as i64 * 1000;
    Some(DateTime(utc_ms + local))
}

// ---------------------------------------------------------------------
// Formatting: the reference minimal form
// ---------------------------------------------------------------------

/// The minimal written form: date; plus ` HH:mm` when either is
/// nonzero; plus `:ss` when seconds are; plus `:SSS` (after a colon,
/// per the reference) when milliseconds are.
pub fn format(ms: i64) -> String {
    let (days, tod) = split(ms);
    let (y, mo, d) = civil_from_days(days);
    let hh = tod / HOUR;
    let mm = (tod % HOUR) / MINUTE;
    let ss = (tod % MINUTE) / SECOND;
    let mmm = tod % SECOND;
    let date = format!("{y:04}-{mo:02}-{d:02}");
    if mmm > 0 {
        format!("{date} {hh:02}:{mm:02}:{ss:02}:{mmm:03}")
    } else if ss > 0 {
        format!("{date} {hh:02}:{mm:02}:{ss:02}")
    } else if hh > 0 || mm > 0 {
        format!("{date} {hh:02}:{mm:02}")
    } else {
        date
    }
}

// ---------------------------------------------------------------------
// Conformance: vectors extracted from the reference library's specs
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference instant: Monday, 2026-10-12, 15:04:05 local.
    fn ref_ms() -> i64 {
        days_from_civil(2026, 10, 12) * DAY + 15 * HOUR + 4 * MINUTE + 5 * SECOND
    }

    fn p(text: &str) -> i64 {
        parse(text, ref_ms())
            .unwrap_or_else(|| panic!("expected Some: {text:?}"))
            .0
    }

    fn fails(text: &str) {
        assert!(parse(text, ref_ms()).is_none(), "expected None: {text}");
    }

    fn at(y: i64, mo: u32, d: u32, h: i64, mi: i64, s: i64) -> i64 {
        days_from_civil(y, mo, d) * DAY + h * HOUR + mi * MINUTE + s * SECOND
    }

    #[test]
    fn absolute_dates_land_on_midnight() {
        assert_eq!(p("2026-10-12"), at(2026, 10, 12, 0, 0, 0));
        assert_eq!(p("2026-10"), at(2026, 10, 1, 0, 0, 0));
        assert_eq!(p("2026"), at(2026, 1, 1, 0, 0, 0));
        assert_eq!(p("1976-11-27"), at(1976, 11, 27, 0, 0, 0));
    }

    #[test]
    fn keywords_resolve_from_midnight_and_now_keeps_time() {
        assert_eq!(p("today"), at(2026, 10, 12, 0, 0, 0));
        assert_eq!(p("tomorrow"), at(2026, 10, 13, 0, 0, 0));
        assert_eq!(p("yesterday"), at(2026, 10, 11, 0, 0, 0));
        assert_eq!(p("now"), ref_ms());
    }

    #[test]
    fn month_names_current_year_with_year_shifting_specifiers() {
        assert_eq!(p("jul 7"), at(2026, 7, 7, 0, 0, 0));
        assert_eq!(p("july 7"), at(2026, 7, 7, 0, 0, 0));
        assert_eq!(p("JuLy 7"), at(2026, 7, 7, 0, 0, 0));
        assert_eq!(p("next JuLy 7"), at(2027, 7, 7, 0, 0, 0));
        assert_eq!(p("last JuLy 7"), at(2025, 7, 7, 0, 0, 0));
        assert_eq!(p("next december"), at(2027, 12, 1, 0, 0, 0));
        assert_eq!(p("last january"), at(2025, 1, 1, 0, 0, 0));
        assert_eq!(p("may"), at(2026, 5, 1, 0, 0, 0));
    }

    #[test]
    fn weekdays_of_the_current_iso_week() {
        // The reference instant is a Monday.
        assert_eq!(p("monday"), at(2026, 10, 12, 0, 0, 0));
        assert_eq!(p("mon"), at(2026, 10, 12, 0, 0, 0));
        assert_eq!(p("next mon"), at(2026, 10, 19, 0, 0, 0));
        assert_eq!(p("last mon"), at(2026, 10, 5, 0, 0, 0));
        assert_eq!(p("friday"), at(2026, 10, 16, 0, 0, 0));
        assert_eq!(p("sunday"), at(2026, 10, 18, 0, 0, 0));
    }

    #[test]
    fn unit_forms_start_then_shift() {
        assert_eq!(p("this year"), at(2026, 1, 1, 0, 0, 0));
        assert_eq!(p("this quarter"), at(2026, 10, 1, 0, 0, 0));
        assert_eq!(p("this month"), at(2026, 10, 1, 0, 0, 0));
        assert_eq!(p("this week"), at(2026, 10, 12, 0, 0, 0));
        assert_eq!(p("this day"), at(2026, 10, 12, 0, 0, 0));
        assert_eq!(p("next month"), at(2026, 11, 1, 0, 0, 0));
        assert_eq!(p("next Month"), at(2026, 11, 1, 0, 0, 0));
        assert_eq!(p("next year"), at(2027, 1, 1, 0, 0, 0));
        assert_eq!(p("last year"), at(2025, 1, 1, 0, 0, 0));
        assert_eq!(p("last quarter"), at(2026, 7, 1, 0, 0, 0));
        assert_eq!(p("next isoweek"), at(2026, 10, 19, 0, 0, 0));
        // Bare unit words are invalid; a specifier is required.
        fails("week");
    }

    #[test]
    fn times() {
        assert_eq!(p("2:15:32"), at(2026, 10, 12, 2, 15, 32));
        assert_eq!(p("2:15:32am"), at(2026, 10, 12, 2, 15, 32));
        assert_eq!(p("2:15:32 PM"), at(2026, 10, 12, 14, 15, 32));
        assert_eq!(p("2:15"), at(2026, 10, 12, 2, 15, 0));
        assert_eq!(p("2 Am"), at(2026, 10, 12, 2, 0, 0));
        assert_eq!(p("2pm"), at(2026, 10, 12, 14, 0, 0));
        assert_eq!(p("at 2pm"), at(2026, 10, 12, 14, 0, 0));
        assert_eq!(p("at 2 pm"), at(2026, 10, 12, 14, 0, 0));
        // A bare integer is a year, never a time; the at-form demands
        // a period, and the colon form forbids at.
        assert_eq!(p("2"), at(2, 1, 1, 0, 0, 0));
        fails("at 9:00");
    }

    #[test]
    fn duration_aliases() {
        let dur = |text: &str, ms: i64| assert_eq!(p(text), ref_ms() + ms, "dur {text}");
        dur("3 ms", 3);
        dur("3 s", 3 * SECOND);
        dur("3 seconds", 3 * SECOND);
        dur("3 second", 3 * SECOND);
        dur("3 sec", 3 * SECOND);
        dur("10 minutes", 10 * MINUTE);
        dur("10 min", 10 * MINUTE);
        dur("10 m", 10 * MINUTE);
        dur("2 hours", 2 * HOUR);
        dur("2h", 2 * HOUR);
        dur("2 days", 2 * DAY);
        dur("2d", 2 * DAY);
        dur("4 weeks", 4 * WEEK);
        dur("4 w", 4 * WEEK);
        // Calendar durations move the date, preserving time of day.
        assert_eq!(format(p("3 quarters")), "2027-07-12 15:04:05");
        assert_eq!(format(p("3 y")), "2029-10-12 15:04:05");
        assert_eq!(format(p("4 o")), "2027-02-12 15:04:05");
        // Sign and spacing forms.
        assert_eq!(format(p("- 3 years")), "2023-10-12 15:04:05");
        assert_eq!(format(p("+3years")), "2029-10-12 15:04:05");
    }

    #[test]
    fn number_unit_disambiguation() {
        // "2012 seconds" is a duration, not the year 2012.
        assert_eq!(p("2012 seconds"), ref_ms() + 2012 * SECOND);
        assert_eq!(p("2012s"), ref_ms() + 2012 * SECOND);
        assert_eq!(p("2012"), at(2012, 1, 1, 0, 0, 0));
    }

    #[test]
    fn combinations_apply_in_order() {
        assert_eq!(
            p("1976-11-27 at 2pm +2s -1sec +2s"),
            at(1976, 11, 27, 14, 0, 3)
        );
        assert_eq!(
            p("1976-11-27 2pm +2s -1sec +2s"),
            at(1976, 11, 27, 14, 0, 3)
        );
        assert_eq!(p("tomorrow 9am"), at(2026, 10, 13, 9, 0, 0));
        assert_eq!(p("Nov 26 3:15 +1day"), at(2026, 11, 27, 3, 15, 0));
        assert_eq!(p("this may at 9am"), at(2026, 5, 1, 9, 0, 0));
        assert_eq!(p("this may 1 9pm"), at(2026, 5, 1, 21, 0, 0));
        assert_eq!(p("this may at 9pm -10m"), at(2026, 5, 1, 20, 50, 0));
        assert_eq!(p("2026-10-12 9am"), at(2026, 10, 12, 9, 0, 0));
    }

    #[test]
    fn iso_fallback() {
        // ISO week dates never reach the grammar.
        assert_eq!(p("2016-W51-4"), at(2016, 12, 22, 0, 0, 0));
        assert_eq!(p("2016-W51"), at(2016, 12, 19, 0, 0, 0));
        // T-separated timestamps: zoneless is local wall time.
        assert_eq!(p("2016-02-01T05:00:00"), at(2016, 2, 1, 5, 0, 0));
        // A Z timestamp is the UTC instant: the local wall form is
        // the UTC value plus the zone's offset at that moment.
        let z = p("2016-02-01T05:00:00Z");
        let zoneless = p("2016-02-01T05:00:00");
        let offset = crate::tz::local_offset_seconds(1_454_302_800);
        assert_eq!(z, zoneless + offset as i64 * 1000);
        // An explicit offset converts through to local.
        assert_eq!(p("2016-02-01T05:00:00-05:00") - z, 5 * HOUR);
    }

    #[test]
    fn formatting_is_minimal() {
        assert_eq!(format(p("this feb")), "2026-02-01");
        assert_eq!(format(p("this feb +5h")), "2026-02-01 05:00");
        assert_eq!(format(p("this feb +5h +10m")), "2026-02-01 05:10");
        assert_eq!(
            format(p("this feb +5h +10m 20ms")),
            "2026-02-01 05:10:00:020"
        );
        assert_eq!(format(p("2016-02-01")), "2016-02-01");
    }

    #[test]
    fn round_trip_through_format() {
        for text in [
            "2026-10-12",
            "this feb +5h +10m",
            "tomorrow 9am",
            "now",
            "2026-02-01 05:10:00:020",
        ] {
            let parsed = p(text);
            let canon = format(parsed);
            let again = parse(&canon, ref_ms())
                .unwrap_or_else(|| panic!("{text}: formatted form {canon} does not re-parse"))
                .0;
            assert_eq!(parsed, again, "round trip broke for {text}");
        }
    }

    #[test]
    fn negative_space() {
        fails("");
        fails("   ");
        fails("tom");
        fails("sept");
        fails("tues");
        fails("thur");
        fails("mar 3 2016");
        fails("7 july");
        fails("3 jan");
        fails("1.5h");
        fails("at 9:00");
        fails("1976 - 11");
        fails("junk");
    }

    #[test]
    fn overflow_normalizes_instead_of_erroring() {
        // The reference constructor carries overflow: apr 31 is
        // May 1 and month 13 is January of the next year.
        assert_eq!(p("2026-04-31"), at(2026, 5, 1, 0, 0, 0));
        assert_eq!(p("2026-13-01"), at(2027, 1, 1, 0, 0, 0));
    }
}
