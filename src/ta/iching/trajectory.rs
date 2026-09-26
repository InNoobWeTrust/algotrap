//! I-Ching cast trajectories materialized over market-bar time ranges.

use super::{LeapMonthPolicy, calendar, plum_blossom_signal_with_policy};
use crate::ta::{TaError, TaResult};
use chrono::Timelike;

const HOUR_MS: i64 = 60 * 60 * 1_000;

/// I-Ching per-channel trajectory data for one market bar.
///
/// - **Original (本卦)** is the evaluated present state, surfaced as an OHLC
///   envelope: `energy_open/high/low/close` span the distinct original energies
///   observed while the bar was open.
/// - **Transformed (变卦)** is the predicted next state, surfaced at the
///   terminal cast as `transformed_close`.
/// - **Mutual (互卦)** is the inner structure, surfaced as an intra-bar band:
///   `mutual_high`/`mutual_low` bound every mutual value observed in the bar,
///   `mutual_mean` is the arithmetic average of those values, and
///   `mutual_close` is the terminal value.
#[derive(Debug, Clone, PartialEq)]
pub struct IchingBarTrajectory {
    /// Original energy at the bar open (opening cast).
    pub energy_open: f64,
    /// Highest original energy observed in `[open, close)`.
    pub energy_high: f64,
    /// Lowest original energy observed in `[open, close)`.
    pub energy_low: f64,
    /// Original energy at the last cast owned by the bar (terminal cast).
    pub energy_close: f64,
    /// Transformed-channel energy derived from the opening cast.
    pub transformed_open: f64,
    /// Transformed-channel energy derived from the terminal cast inside the bar.
    pub transformed_close: f64,
    /// Mutual-channel energy derived from the opening cast.
    pub mutual_open: f64,
    /// Mutual-channel energy derived from the terminal cast inside the bar.
    pub mutual_close: f64,
    /// Highest mutual energy observed in `[open, close)`.
    pub mutual_high: f64,
    /// Lowest mutual energy observed in `[open, close)`.
    pub mutual_low: f64,
    /// Average of all mutual values observed in `[open, close)`.
    pub mutual_mean: f64,
    /// Moving line of the terminal cast (one-based, bottom-to-top).
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
    let mut mutual_high = trajectory.mutual_close;
    let mut mutual_low = trajectory.mutual_close;
    let mut mutual_sum = trajectory.mutual_close;
    let mut cast_count: u32 = 1;
    if duration_ms == 0 {
        trajectory.mutual_high = mutual_high;
        trajectory.mutual_low = mutual_low;
        trajectory.mutual_mean = mutual_sum;
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
            fold_cast(
                &mut trajectory,
                boundary,
                &mut mutual_high,
                &mut mutual_low,
                &mut mutual_sum,
                &mut cast_count,
            )?;
        }
        boundary = boundary
            .checked_add(HOUR_MS)
            .ok_or_else(|| TaError::validation("I-Ching cast boundary overflows i64"))?;
    }

    let mutual_mean = mutual_sum / cast_count as f64;
    trajectory.mutual_high = mutual_high;
    trajectory.mutual_low = mutual_low;
    trajectory.mutual_mean = mutual_mean;
    Ok(trajectory)
}

fn fold_cast(
    trajectory: &mut IchingBarTrajectory,
    time_ms: i64,
    mutual_high: &mut f64,
    mutual_low: &mut f64,
    mutual_sum: &mut f64,
    cast_count: &mut u32,
) -> TaResult<()> {
    let next = trajectory_at(time_ms)?;
    trajectory.energy_high = trajectory.energy_high.max(next.energy_close);
    trajectory.energy_low = trajectory.energy_low.min(next.energy_close);
    trajectory.energy_close = next.energy_close;
    trajectory.transformed_close = next.transformed_close;
    trajectory.mutual_close = next.mutual_close;
    trajectory.moving_line = next.moving_line;
    let value = next.mutual_close;
    *mutual_high = (*mutual_high).max(value);
    *mutual_low = (*mutual_low).min(value);
    *mutual_sum += value;
    *cast_count = cast_count
        .checked_add(1)
        .ok_or_else(|| TaError::validation("I-Ching intra-bar cast count overflows"))?;
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
        mutual_open: signal.mutual.energy,
        mutual_close: signal.mutual.energy,
        mutual_high: signal.mutual.energy,
        mutual_low: signal.mutual.energy,
        mutual_mean: signal.mutual.energy,
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
        let open_mutual = signal.mutual.energy;

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
        assert_eq!(trajectory.mutual_open, open_mutual);
        assert_eq!(trajectory.mutual_close, open_mutual);
        assert_eq!(trajectory.mutual_high, open_mutual);
        assert_eq!(trajectory.mutual_low, open_mutual);
        assert_eq!(trajectory.mutual_mean, open_mutual);
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
        // The mutual band always contains the mean and spans open..close.
        assert!(
            trajectory.mutual_low <= trajectory.mutual_mean
                && trajectory.mutual_mean <= trajectory.mutual_high
        );
        assert!(
            trajectory.mutual_low <= trajectory.mutual_open
                && trajectory.mutual_open <= trajectory.mutual_high
        );
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
        // Exactly two observations (open + boundary) drive the mutual band.
        let open_mutual = open_signal.mutual.energy;
        let boundary_mutual = boundary_signal.mutual.energy;
        let expected_mean = (open_mutual + boundary_mutual) / 2.0;
        assert!((trajectory.mutual_mean - expected_mean).abs() < 1e-9);
        assert_eq!(trajectory.mutual_high, open_mutual.max(boundary_mutual));
        assert_eq!(trajectory.mutual_low, open_mutual.min(boundary_mutual));
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

    #[test]
    fn mutual_band_covers_many_internal_casts() {
        // A monthly-spanning window crosses many midnight/odd-hour boundaries,
        // so the band must widen beyond any single cast value.
        let open_time_ms = chrono::Utc
            .with_ymd_and_hms(2024, 1, 1, 0, 0, 0)
            .unwrap()
            .timestamp_millis();
        let close_time_ms = chrono::Utc
            .with_ymd_and_hms(2024, 1, 31, 0, 0, 0)
            .unwrap()
            .timestamp_millis();

        let trajectory =
            iching_bar_trajectory(open_time_ms, close_time_ms).expect("wide-bar trajectory");

        // The energy domain is [-31.5, 31.5]; a real month must show a range.
        assert!(trajectory.mutual_high > trajectory.mutual_low);
        assert!(trajectory.energy_high >= trajectory.energy_low);
        assert!(trajectory.mutual_low.is_finite() && trajectory.mutual_high.is_finite());
    }
}
