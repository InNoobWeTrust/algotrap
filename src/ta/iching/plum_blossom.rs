//! Pure Plum Blossom casting over discrete inputs (no I/O, no chrono).
//!
//! The facade consumption is deferred to a later unit; items are crate-visible
//! but not yet consumed outside this module.
#![allow(dead_code)]

use super::types::{Hexagram, HexagramEnergy, Trigram};

/// Discrete Plum Blossom input — all validated discrete values.
/// Constructed only by the calendar adapter; callers must not fabricate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlumBlossomInput {
    /// Earthly branch of the lunar year, 1..=12. Branch 1 = Zi.
    pub year_branch: u8,
    /// Lunar month, 1..=12.
    pub lunar_month: u8,
    /// Lunar day, 1..=30.
    pub lunar_day: u8,
    /// Earthly branch of the hour, 1..=12. Branch 1 = Zi.
    pub hour_branch: u8,
}

/// Internal Plum Blossom casting record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlumBlossomRecord {
    /// Canonical six-bit original value 0..=63.
    pub hexagram: u8,
    /// Moving line position 1..=6 (bottom-to-top).
    pub moving_line: u8,
    /// Canonical six-bit value after flip 0..=63.
    pub transformed: u8,
    /// Canonical six-bit mutual value 0..=63.
    pub mutual: u8,
}

/// Pure Plum Blossom cast over discrete inputs.
///
/// Formulas (research §3.1):
/// ```text
/// S1 = year_branch + lunar_month + lunar_day
/// S2 = S1 + hour_branch
/// upper_num = if S1 % 8 == 0 { 8 } else { S1 % 8 }  // 1..=8
/// lower_num = if S2 % 8 == 0 { 8 } else { S2 % 8 }  // 1..=8
/// moving_line = if S2 % 6 == 0 { 6 } else { S2 % 6 } // 1..=6
/// ```
pub fn cast(input: PlumBlossomInput) -> crate::ta::TaResult<PlumBlossomRecord> {
    if !(1..=12).contains(&input.year_branch) {
        return Err(crate::ta::TaError::validation("year_branch must be 1..=12"));
    }
    if !(1..=12).contains(&input.lunar_month) {
        return Err(crate::ta::TaError::validation("lunar_month must be 1..=12"));
    }
    if !(1..=30).contains(&input.lunar_day) {
        return Err(crate::ta::TaError::validation("lunar_day must be 1..=30"));
    }
    if !(1..=12).contains(&input.hour_branch) {
        return Err(crate::ta::TaError::validation("hour_branch must be 1..=12"));
    }
    let s1: u32 = input.year_branch as u32 + input.lunar_month as u32 + input.lunar_day as u32;
    let s2: u32 = s1 + input.hour_branch as u32;
    let upper = Trigram::from_num_mod8(s1)?;
    let lower = Trigram::from_num_mod8(s2)?;
    let r = s2 % 6;
    let moving_line: u8 = if r == 0 { 6 } else { r as u8 };
    let original = Hexagram::from_trigrams(upper, lower);
    let hexagram = original.binary_index();
    let transformed = original.flip_line(moving_line)?.binary_index();
    let mutual = original.mutual().binary_index();
    Ok(PlumBlossomRecord {
        hexagram,
        moving_line,
        transformed,
        mutual,
    })
}

/// Result of a Plum Blossom computation, materializing energy channels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PlumBlossomResult {
    pub original: HexagramEnergy,
    pub transformed: HexagramEnergy,
    pub mutual: HexagramEnergy,
    pub moving_line: u8,
}

/// Computes Plum Blossom energy channels from discrete inputs.
///
/// 1. Calls `cast(input)` to get the `PlumBlossomRecord`
/// 2. Constructs `HexagramEnergy::new(record.hexagram)` for each channel
/// 3. Each channel's `energy == hexagram as f64 - 31.5` by factory invariant
/// 4. `moving_line` does not alter any channel's energy
pub fn compute_channels(input: PlumBlossomInput) -> crate::ta::TaResult<PlumBlossomResult> {
    let record = cast(input)?;
    Ok(PlumBlossomResult {
        original: HexagramEnergy::new(record.hexagram)?,
        transformed: HexagramEnergy::new(record.transformed)?,
        mutual: HexagramEnergy::new(record.mutual)?,
        moving_line: record.moving_line,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ta::TaErrorKind;

    fn fixtures_per_moving_line() -> [(PlumBlossomInput, u8); 6] {
        [
            (
                PlumBlossomInput {
                    year_branch: 1,
                    lunar_month: 1,
                    lunar_day: 1,
                    hour_branch: 4,
                },
                1,
            ),
            (
                PlumBlossomInput {
                    year_branch: 1,
                    lunar_month: 1,
                    lunar_day: 1,
                    hour_branch: 5,
                },
                2,
            ),
            (
                PlumBlossomInput {
                    year_branch: 1,
                    lunar_month: 1,
                    lunar_day: 1,
                    hour_branch: 6,
                },
                3,
            ),
            (
                PlumBlossomInput {
                    year_branch: 1,
                    lunar_month: 1,
                    lunar_day: 1,
                    hour_branch: 1,
                },
                4,
            ),
            (
                PlumBlossomInput {
                    year_branch: 1,
                    lunar_month: 1,
                    lunar_day: 1,
                    hour_branch: 2,
                },
                5,
            ),
            (
                PlumBlossomInput {
                    year_branch: 1,
                    lunar_month: 1,
                    lunar_day: 1,
                    hour_branch: 3,
                },
                6,
            ),
        ]
    }

    #[test]
    fn cast_min_fixture_matches_hand_computed() {
        // S1 = 1+1+1 = 3, S2 = 3+1 = 4.
        // upper_num = 3 (Li), lower_num = 4 (Zhen), moving = 4.
        let input = PlumBlossomInput {
            year_branch: 1,
            lunar_month: 1,
            lunar_day: 1,
            hour_branch: 1,
        };
        let record = cast(input).expect("min fixture is valid");
        assert_eq!(record.moving_line, 4);
        let upper = Trigram::from_num(3).expect("3 is Li");
        let lower = Trigram::from_num(4).expect("4 is Zhen");
        let expected_hex = Hexagram::from_trigrams(upper, lower).binary_index();
        assert_eq!(record.hexagram, expected_hex);
        assert_eq!(record.hexagram, 41);
        let expected_transformed = Hexagram::from_binary_index(expected_hex)
            .expect("0..=63 is valid")
            .flip_line(4)
            .expect("line 4 is valid")
            .binary_index();
        assert_eq!(record.transformed, expected_transformed);
        let expected_mutual = Hexagram::from_binary_index(expected_hex)
            .expect("0..=63 is valid")
            .mutual()
            .binary_index();
        assert_eq!(record.mutual, expected_mutual);
    }

    #[test]
    fn cast_max_fixture_matches_hand_computed() {
        // S1 = 12+12+30 = 54, S2 = 54+12 = 66.
        // upper = 54 % 8 = 6 (Kan), lower = 66 % 8 = 2 (Dui), moving = 66 % 6 = 0 -> 6.
        let input = PlumBlossomInput {
            year_branch: 12,
            lunar_month: 12,
            lunar_day: 30,
            hour_branch: 12,
        };
        let record = cast(input).expect("max fixture is valid");
        assert_eq!(record.moving_line, 6);
        let upper = Trigram::from_num(6).expect("6 is Kan");
        let lower = Trigram::from_num(2).expect("2 is Dui");
        let expected_hex = Hexagram::from_trigrams(upper, lower).binary_index();
        assert_eq!(record.hexagram, expected_hex);
        assert_eq!(record.hexagram, 19);
        let expected_transformed = Hexagram::from_binary_index(expected_hex)
            .expect("0..=63 is valid")
            .flip_line(6)
            .expect("line 6 is valid")
            .binary_index();
        assert_eq!(record.transformed, expected_transformed);
        let expected_mutual = Hexagram::from_binary_index(expected_hex)
            .expect("0..=63 is valid")
            .mutual()
            .binary_index();
        assert_eq!(record.mutual, expected_mutual);
    }

    #[test]
    fn cast_rejects_invalid_values() {
        let base = PlumBlossomInput {
            year_branch: 1,
            lunar_month: 1,
            lunar_day: 1,
            hour_branch: 1,
        };
        let err_year = cast(PlumBlossomInput {
            year_branch: 0,
            ..base
        })
        .expect_err("year_branch 0 must be rejected");
        assert_eq!(err_year.kind, TaErrorKind::Validation);
        let err_hour = cast(PlumBlossomInput {
            hour_branch: 13,
            ..base
        })
        .expect_err("hour_branch 13 must be rejected");
        assert_eq!(err_hour.kind, TaErrorKind::Validation);
        let err_day = cast(PlumBlossomInput {
            lunar_day: 0,
            ..base
        })
        .expect_err("lunar_day 0 must be rejected");
        assert_eq!(err_day.kind, TaErrorKind::Validation);
        let err_month_low = cast(PlumBlossomInput {
            lunar_month: 0,
            ..base
        })
        .expect_err("lunar_month 0 must be rejected");
        assert_eq!(err_month_low.kind, TaErrorKind::Validation);
        let err_month_high = cast(PlumBlossomInput {
            lunar_month: 13,
            ..base
        })
        .expect_err("lunar_month 13 must be rejected");
        assert_eq!(err_month_high.kind, TaErrorKind::Validation);
        let err_day_high = cast(PlumBlossomInput {
            lunar_day: 31,
            ..base
        })
        .expect_err("lunar_day 31 must be rejected");
        assert_eq!(err_day_high.kind, TaErrorKind::Validation);
        let err_year_high = cast(PlumBlossomInput {
            year_branch: 13,
            ..base
        })
        .expect_err("year_branch 13 must be rejected");
        assert_eq!(err_year_high.kind, TaErrorKind::Validation);
        let err_hour_low = cast(PlumBlossomInput {
            hour_branch: 0,
            ..base
        })
        .expect_err("hour_branch 0 must be rejected");
        assert_eq!(err_hour_low.kind, TaErrorKind::Validation);
    }

    #[test]
    fn cast_moving_line_always_in_range() {
        let min = cast(PlumBlossomInput {
            year_branch: 1,
            lunar_month: 1,
            lunar_day: 1,
            hour_branch: 1,
        })
        .expect("min fixture is valid");
        assert!((1..=6).contains(&min.moving_line));
        let max = cast(PlumBlossomInput {
            year_branch: 12,
            lunar_month: 12,
            lunar_day: 30,
            hour_branch: 12,
        })
        .expect("max fixture is valid");
        assert!((1..=6).contains(&max.moving_line));
        for (input, _) in fixtures_per_moving_line() {
            let record = cast(input).expect("fixture is valid");
            assert!(
                (1..=6).contains(&record.moving_line),
                "moving_line {} out of range",
                record.moving_line
            );
        }
    }

    #[test]
    fn cast_transformed_differs_for_each_moving_line() {
        for (input, expected_moving) in fixtures_per_moving_line() {
            let record = cast(input).expect("fixture is valid");
            assert_eq!(
                record.moving_line, expected_moving,
                "moving line for fixture {input:?}"
            );
            assert_ne!(
                record.transformed, record.hexagram,
                "transformed must differ for moving line {expected_moving}"
            );
        }
    }

    #[test]
    fn cast_mutual_always_available() {
        for (input, _) in fixtures_per_moving_line() {
            let record = cast(input).expect("fixture is valid");
            assert!(record.mutual <= 63, "mutual must be 0..=63");
            assert!(record.hexagram <= 63, "hexagram must be 0..=63");
            assert!(record.transformed <= 63, "transformed must be 0..=63");
            let original =
                Hexagram::from_binary_index(record.hexagram).expect("hexagram is 0..=63");
            assert_eq!(
                record.mutual,
                original.mutual().binary_index(),
                "mutual must match Hexagram::mutual for {input:?}"
            );
            Hexagram::from_binary_index(record.mutual)
                .expect("mutual must be a valid hexagram index");
        }
    }

    #[test]
    fn cast_double_flip_restores_original() {
        for (input, _) in fixtures_per_moving_line() {
            let record = cast(input).expect("fixture is valid");
            let transformed =
                Hexagram::from_binary_index(record.transformed).expect("transformed is 0..=63");
            let restored = transformed
                .flip_line(record.moving_line)
                .expect("moving line is 1..=6")
                .binary_index();
            assert_eq!(
                restored, record.hexagram,
                "double flip must restore original for {input:?}"
            );
        }
    }

    #[test]
    fn compute_channels_populates_all_three_channels() {
        let input = PlumBlossomInput {
            year_branch: 1,
            lunar_month: 1,
            lunar_day: 1,
            hour_branch: 1,
        };
        let record = cast(input).expect("fixture is valid");
        let result = compute_channels(input).expect("valid input returns Ok");
        assert_eq!(result.original.hexagram, record.hexagram);
        assert_eq!(result.transformed.hexagram, record.transformed);
        assert_eq!(result.mutual.hexagram, record.mutual);
        assert!(result.original.hexagram <= 63);
        assert!(result.transformed.hexagram <= 63);
        assert!(result.mutual.hexagram <= 63);
    }

    #[test]
    fn compute_channels_energy_formula_each_channel() {
        for (input, _) in fixtures_per_moving_line() {
            let result = compute_channels(input).expect("fixture is valid");
            assert_eq!(
                result.original.energy,
                result.original.hexagram as f64 - 31.5,
                "original energy formula for {input:?}"
            );
            assert_eq!(
                result.transformed.energy,
                result.transformed.hexagram as f64 - 31.5,
                "transformed energy formula for {input:?}"
            );
            assert_eq!(
                result.mutual.energy,
                result.mutual.hexagram as f64 - 31.5,
                "mutual energy formula for {input:?}"
            );
        }
    }

    #[test]
    fn compute_channels_moving_line_in_range_and_preserved() {
        for (input, _) in fixtures_per_moving_line() {
            let record = cast(input).expect("fixture is valid");
            let result = compute_channels(input).expect("fixture is valid");
            assert!(
                (1..=6).contains(&result.moving_line),
                "moving_line {} out of range",
                result.moving_line
            );
            assert_eq!(
                result.moving_line, record.moving_line,
                "compute_channels preserves moving line for {input:?}"
            );
        }
    }

    #[test]
    fn compute_channels_fixed_input_matches_manual_channels() {
        // Fixed input: year=1, month=1, day=1, hour=1 -> S1=3, S2=4
        // -> hexagram 41 (hand-verified in cast_min_fixture_matches_hand_computed).
        let input = PlumBlossomInput {
            year_branch: 1,
            lunar_month: 1,
            lunar_day: 1,
            hour_branch: 1,
        };
        let record = cast(input).expect("fixture is valid");
        assert_eq!(record.hexagram, 41);
        let result = compute_channels(input).expect("fixture is valid");
        let expected_original = HexagramEnergy::new(record.hexagram).expect("0..=63 is valid");
        let expected_transformed =
            HexagramEnergy::new(record.transformed).expect("0..=63 is valid");
        let expected_mutual = HexagramEnergy::new(record.mutual).expect("0..=63 is valid");
        assert_eq!(result.original, expected_original);
        assert_eq!(result.transformed, expected_transformed);
        assert_eq!(result.mutual, expected_mutual);
        assert_eq!(result.original.energy, record.hexagram as f64 - 31.5);
        assert_eq!(result.transformed.energy, record.transformed as f64 - 31.5);
        assert_eq!(result.mutual.energy, record.mutual as f64 - 31.5);
        assert_eq!(result.moving_line, 4);
    }

    #[test]
    fn compute_channels_midpoint_energies() {
        let zero = HexagramEnergy::new(0).expect("0 is valid");
        assert_eq!(zero.energy, -31.5);
        let max = HexagramEnergy::new(63).expect("63 is valid");
        assert_eq!(max.energy, 31.5);
    }
}
