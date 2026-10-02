use crate::model::Timeframe;
use chrono::{DateTime, Datelike, Months, TimeZone, Timelike, Utc, Weekday};
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

/// Scheduled exclusive close relative to the observed bar open (not an epoch-aligned boundary).
/// Monthly bars retain the UTC day and time, clamping the day to the last day of
/// the next month when it does not exist (e.g. January 31 → February 28/29).
/// Panics if the open or its scheduled close is outside the supported timestamp range.
pub fn bar_scheduled_close_ms(bar_open_ms: i64, tf: Timeframe) -> i64 {
    if tf == Timeframe::MOS1 {
        let open = Utc
            .timestamp_millis_opt(bar_open_ms)
            .single()
            .expect("monthly bar open outside UTC range");
        let (year, month) = if open.month() == 12 {
            (
                open.year().checked_add(1).expect("monthly year overflow"),
                1,
            )
        } else {
            (open.year(), open.month() + 1)
        };
        let next_month_start = Utc
            .with_ymd_and_hms(year, month, 1, 0, 0, 0)
            .single()
            .expect("next month outside UTC range");
        let last_day = next_month_start
            .checked_add_months(Months::new(1))
            .and_then(|start| start.checked_sub_signed(chrono::Duration::days(1)))
            .expect("monthly close outside UTC range")
            .day();
        return Utc
            .with_ymd_and_hms(
                year,
                month,
                open.day().min(last_day),
                open.hour(),
                open.minute(),
                open.second(),
            )
            .single()
            .and_then(|close| close.with_nanosecond(open.nanosecond()))
            .expect("monthly close outside UTC range")
            .timestamp_millis();
    }
    let period_ms: i64 = match tf {
        Timeframe::M1 => 60_000,
        Timeframe::M5 => 300_000,
        Timeframe::M15 => 900_000,
        Timeframe::M30 => 1_800_000,
        Timeframe::H1 => 3_600_000,
        Timeframe::H2 => 7_200_000,
        Timeframe::H4 => 14_400_000,
        Timeframe::H6 => 21_600_000,
        Timeframe::H8 => 28_800_000,
        Timeframe::H12 => 43_200_000,
        Timeframe::D1 => 86_400_000,
        Timeframe::D3 => 259_200_000,
        Timeframe::W1 => 7 * 86_400_000,
        Timeframe::MOS1 => unreachable!(),
    };
    bar_open_ms
        .checked_add(period_ms)
        .expect("scheduled bar close overflow")
}

/// Projects 10 bars for M1–D3, 5 for W1, and 2 for MOS1.
/// The first bar opens strictly after the observed bar; successive half-open bars
/// are contiguous (each close equals the next open) and advance relative to the
/// observed open, never a calendar grid. Uses only the injected `as_of`, not a clock.
/// Returns no bars when the full window has elapsed, or a shorter window if a
/// scheduled timestamp cannot be represented.
pub fn iching_forecast_horizon(
    last_observed_open_ms: i64,
    tf: Timeframe,
    as_of: DateTime<Utc>,
) -> Vec<(i64, i64)> {
    let count = match tf {
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
        | Timeframe::D3 => 10,
        Timeframe::W1 => 5,
        Timeframe::MOS1 => 2,
    };
    let mut bars = Vec::with_capacity(count);
    let mut open = last_observed_open_ms;
    for index in 0..=count {
        // The shared close helper panics outside its range, so check before calling it.
        if tf == Timeframe::MOS1 {
            if !Utc
                .timestamp_millis_opt(open)
                .single()
                .is_some_and(|date| date.checked_add_months(Months::new(2)).is_some())
            {
                break;
            }
        } else if (tf.weight() as i64)
            .checked_mul(60_000)
            .and_then(|period_ms| open.checked_add(period_ms))
            .is_none()
        {
            break;
        }
        let close = bar_scheduled_close_ms(open, tf);
        if close <= open {
            break;
        }
        if index > 0 {
            bars.push((open, close));
        }
        open = close;
    }
    if bars.len() == count
        && bars
            .last()
            .is_some_and(|bar| bar.1 <= as_of.timestamp_millis())
    {
        Vec::new()
    } else {
        bars
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
    fn horizon_starts_after_forming_bar() {
        let anchor = Utc
            .with_ymd_and_hms(2025, 1, 31, 12, 0, 0)
            .unwrap()
            .timestamp_millis();
        let as_of = Utc.with_ymd_and_hms(2025, 1, 31, 12, 0, 0).unwrap();
        for tf in [Timeframe::M1, Timeframe::D3, Timeframe::W1, Timeframe::MOS1] {
            let bars = iching_forecast_horizon(anchor, tf, as_of);
            assert_eq!(bars[0].0, bar_scheduled_close_ms(anchor, tf), "{tf}");
            assert!(bars[0].0 > anchor, "{tf}");
        }
    }

    #[test]
    fn horizon_counts_all_tf_classes() {
        let anchor = Utc
            .with_ymd_and_hms(2025, 7, 10, 12, 0, 0)
            .unwrap()
            .timestamp_millis();
        let as_of = Utc.with_ymd_and_hms(2025, 7, 10, 12, 0, 0).unwrap();
        assert_eq!(Timeframe::ALL_CANONICAL.len(), 14);
        for label in Timeframe::ALL_CANONICAL {
            let tf: Timeframe = label.parse().unwrap();
            let expected = match tf {
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
                | Timeframe::D3 => 10,
                Timeframe::W1 => 5,
                Timeframe::MOS1 => 2,
            };
            assert_eq!(
                iching_forecast_horizon(anchor, tf, as_of).len(),
                expected,
                "{tf}"
            );
        }
    }

    #[test]
    fn horizon_uses_observed_weekly_grid() {
        let anchor = Utc
            .with_ymd_and_hms(2025, 7, 10, 12, 34, 0)
            .unwrap()
            .timestamp_millis();
        let as_of = Utc.with_ymd_and_hms(2025, 7, 10, 12, 34, 0).unwrap();
        let bars = iching_forecast_horizon(anchor, Timeframe::W1, as_of);
        assert_eq!(bars.len(), 5);
        for (index, (open, _)) in bars.iter().enumerate() {
            assert_eq!(*open, anchor + 7 * 86_400_000 * (index as i64 + 1));
            assert_eq!(
                Utc.timestamp_millis_opt(*open).unwrap().weekday(),
                Weekday::Thu
            );
        }
    }

    #[test]
    fn horizon_handles_short_month_and_year() {
        for ((year, month, day), expected) in [
            (
                (2025, 12, 15),
                [(2026, 1, 15), (2026, 2, 15), (2026, 3, 15)],
            ),
            ((2024, 1, 28), [(2024, 2, 28), (2024, 3, 28), (2024, 4, 28)]),
            ((2025, 1, 28), [(2025, 2, 28), (2025, 3, 28), (2025, 4, 28)]),
            ((2024, 2, 29), [(2024, 3, 29), (2024, 4, 29), (2024, 5, 29)]),
            ((2025, 2, 28), [(2025, 3, 28), (2025, 4, 28), (2025, 5, 28)]),
        ] {
            let anchor = Utc.with_ymd_and_hms(year, month, day, 10, 11, 12).unwrap();
            let bars = iching_forecast_horizon(anchor.timestamp_millis(), Timeframe::MOS1, anchor);
            assert_eq!(bars.len(), 2);
            for (index, (open, close)) in bars.iter().enumerate() {
                let expected_open = Utc
                    .with_ymd_and_hms(
                        expected[index].0,
                        expected[index].1,
                        expected[index].2,
                        10,
                        11,
                        12,
                    )
                    .unwrap()
                    .timestamp_millis();
                let expected_close = Utc
                    .with_ymd_and_hms(
                        expected[index + 1].0,
                        expected[index + 1].1,
                        expected[index + 1].2,
                        10,
                        11,
                        12,
                    )
                    .unwrap()
                    .timestamp_millis();
                assert_eq!((*open, *close), (expected_open, expected_close));
                assert_eq!(*close, bar_scheduled_close_ms(*open, Timeframe::MOS1));
            }
            assert!(bars[0].0 < bars[1].0);
        }
    }

    #[test]
    fn horizon_preserves_month_anchor_without_duplication() {
        let mut anchor = Utc
            .with_ymd_and_hms(2025, 1, 31, 12, 0, 0)
            .unwrap()
            .timestamp_millis();
        let as_of = Utc.with_ymd_and_hms(2025, 1, 31, 12, 0, 0).unwrap();
        let mut opens = Vec::new();
        for _ in 0..6 {
            let bars = iching_forecast_horizon(anchor, Timeframe::MOS1, as_of);
            assert_eq!(bars.len(), 2);
            opens.extend(bars.iter().map(|bar| bar.0));
            anchor = bars[1].0;
        }
        assert_eq!(opens.len(), 12);
        assert_eq!(
            opens[0],
            Utc.with_ymd_and_hms(2025, 2, 28, 12, 0, 0)
                .unwrap()
                .timestamp_millis()
        );
        assert_eq!(
            opens[1],
            Utc.with_ymd_and_hms(2025, 3, 28, 12, 0, 0)
                .unwrap()
                .timestamp_millis()
        );
        assert!(opens.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn horizon_rejects_stale_or_overflowing_anchor() {
        let old = Utc
            .with_ymd_and_hms(2020, 1, 1, 0, 0, 0)
            .unwrap()
            .timestamp_millis();
        let as_of = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        assert!(iching_forecast_horizon(old, Timeframe::M1, as_of).is_empty());
        assert!(iching_forecast_horizon(i64::MAX - 30_000, Timeframe::M1, as_of).is_empty());
        assert!(iching_forecast_horizon(i64::MAX, Timeframe::MOS1, as_of).is_empty());
        assert_eq!(
            iching_forecast_horizon(i64::MAX - 150_000, Timeframe::M1, as_of),
            vec![(i64::MAX - 90_000, i64::MAX - 30_000)]
        );

        let forming = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        let full_window_close = forming + chrono::Duration::minutes(11);
        assert!(
            iching_forecast_horizon(forming.timestamp_millis(), Timeframe::M1, full_window_close)
                .is_empty()
        );
        let partially_elapsed = forming + chrono::Duration::minutes(2);
        assert_eq!(
            iching_forecast_horizon(forming.timestamp_millis(), Timeframe::M1, partially_elapsed)
                .len(),
            10
        );
    }

    #[test]
    fn horizon_is_deterministic_for_fixed_as_of() {
        let as_of = Utc.with_ymd_and_hms(2025, 7, 10, 12, 0, 0).unwrap();
        let anchor = as_of.timestamp_millis();
        assert_eq!(
            iching_forecast_horizon(anchor, Timeframe::H4, as_of),
            iching_forecast_horizon(anchor, Timeframe::H4, as_of)
        );
    }

    #[test]
    fn horizon_bars_are_contiguous_and_half_open() {
        let as_of = Utc.with_ymd_and_hms(2025, 1, 31, 12, 0, 0).unwrap();
        let anchor = as_of.timestamp_millis();
        assert_eq!(Timeframe::ALL_CANONICAL.len(), 14);
        for label in Timeframe::ALL_CANONICAL {
            let tf: Timeframe = label.parse().unwrap();
            let bars = iching_forecast_horizon(anchor, tf, as_of);
            assert!(!bars.is_empty(), "{tf}");
            assert_eq!(bars[0].0, bar_scheduled_close_ms(anchor, tf), "{tf}");
            for pair in &bars {
                assert!(pair.1 > pair.0, "{tf}");
            }
            for adjacent in bars.windows(2) {
                assert_eq!(adjacent[1].0, adjacent[0].1, "{tf}");
                assert!(adjacent[1].0 > adjacent[0].0, "{tf}");
            }
        }
    }

    #[test]
    fn scheduled_close_preserves_observed_weekly_open() {
        let open = Utc
            .with_ymd_and_hms(2025, 7, 9, 12, 34, 0)
            .unwrap()
            .timestamp_millis();
        assert_eq!(
            bar_scheduled_close_ms(open, Timeframe::W1),
            open + 7 * 86_400_000
        );
    }

    #[test]
    fn scheduled_close_advances_month_across_february_and_year() {
        for ((year, month, day), (next_year, next_month, next_day)) in [
            ((2025, 1, 15), (2025, 2, 15)),
            ((2025, 12, 15), (2026, 1, 15)),
            ((2024, 2, 15), (2024, 3, 15)),
            ((2025, 2, 15), (2025, 3, 15)),
        ] {
            let open = Utc
                .with_ymd_and_hms(year, month, day, 10, 11, 12)
                .unwrap()
                .timestamp_millis();
            let close = Utc
                .with_ymd_and_hms(next_year, next_month, next_day, 10, 11, 12)
                .unwrap()
                .timestamp_millis();
            assert_eq!(bar_scheduled_close_ms(open, Timeframe::MOS1), close);
        }
    }

    #[test]
    fn scheduled_close_clamps_31st_to_last_day_of_next_month() {
        for (year, day) in [(2024, 29), (2025, 28)] {
            let open = Utc
                .with_ymd_and_hms(year, 1, 31, 23, 0, 0)
                .unwrap()
                .timestamp_millis();
            let close = Utc
                .with_ymd_and_hms(year, 2, day, 23, 0, 0)
                .unwrap()
                .timestamp_millis();
            assert_eq!(bar_scheduled_close_ms(open, Timeframe::MOS1), close);
        }
    }

    #[test]
    fn scheduled_close_uses_exact_fixed_periods() {
        let open = 1_700_000_000_123;
        for (tf, period) in [
            (Timeframe::M1, 60_000),
            (Timeframe::M5, 300_000),
            (Timeframe::M15, 900_000),
            (Timeframe::M30, 1_800_000),
            (Timeframe::H1, 3_600_000),
            (Timeframe::H2, 7_200_000),
            (Timeframe::H4, 14_400_000),
            (Timeframe::H6, 21_600_000),
            (Timeframe::H8, 28_800_000),
            (Timeframe::H12, 43_200_000),
            (Timeframe::D1, 86_400_000),
            (Timeframe::D3, 259_200_000),
            (Timeframe::W1, 604_800_000),
        ] {
            assert_eq!(bar_scheduled_close_ms(open, tf), open + period);
        }
    }

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
