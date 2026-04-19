use crate::model::Timeframe;
use chrono::{DateTime, Datelike, TimeZone, Timelike, Utc, Weekday};
use core::time::Duration;

/// Check if the time now is multiple of period, with optional tolerance
pub fn is_time_multiple_of_period(
    period: Duration,
    now: DateTime<Utc>,
    tolerance: Option<Duration>,
) -> bool {
    // Calculate the total seconds from the start of the day
    let total_seconds_since_midnight = now.timestamp() as u64;
    let tolerance_seconds = tolerance.map(|t| t.as_secs()).unwrap_or(0);

    // Convert the period to seconds
    let period_seconds = period.as_secs();

    // Check if the total seconds is a multiple of the period seconds
    if period_seconds == 0 {
        return false; // Avoid division by zero
    }

    let remainder = total_seconds_since_midnight % period_seconds;

    // Check if it's exactly a multiple or just after a multiple within tolerance
    if remainder <= tolerance_seconds {
        return true;
    }

    // Check if it's just before a multiple within tolerance
    if (period_seconds - remainder) <= tolerance_seconds {
        return true;
    }

    false
}

/// Check if the time now is the closing time of timeframe, within an optional tolerance
/// Valid tf values: 1m, 5m, 15m, 30m, 1h, 4h, 6h, 8h, 12h, 1d, 3d, 1w, 1M
pub fn is_closing_timeframe(
    tf: &Timeframe,
    now: DateTime<Utc>,
    tolerance: Option<Duration>,
) -> Result<bool, String> {
    let tolerance_seconds = tolerance.map(|t| t.as_secs()).unwrap_or(0);
    let seconds_in_day = 86_400;

    match tf {
        Timeframe::M1 => Ok(is_time_multiple_of_period(
            Duration::from_secs(60),
            now,
            tolerance,
        )),
        Timeframe::M5 => Ok(is_time_multiple_of_period(
            Duration::from_secs(120),
            now,
            tolerance,
        )),
        Timeframe::M15 => Ok(is_time_multiple_of_period(
            Duration::from_secs(900),
            now,
            tolerance,
        )),
        Timeframe::M30 => Ok(is_time_multiple_of_period(
            Duration::from_secs(1_800),
            now,
            tolerance,
        )),
        Timeframe::H1 => Ok(is_time_multiple_of_period(
            Duration::from_secs(3_600),
            now,
            tolerance,
        )),
        Timeframe::H2 => Ok(is_time_multiple_of_period(
            Duration::from_secs(7_200),
            now,
            tolerance,
        )),
        Timeframe::H4 => Ok(is_time_multiple_of_period(
            Duration::from_secs(14_400),
            now,
            tolerance,
        )),
        Timeframe::H6 => Ok(is_time_multiple_of_period(
            Duration::from_secs(21_600),
            now,
            tolerance,
        )),
        Timeframe::H8 => Ok(is_time_multiple_of_period(
            Duration::from_secs(28_800),
            now,
            tolerance,
        )),
        Timeframe::H12 => Ok(is_time_multiple_of_period(
            Duration::from_secs(43_200),
            now,
            tolerance,
        )),
        Timeframe::D1 => {
            let seconds_from_midnight = now.num_seconds_from_midnight() as u64;
            Ok(seconds_from_midnight <= tolerance_seconds
                || (seconds_in_day - seconds_from_midnight) <= tolerance_seconds)
        }
        Timeframe::D3 => Ok(is_time_multiple_of_period(
            Duration::from_secs(259_200),
            now,
            tolerance,
        )),
        Timeframe::W1 => {
            if tolerance_seconds > seconds_in_day {
                Err(format!(
                    "Tolerance too big, must be less than a day: {tolerance:#?}"
                ))
            } else {
                let seconds_from_midnight = now.num_seconds_from_midnight() as u64;
                let is_monday_start = now.weekday() == Weekday::Mon
                    && (seconds_from_midnight <= tolerance_seconds
                        || (seconds_in_day - seconds_from_midnight) <= tolerance_seconds);
                let is_sunday_end = now.weekday() == Weekday::Sun
                    && (seconds_from_midnight >= seconds_in_day - tolerance_seconds
                        || seconds_from_midnight <= tolerance_seconds);
                Ok(is_monday_start || is_sunday_end)
            }
        }
        Timeframe::MOS1 => {
            if tolerance_seconds > seconds_in_day {
                Err(format!(
                    "Tolerance too big, must be less than a day: {tolerance:#?}"
                ))
            } else {
                let seconds_from_midnight = now.num_seconds_from_midnight() as u64;
                let is_first_day_start = now.day() == 1
                    && (seconds_from_midnight <= tolerance_seconds
                        || (seconds_in_day - seconds_from_midnight) <= tolerance_seconds);
                let is_last_day_end = (now + Duration::from_secs(86_400)).day() == 1
                    && (seconds_from_midnight >= seconds_in_day - tolerance_seconds
                        || seconds_from_midnight <= tolerance_seconds);
                Ok(is_first_day_start || is_last_day_end)
            }
        }
    }
}

/// Calculate seconds from `now` until the next candle close for the given timeframe.
///
/// Candle close = the start of the next period (e.g. for 4h: 00:00, 04:00, 08:00 UTC...).
/// Returns 0 if exactly on a boundary.
pub fn seconds_until_next_close(tf: &Timeframe, now: DateTime<Utc>) -> u64 {
    let ts = now.timestamp() as u64;

    match tf {
        // Fixed-period TFs: close at multiples of period_secs from epoch
        Timeframe::M1
        | Timeframe::M5
        | Timeframe::M15
        | Timeframe::M30
        | Timeframe::H1
        | Timeframe::H2
        | Timeframe::H4
        | Timeframe::H6
        | Timeframe::H8
        | Timeframe::H12
        | Timeframe::D1
        | Timeframe::D3 => {
            let period_secs = match tf {
                Timeframe::M1 => 60,
                Timeframe::M5 => 300,
                Timeframe::M15 => 900,
                Timeframe::M30 => 1_800,
                Timeframe::H1 => 3_600,
                Timeframe::H2 => 7_200,
                Timeframe::H4 => 14_400,
                Timeframe::H6 => 21_600,
                Timeframe::H8 => 28_800,
                Timeframe::H12 => 43_200,
                Timeframe::D1 => 86_400,
                Timeframe::D3 => 259_200,
                _ => unreachable!(),
            };
            let remainder = ts % period_secs;
            if remainder == 0 {
                0
            } else {
                period_secs - remainder
            }
        }
        // Weekly: closes at Monday 00:00:00 UTC
        Timeframe::W1 => {
            let weekday = now.weekday().num_days_from_monday(); // Mon=0, Sun=6
            let secs_from_midnight = now.num_seconds_from_midnight() as u64;
            let days_until_monday = if weekday == 0 && secs_from_midnight == 0 {
                0 // already at boundary
            } else {
                (7 - weekday as u64) % 7
            };
            if days_until_monday == 0 && secs_from_midnight == 0 {
                0
            } else if weekday == 0 && secs_from_midnight > 0 {
                // Monday but past midnight — next Monday
                7 * 86_400 - secs_from_midnight
            } else {
                days_until_monday * 86_400 - secs_from_midnight
            }
        }
        // Monthly: closes at 1st of next month 00:00:00 UTC
        Timeframe::MOS1 => {
            let year = now.year();
            let month = now.month();
            let (next_year, next_month) = if month == 12 {
                (year + 1, 1)
            } else {
                (year, month + 1)
            };
            let next_month_start = Utc
                .with_ymd_and_hms(next_year, next_month, 1, 0, 0, 0)
                .unwrap();
            let diff = next_month_start.signed_duration_since(now);
            if diff.num_seconds() <= 0 {
                0
            } else {
                diff.num_seconds() as u64
            }
        }
    }
}

/// Find the minimum seconds until next candle close across a set of timeframes.
/// Returns (secs_until_close, which_tf).
pub fn next_close_across_tfs(tfs: &[Timeframe], now: DateTime<Utc>) -> Option<(u64, Timeframe)> {
    tfs.iter()
        .map(|tf| (seconds_until_next_close(tf, now), *tf))
        .filter(|(secs, _)| *secs > 0) // skip TFs at exact boundary
        .min_by_key(|(secs, _)| *secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn test_is_time_multiple_of_period_no_tolerance() {
        let period = Duration::from_secs(5 * 60);
        let now = Utc.with_ymd_and_hms(2025, 7, 6, 10, 0, 0).unwrap();
        assert!(is_time_multiple_of_period(period, now, None));

        let now = Utc.with_ymd_and_hms(2025, 7, 6, 10, 5, 0).unwrap();
        assert!(is_time_multiple_of_period(period, now, None));

        let now = Utc.with_ymd_and_hms(2025, 7, 6, 10, 1, 0).unwrap();
        assert!(!is_time_multiple_of_period(period, now, None));
    }

    #[test]
    fn test_is_time_multiple_of_period_with_tolerance() {
        let period = Duration::from_secs(5 * 60);
        let tolerance = Some(Duration::from_secs(30));

        // Just after a multiple
        let now = Utc.with_ymd_and_hms(2025, 7, 6, 10, 0, 15).unwrap();
        assert!(is_time_multiple_of_period(period, now, tolerance));

        // Just before a multiple
        let now = Utc.with_ymd_and_hms(2025, 7, 6, 10, 4, 45).unwrap();
        assert!(is_time_multiple_of_period(period, now, tolerance));

        // Outside tolerance
        let now = Utc.with_ymd_and_hms(2025, 7, 6, 10, 1, 31).unwrap();
        assert!(!is_time_multiple_of_period(period, now, tolerance));
    }

    #[test]
    fn test_is_time_multiple_of_period_zero_period() {
        let period = Duration::from_secs(0);
        let now = Utc.with_ymd_and_hms(2025, 7, 6, 10, 0, 0).unwrap();
        assert!(!is_time_multiple_of_period(period, now, None));
    }

    #[test]
    fn test_is_closing_timeframe_m1() {
        let tf = Timeframe::M1;
        let now = Utc.with_ymd_and_hms(2025, 7, 6, 10, 0, 0).unwrap();
        assert!(is_closing_timeframe(&tf, now, None).unwrap());

        let now = Utc.with_ymd_and_hms(2025, 7, 6, 10, 0, 30).unwrap();
        assert!(!is_closing_timeframe(&tf, now, None).unwrap());

        let tolerance = Some(Duration::from_secs(30));
        let now = Utc.with_ymd_and_hms(2025, 7, 6, 10, 0, 15).unwrap();
        assert!(is_closing_timeframe(&tf, now, tolerance).unwrap());
    }

    #[test]
    fn test_is_closing_timeframe_h1() {
        let tf = Timeframe::H1;
        let now = Utc.with_ymd_and_hms(2025, 7, 6, 10, 0, 0).unwrap();
        assert!(is_closing_timeframe(&tf, now, None).unwrap());

        let now = Utc.with_ymd_and_hms(2025, 7, 6, 10, 30, 0).unwrap();
        assert!(!is_closing_timeframe(&tf, now, None).unwrap());

        let tolerance = Some(Duration::from_secs(15 * 60));
        let now = Utc.with_ymd_and_hms(2025, 7, 6, 10, 59, 0).unwrap();
        assert!(is_closing_timeframe(&tf, now, tolerance).unwrap());
    }

    #[test]
    fn test_is_closing_timeframe_d1() {
        let tf = Timeframe::D1;
        let now = Utc.with_ymd_and_hms(2025, 7, 6, 0, 0, 0).unwrap();
        assert!(is_closing_timeframe(&tf, now, None).unwrap());

        let now = Utc.with_ymd_and_hms(2025, 7, 6, 12, 0, 0).unwrap();
        assert!(!is_closing_timeframe(&tf, now, None).unwrap());

        let tolerance = Some(Duration::from_secs(3600));
        let now = Utc.with_ymd_and_hms(2025, 7, 6, 23, 50, 0).unwrap();
        assert!(is_closing_timeframe(&tf, now, tolerance).unwrap());

        let now = Utc.with_ymd_and_hms(2025, 7, 6, 0, 0, 30).unwrap();
        assert!(is_closing_timeframe(&tf, now, Some(Duration::from_secs(60))).unwrap());
    }

    #[test]
    fn test_is_closing_timeframe_w1() {
        let tf = Timeframe::W1;
        // Monday 00:00:00 UTC
        let now = Utc.with_ymd_and_hms(2025, 7, 7, 0, 0, 0).unwrap(); // Monday
        assert!(is_closing_timeframe(&tf, now, None).unwrap());

        // Sunday 23:59:59 UTC (just before Monday)
        let now = Utc.with_ymd_and_hms(2025, 7, 6, 23, 59, 59).unwrap(); // Sunday
        assert!(is_closing_timeframe(&tf, now, Some(Duration::from_secs(1))).unwrap());

        // Tuesday
        let now = Utc.with_ymd_and_hms(2025, 7, 8, 0, 0, 0).unwrap(); // Tuesday
        assert!(!is_closing_timeframe(&tf, now, None).unwrap());

        // Tolerance too big
        let tolerance = Some(Duration::from_secs(2 * 24 * 3600));
        let now = Utc.with_ymd_and_hms(2025, 7, 7, 0, 0, 0).unwrap();
        assert!(is_closing_timeframe(&tf, now, tolerance).is_err());

        // Monday with tolerance
        let now = Utc.with_ymd_and_hms(2025, 7, 7, 0, 0, 30).unwrap();
        assert!(is_closing_timeframe(&tf, now, Some(Duration::from_secs(60))).unwrap());

        // Sunday with tolerance
        let now = Utc.with_ymd_and_hms(2025, 7, 6, 23, 59, 30).unwrap();
        assert!(is_closing_timeframe(&tf, now, Some(Duration::from_secs(60))).unwrap());
    }

    #[test]
    fn test_is_closing_timeframe_mos1() {
        let tf = Timeframe::MOS1;
        // First day of month 00:00:00 UTC
        let now = Utc.with_ymd_and_hms(2025, 7, 1, 0, 0, 0).unwrap();
        assert!(is_closing_timeframe(&tf, now, None).unwrap());

        // Last day of month 23:59:59 UTC (just before first day of next month)
        let now = Utc.with_ymd_and_hms(2025, 7, 31, 23, 59, 59).unwrap();
        assert!(is_closing_timeframe(&tf, now, Some(Duration::from_secs(60))).unwrap());

        // Middle of month
        let now = Utc.with_ymd_and_hms(2025, 7, 15, 0, 0, 0).unwrap();
        assert!(!is_closing_timeframe(&tf, now, None).unwrap());

        // Tolerance too big
        let tolerance = Some(Duration::from_secs(2 * 24 * 3600));
        let now = Utc.with_ymd_and_hms(2025, 7, 1, 0, 0, 0).unwrap();
        assert!(is_closing_timeframe(&tf, now, tolerance).is_err());

        // First day of month with tolerance
        let now = Utc.with_ymd_and_hms(2025, 7, 1, 0, 0, 30).unwrap();
        assert!(is_closing_timeframe(&tf, now, Some(Duration::from_secs(60))).unwrap());

        // Last day of month with tolerance
        let now = Utc.with_ymd_and_hms(2025, 7, 31, 23, 59, 30).unwrap();
        assert!(is_closing_timeframe(&tf, now, Some(Duration::from_secs(60))).unwrap());
    }
}
