//! The local timezone offset, read from `/etc/localtime` (TZif).
//!
//! Keyword dates ("today") and zoneless parsing resolve on the local
//! wall clock, so the engine needs today's real offset. This reads
//! the system TZif database directly (v1 data as a fallback, v2/v3
//! 64-bit tables when present) and returns the UTC offset in effect
//! at a given UTC instant. No `TZ` string forms beyond `UTC` are
//! interpreted; anything unreadable falls back to UTC.

use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds on the local wall-clock timeline for "right now":
/// the UTC instant plus the zone's current offset.
pub fn now_millis() -> i64 {
    let utc = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    utc + local_offset_seconds(utc.div_euclid(1000)) as i64 * 1000
}

/// The zone's offset in seconds at a UTC instant.
pub fn local_offset_seconds(at_utc: i64) -> i32 {
    let Ok(data) = std::fs::read("/etc/localtime") else {
        return 0;
    };
    parse_tzif_offset(&data, at_utc).unwrap_or(0)
}

fn parse_tzif_offset(data: &[u8], at_utc: i64) -> Option<i32> {
    if data.len() < 44 || &data[0..4] != b"TZif" {
        return None;
    }
    let version = data[4];
    let be32 = |b: &[u8]| i32::from_be_bytes([b[0], b[1], b[2], b[3]]);
    let counts = |h: &[u8]| -> (usize, usize, usize, usize, usize) {
        (
            be32(&h[20..24]) as usize,
            be32(&h[24..28]) as usize,
            be32(&h[28..32]) as usize,
            be32(&h[32..36]) as usize,
            be32(&h[36..40]) as usize,
        )
    };

    // v1 block layout: header(44) + transitions*4 + indices + types*6
    // + abbreviations. Version 2+ repeats the whole block with 64-bit
    // transitions and appends a footer; the v2 block is the accurate
    // one, so skip past v1 when it exists.
    let (isutcnt, isstdcnt, leapcnt, timecnt, typecnt) = counts(data);
    let v1_len = 44
        + timecnt * 4
        + timecnt
        + typecnt * 6
        + charcnt_of(data, timecnt, typecnt)
        + leapcnt * 8
        + isstdcnt
        + isutcnt;
    if version >= b'2' && data.len() > v1_len + 44 {
        let rest = &data[v1_len..];
        if rest.len() >= 44 && &rest[0..4] == b"TZif" {
            let (isutcnt, isstdcnt, leapcnt, timecnt, typecnt) = counts(rest);
            return offset_from_block(
                &rest[44..],
                timecnt,
                typecnt,
                leapcnt,
                isstdcnt,
                isutcnt,
                8,
                at_utc,
            );
        }
    }
    offset_from_block(
        &data[44..],
        timecnt,
        typecnt,
        leapcnt_of(data),
        isstdcnt,
        isutcnt,
        4,
        at_utc,
    )
}

fn charcnt_of(data: &[u8], _timecnt: usize, _typecnt: usize) -> usize {
    i32::from_be_bytes([data[40], data[41], data[42], data[43]]) as usize
}

fn leapcnt_of(data: &[u8]) -> usize {
    i32::from_be_bytes([data[28], data[29], data[30], data[31]]) as usize
}

#[allow(clippy::too_many_arguments)]
fn offset_from_block(
    block: &[u8],
    timecnt: usize,
    typecnt: usize,
    leapcnt: usize,
    isstdcnt: usize,
    isutcnt: usize,
    time_size: usize,
    at_utc: i64,
) -> Option<i32> {
    let mut pos = 0;
    let mut transitions = Vec::with_capacity(timecnt);
    for _ in 0..timecnt {
        let raw = block.get(pos..pos + time_size)?;
        let t = if time_size == 8 {
            i64::from_be_bytes([
                raw[0], raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7],
            ])
        } else {
            i32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]) as i64
        };
        transitions.push(t);
        pos += time_size;
    }
    let indices = block.get(pos..pos + timecnt)?;
    pos += timecnt;
    // Type records: gmtoff(4) isdst(1) abbrind(1).
    let mut offsets = Vec::with_capacity(typecnt);
    let mut dst_flags = Vec::with_capacity(typecnt);
    for t in 0..typecnt {
        let rec = block.get(pos + t * 6..pos + t * 6 + 6)?;
        offsets.push(i32::from_be_bytes([rec[0], rec[1], rec[2], rec[3]]));
        dst_flags.push(rec[4] != 0);
    }
    // The rest (abbreviations, leap seconds, std/ut markers) is past
    // what we need; sizes are only used for locating the v2 block.

    let _ = (leapcnt, isstdcnt, isutcnt);
    // First standard (non-DST) type is the pre-first-transition rule.
    let default_type = dst_flags.iter().position(|d| !d).unwrap_or(0);
    let chosen = match transitions.iter().position(|&t| t > at_utc) {
        Some(0) | None => {
            let last = transitions.len();
            if last == 0 {
                default_type
            } else {
                indices[last - 1] as usize
            }
        }
        Some(first_after) => indices[first_after - 1] as usize,
    };
    offsets.get(chosen).copied()
}

#[cfg(test)]
mod tests {
    use super::{local_offset_seconds, now_millis};

    #[test]
    fn the_local_zone_resolves_to_something_sane() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let offset = local_offset_seconds(now);
        // Every real zone is within ±14h.
        assert!(offset.abs() <= 14 * 3600, "offset {offset}");
        // The offset must be a whole-minute form.
        assert_eq!(offset % 60, 0, "offset {offset}");
    }

    #[test]
    fn now_millis_is_near_the_utc_clock() {
        let utc = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let local = now_millis();
        let delta = (local - utc).abs();
        assert!(delta <= 14 * 3600 * 1000, "delta {delta}");
    }
}
