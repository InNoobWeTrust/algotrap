//! I-Ching cast trajectories materialized over market-bar time ranges.

use super::{LeapMonthPolicy, calendar, plum_blossom_signal_with_policy};
use crate::ta::{TaError, TaResult};
use chrono::Timelike;

const HOUR_MS: i64 = 60 * 60 * 1_000;

/// I-Ching original-channel OHLC and terminal moving-line data for one market bar.
#[derive(Debug, Clone, PartialEq)]
pub struct IchingBarTrajectory {
    pub energy_open: f64,
    pub energy_high: f64,
    pub energy_low: f64,
    pub energy_close: f64,
    /// Transformed-channel energy derived from the opening cast.
    pub transformed_open: f64,
    /// Transformed-channel energy derived from the terminal cast inside the bar.
    pub transformed_close: f64,
    /// Nuclear-channel energy derived from the opening cast.
    pub nuclear_open: f64,
    /// Nuclear-channel energy derived from the terminal cast inside the bar.
    pub nuclear_close: f64,
    pub moving_line: Option<u8>,
}

/// Computes the Plum Blossom cast trajectory contained within one market bar.
///
/// The bar owns the right-half-open interval `[open, close)`. The opening cast
/// is always included. Additional casts are evaluated only at instants where
/// the documented `cst-utc8-fixed-v1` input tuple can change:
///
/// - CST midnight (the local date/lunar day changes), and
/// - odd CST hours (the two-hour Earthly Branch changes).
///
/// The exact close boundary is excluded so it belongs only to the next bar.
pub fn iching_bar_trajectory(
    bar_open_time_ms: i64,
    bar_close_time_ms: i64,
) -> TaResult<IchingBarTrajectory> {
    let duration_ms = bar_close_time_ms
        .checked_sub(bar_open_time_ms)
        .ok_or_else(|| TaError::validation("I-Ching bar duration overflows i64"))?;
    if duration_ms < 0 {
        return Err(TaError::validation(
            "I-Ching bar close time must not precede its open time",
        ));
    }

    let mut trajectory = trajectory_at(bar_open_time_ms)?;
    if duration_ms == 0 {
        return Ok(trajectory);
    }

    let next_hour = bar_open_time_ms
        .div_euclid(HOUR_MS)
        .checked_add(1)
        .and_then(|hour| hour.checked_mul(HOUR_MS))
        .ok_or_else(|| TaError::validation("I-Ching cast boundary overflows i64"))?;
    let mut boundary = next_hour;

    while boundary < bar_close_time_ms {
        let datetime = chrono::DateTime::from_timestamp_millis(boundary).ok_or_else(|| {
            TaError::computation(format!("I-Ching cast time {boundary} is out of range"))
        })?;
        let (_, local_time) = calendar::to_local_datetime(datetime);
        let local_hour = local_time.hour();

        // Midnight changes the calendar input even though Zi's branch remains
        // active. Odd hours are the remaining branch boundaries for the
        // `hour.div_ceil(2)` mapping.
        if local_hour == 0 || local_hour % 2 == 1 {
            fold_cast(&mut trajectory, boundary)?;
        }
        boundary = boundary
            .checked_add(HOUR_MS)
            .ok_or_else(|| TaError::validation("I-Ching cast boundary overflows i64"))?;
    }

    Ok(trajectory)
}

fn fold_cast(trajectory: &mut IchingBarTrajectory, time_ms: i64) -> TaResult<()> {
    let next = trajectory_at(time_ms)?;
    trajectory.energy_high = trajectory.energy_high.max(next.energy_close);
    trajectory.energy_low = trajectory.energy_low.min(next.energy_close);
    trajectory.energy_close = next.energy_close;
    trajectory.transformed_close = next.transformed_close;
    trajectory.nuclear_close = next.nuclear_close;
    trajectory.moving_line = next.moving_line;
    Ok(())
}

fn trajectory_at(time_ms: i64) -> TaResult<IchingBarTrajectory> {
    let datetime = chrono::DateTime::from_timestamp_millis(time_ms).ok_or_else(|| {
        TaError::computation(format!("I-Ching cast time {time_ms} is out of range"))
    })?;
    let signal = plum_blossom_signal_with_policy(datetime, LeapMonthPolicy::Allow)
        .map_err(|error| TaError::computation(error.to_string()))?;
    let energy = signal.original.energy;
    let transformed_close = signal
        .transformed
        .ok_or_else(|| TaError::computation("Plum Blossom transformed channel is missing"))?
        .energy;
    Ok(IchingBarTrajectory {
        energy_open: energy,
        energy_high: energy,
        energy_low: energy,
        energy_close: energy,
        transformed_open: transformed_close,
        transformed_close,
        nuclear_open: signal.nuclear.energy,
        nuclear_close: signal.nuclear.energy,
        moving_line: signal.moving_line,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn bar_without_internal_boundary_degenerates_to_flat_tick() {
        let open_time_ms = 1_704_067_200_000;
        let close_time_ms = open_time_ms + 60 * 60 * 1_000;
        let signal = plum_blossom_signal_with_policy(
            chrono::DateTime::from_timestamp_millis(open_time_ms).expect("valid test timestamp"),
            LeapMonthPolicy::Allow,
        )
        .expect("test cast must succeed");

        let trajectory = iching_bar_trajectory(open_time_ms, close_time_ms)
            .expect("short bar trajectory must succeed");

        assert_eq!(trajectory.energy_open, signal.original.energy);
        assert_eq!(trajectory.energy_high, signal.original.energy);
        assert_eq!(trajectory.energy_low, signal.original.energy);
        assert_eq!(trajectory.energy_close, signal.original.energy);
        assert_eq!(
            trajectory.transformed_open,
            signal.transformed.expect("plum blossom transforms").energy
        );
        assert_eq!(
            trajectory.transformed_close,
            signal.transformed.expect("plum blossom transforms").energy
        );
        assert_eq!(trajectory.nuclear_open, signal.nuclear.energy);
        assert_eq!(trajectory.nuclear_close, signal.nuclear.energy);
        assert_eq!(trajectory.moving_line, signal.moving_line);
    }

    #[test]
    fn off_grid_open_is_the_true_open_cast() {
        let open_time_ms = 1_704_067_200_000 + 30 * 60 * 1_000;
        let close_time_ms = open_time_ms + 4 * HOUR_MS;
        let open_signal = plum_blossom_signal_with_policy(
            chrono::DateTime::from_timestamp_millis(open_time_ms).expect("valid test timestamp"),
            LeapMonthPolicy::Allow,
        )
        .expect("opening cast");

        let trajectory = iching_bar_trajectory(open_time_ms, close_time_ms)
            .expect("long bar trajectory must succeed");

        assert_eq!(trajectory.energy_open, open_signal.original.energy);
        assert!(trajectory.energy_open <= trajectory.energy_high);
        assert!(trajectory.energy_low <= trajectory.energy_close);
    }

    #[test]
    fn sub_two_hour_bar_can_cross_a_cast_boundary() {
        // UTC 16:30 is CST 00:30; the interval crosses the CST 01:00
        // branch boundary at UTC 17:00 while remaining shorter than two hours.
        let open_time_ms = chrono::Utc
            .with_ymd_and_hms(2024, 1, 1, 16, 30, 0)
            .unwrap()
            .timestamp_millis();
        let boundary_time_ms = chrono::Utc
            .with_ymd_and_hms(2024, 1, 1, 17, 0, 0)
            .unwrap()
            .timestamp_millis();
        let close_time_ms = chrono::Utc
            .with_ymd_and_hms(2024, 1, 1, 18, 0, 0)
            .unwrap()
            .timestamp_millis();
        let boundary_signal = plum_blossom_signal_with_policy(
            chrono::DateTime::from_timestamp_millis(boundary_time_ms)
                .expect("valid boundary timestamp"),
            LeapMonthPolicy::Allow,
        )
        .expect("boundary cast");
        let open_signal = plum_blossom_signal_with_policy(
            chrono::DateTime::from_timestamp_millis(open_time_ms).expect("valid opening timestamp"),
            LeapMonthPolicy::Allow,
        )
        .expect("opening cast");
        assert_ne!(
            open_signal.original.energy, boundary_signal.original.energy,
            "fixture must cross a real I-Ching state boundary"
        );

        let trajectory = iching_bar_trajectory(open_time_ms, close_time_ms)
            .expect("cross-boundary bar trajectory");

        assert_eq!(trajectory.energy_close, boundary_signal.original.energy);
    }

    #[test]
    fn exact_close_boundary_belongs_to_the_next_bar() {
        let open_time_ms = 1_704_067_200_000;
        let close_time_ms = open_time_ms + 4 * HOUR_MS;
        let just_before_close_ms = close_time_ms - 1;
        let expected = plum_blossom_signal_with_policy(
            chrono::DateTime::from_timestamp_millis(just_before_close_ms)
                .expect("valid pre-close timestamp"),
            LeapMonthPolicy::Allow,
        )
        .expect("pre-close cast");

        let trajectory =
            iching_bar_trajectory(open_time_ms, close_time_ms).expect("aligned bar trajectory");

        assert_eq!(trajectory.energy_close, expected.original.energy);
        assert_eq!(
            trajectory.transformed_close,
            expected
                .transformed
                .expect("plum blossom transforms")
                .energy
        );
    }
}
