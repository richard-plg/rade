//! Calendar dates from Unix time without a date crate: paste file names
//! (R5).

use std::time::{SystemTime, UNIX_EPOCH};

/// `(year, month, day, hour, minute, second)` for `secs` since the epoch
/// (Howard Hinnant's `civil_from_days`). Add [`local_offset`] first for
/// local time.
pub fn civil(secs: i64) -> (i64, i64, i64, i64, i64, i64) {
    let (days, sod) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d, sod / 3600, sod % 3600 / 60, sod % 60)
}

/// The local time zone's offset from UTC in seconds at `secs`.
pub fn local_offset(secs: i64) -> i64 {
    unsafe {
        let t = secs as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            0
        } else {
            tm.tm_gmtoff
        }
    }
}

/// Seconds since the epoch, shifted to local time.
pub fn local_secs(now: SystemTime) -> i64 {
    let secs = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    secs + local_offset(secs)
}

/// Days since the epoch of a `YYYY-MM-DD…` prefix (inverse of [`civil`]).
pub fn days_from_civil(s: &str) -> Option<i64> {
    let (y, m, d) = (
        s.get(0..4)?.parse::<i64>().ok()?,
        s.get(5..7)?.parse::<i64>().ok()?,
        s.get(8..10)?.parse::<i64>().ok()?,
    );
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_round_trips_known_dates() {
        assert_eq!(civil(0), (1970, 1, 1, 0, 0, 0));
        // 2026-10-02 09:10:05 UTC
        let t = days_from_civil("2026-10-02").unwrap() * 86_400 + 9 * 3600 + 10 * 60 + 5;
        assert_eq!(civil(t), (2026, 10, 2, 9, 10, 5));
        assert_eq!(days_from_civil("2000-03-01"), Some(11_017));
        assert_eq!(days_from_civil("nope"), None);
    }
}
