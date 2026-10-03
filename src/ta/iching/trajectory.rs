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
/// - **Transformed (变卦)** is the within-cast moving-line destination, surfaced at the
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
    /// Moving line of the opening cast (one-based, bottom-to-top), unchanged by folding.
    pub moving_line_open: Option<u8>,
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
        moving_line_open: signal.moving_line,
        moving_line: signal.moving_line,
    })
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use chrono::TimeZone;

    // Independent physical cast oracle: only the calendar adapter is shared.
    pub(crate) fn reference_observations(open: i64, close: i64) -> Vec<(i64, [u8; 3], u8)> {
        let mut instants = vec![open];
        let mut hour = (open.div_euclid(3_600_000) + 1) * 3_600_000;
        while hour < close {
            let utc = chrono::DateTime::from_timestamp_millis(hour).unwrap();
            let cst_hour = (utc.hour() + 8) % 24;
            if cst_hour == 0 || cst_hour % 2 == 1 {
                instants.push(hour);
            }
            hour += 3_600_000;
        }
        let patterns = [
            [0, 0, 0], // modulo zero means trigram 8, Kun
            [1, 1, 1],
            [1, 1, 0],
            [1, 0, 1],
            [1, 0, 0],
            [0, 1, 1],
            [0, 1, 0],
            [0, 0, 1],
        ];
        instants
            .into_iter()
            .map(|instant| {
                let input = calendar::to_plum_blossom_input(
                    chrono::DateTime::from_timestamp_millis(instant).unwrap(),
                    LeapMonthPolicy::Allow,
                )
                .unwrap();
                let s1 = u32::from(input.year_branch)
                    + u32::from(input.lunar_month)
                    + u32::from(input.lunar_day);
                let s2 = s1 + u32::from(input.hour_branch);
                let upper = patterns[(s1 % 8) as usize];
                let lower = patterns[(s2 % 8) as usize];
                let original = [lower[0], lower[1], lower[2], upper[0], upper[1], upper[2]];
                let moving_line = if s2 % 6 == 0 { 6 } else { (s2 % 6) as u8 };
                let mut transformed = original;
                transformed[usize::from(moving_line - 1)] ^= 1;
                let mutual = [
                    original[1],
                    original[2],
                    original[3],
                    original[2],
                    original[3],
                    original[4],
                ];
                let coordinates = [original, transformed, mutual].map(|lines| {
                    lines
                        .iter()
                        .zip([32, 16, 8, 4, 2, 1])
                        .map(|(&line, weight)| line * weight)
                        .sum()
                });
                (instant, coordinates, moving_line)
            })
            .collect()
    }

    pub(crate) fn reference_trajectory(open: i64, close: i64) -> IchingBarTrajectory {
        let observations = reference_observations(open, close);
        let (_, first, first_line) = observations[0];
        let (_, last, last_line) = *observations.last().unwrap();
        let original_high = observations.iter().map(|(_, q, _)| q[0]).max().unwrap();
        let original_low = observations.iter().map(|(_, q, _)| q[0]).min().unwrap();
        let mutual_high = observations.iter().map(|(_, q, _)| q[2]).max().unwrap();
        let mutual_low = observations.iter().map(|(_, q, _)| q[2]).min().unwrap();
        let mutual_sum: f64 = observations.iter().map(|(_, q, _)| f64::from(q[2])).sum();
        IchingBarTrajectory {
            energy_open: f64::from(first[0]) - 31.5,
            energy_high: f64::from(original_high) - 31.5,
            energy_low: f64::from(original_low) - 31.5,
            energy_close: f64::from(last[0]) - 31.5,
            transformed_open: f64::from(first[1]) - 31.5,
            transformed_close: f64::from(last[1]) - 31.5,
            mutual_open: f64::from(first[2]) - 31.5,
            mutual_close: f64::from(last[2]) - 31.5,
            mutual_high: f64::from(mutual_high) - 31.5,
            mutual_low: f64::from(mutual_low) - 31.5,
            mutual_mean: mutual_sum / observations.len() as f64 - 31.5,
            moving_line_open: Some(first_line),
            moving_line: Some(last_line),
        }
    }

    pub(crate) fn assert_reference_trajectory(
        actual: &IchingBarTrajectory,
        expected: &IchingBarTrajectory,
    ) {
        assert_eq!(
            [
                actual.energy_open,
                actual.energy_high,
                actual.energy_low,
                actual.energy_close,
                actual.transformed_open,
                actual.transformed_close,
                actual.mutual_open,
                actual.mutual_close,
                actual.mutual_high,
                actual.mutual_low
            ],
            [
                expected.energy_open,
                expected.energy_high,
                expected.energy_low,
                expected.energy_close,
                expected.transformed_open,
                expected.transformed_close,
                expected.mutual_open,
                expected.mutual_close,
                expected.mutual_high,
                expected.mutual_low
            ],
        );
        assert!((actual.mutual_mean - expected.mutual_mean).abs() < 1e-12);
        assert_eq!(actual.moving_line_open, expected.moving_line_open);
        assert_eq!(actual.moving_line, expected.moving_line);
    }

    #[test]
    fn physical_hour_cast_oracle_matches_every_channel_and_all_trajectory_fields() {
        let open = chrono::Utc
            .with_ymd_and_hms(2024, 2, 10, 0, 0, 0)
            .unwrap()
            .timestamp_millis();
        let midnight_open = chrono::Utc
            .with_ymd_and_hms(2024, 2, 9, 15, 30, 0)
            .unwrap()
            .timestamp_millis();
        for (start, close, count) in [
            (open, open + 4 * HOUR_MS, 3),
            (open + HOUR_MS / 2, open + 4 * HOUR_MS, 3),
            (midnight_open, midnight_open + 2 * HOUR_MS, 3),
            (open, open + 3 * HOUR_MS, 2),
            (open, open, 1),
        ] {
            let observations = reference_observations(start, close);
            assert_eq!(observations.len(), count);
            for (instant, q, line) in observations {
                let signal = plum_blossom_signal_with_policy(
                    chrono::DateTime::from_timestamp_millis(instant).unwrap(),
                    LeapMonthPolicy::Allow,
                )
                .unwrap();
                for (channel, expected) in
                    [signal.original, signal.transformed.unwrap(), signal.mutual]
                        .into_iter()
                        .zip(q)
                {
                    assert_eq!(channel.hexagram, expected, "cast {instant}");
                    assert_eq!(channel.energy, f64::from(expected) - 31.5, "cast {instant}");
                }
                assert_eq!(signal.moving_line, Some(line));
            }
            assert_reference_trajectory(
                &iching_bar_trajectory(start, close).unwrap(),
                &reference_trajectory(start, close),
            );
        }
        let observations = reference_observations(open, open + 4 * HOUR_MS);
        assert_eq!(
            observations
                .iter()
                .map(|(_, q, _)| q[0])
                .collect::<Vec<_>>(),
            [33, 25, 17]
        );
        assert_eq!(
            observations
                .iter()
                .map(|(_, q, _)| q[2])
                .collect::<Vec<_>>(),
            [0, 52, 32]
        );
        assert_eq!(
            reference_trajectory(open, open + 4 * HOUR_MS).mutual_mean,
            -3.5
        );
    }

    #[test]
    fn crossing_bar_preserves_opening_and_terminal_cast_tuples() {
        let open = chrono::Utc
            .with_ymd_and_hms(2024, 2, 10, 0, 0, 0)
            .unwrap()
            .timestamp_millis();
        let trajectory = iching_bar_trajectory(open, open + 4 * HOUR_MS).unwrap();
        assert_eq!(trajectory.moving_line_open, Some(6));
        assert_eq!(trajectory.moving_line, Some(2));
        assert_eq!(trajectory.energy_open + 31.5, 33.0);
        assert_eq!(trajectory.transformed_open + 31.5, 32.0);
        assert_eq!(trajectory.energy_close + 31.5, 17.0);
        assert_eq!(trajectory.transformed_close + 31.5, 1.0);
        assert_eq!(
            (trajectory.energy_open + 31.5) as u8
                ^ (1 << (6 - trajectory.moving_line_open.unwrap())),
            (trajectory.transformed_open + 31.5) as u8
        );
        assert_eq!(
            (trajectory.energy_close + 31.5) as u8 ^ (1 << (6 - trajectory.moving_line.unwrap())),
            (trajectory.transformed_close + 31.5) as u8
        );
    }

    #[test]
    fn zero_length_bar_keeps_identical_opening_and_terminal_lines() {
        let open = 1_704_067_200_000;
        let trajectory = iching_bar_trajectory(open, open).unwrap();
        assert_eq!(trajectory.moving_line_open, trajectory.moving_line);
        assert_eq!(trajectory, trajectory_at(open).unwrap());
    }

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
        assert_eq!(trajectory.moving_line_open, signal.moving_line);
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
        assert_eq!(trajectory.moving_line_open, open_signal.moving_line);
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
        assert_eq!(trajectory.moving_line_open, open_signal.moving_line);
        assert_eq!(trajectory.moving_line, boundary_signal.moving_line);
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
        // Close at CST 11:00: the branch and physical original really change.
        let open_time_ms = chrono::Utc
            .with_ymd_and_hms(2024, 2, 10, 0, 0, 0)
            .unwrap()
            .timestamp_millis();
        let close_time_ms = open_time_ms + 3 * HOUR_MS;
        let just_before_close_ms = close_time_ms - 1;
        let expected = plum_blossom_signal_with_policy(
            chrono::DateTime::from_timestamp_millis(just_before_close_ms)
                .expect("valid pre-close timestamp"),
            LeapMonthPolicy::Allow,
        )
        .expect("pre-close cast");

        let trajectory =
            iching_bar_trajectory(open_time_ms, close_time_ms).expect("aligned bar trajectory");

        let before = reference_observations(just_before_close_ms, just_before_close_ms)[0].1;
        let at_close = reference_observations(close_time_ms, close_time_ms)[0].1;
        assert_ne!(before, at_close, "close must change the physical cast");
        assert_eq!(before[0], 25);
        assert_eq!(at_close[0], 17);
        assert_reference_trajectory(
            &trajectory,
            &reference_trajectory(open_time_ms, close_time_ms),
        );
        let next = iching_bar_trajectory(close_time_ms, close_time_ms + HOUR_MS).unwrap();
        assert_reference_trajectory(
            &next,
            &reference_trajectory(close_time_ms, close_time_ms + HOUR_MS),
        );
        assert_eq!(next.energy_open, -14.5);
        assert_ne!(trajectory.energy_close, next.energy_open);

        assert_eq!(trajectory.energy_close, expected.original.energy);
        assert_eq!(
            trajectory.moving_line_open,
            trajectory_at(open_time_ms).unwrap().moving_line
        );
        assert_eq!(trajectory.moving_line, expected.moving_line);
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
        assert_eq!(
            trajectory.moving_line_open,
            trajectory_at(open_time_ms).unwrap().moving_line
        );
        assert!(trajectory.mutual_high > trajectory.mutual_low);
        assert!(trajectory.energy_high >= trajectory.energy_low);
        assert!(trajectory.mutual_low.is_finite() && trajectory.mutual_high.is_finite());
    }
}
