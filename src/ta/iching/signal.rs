use super::calendar;
use super::plum_blossom;
use super::types::{HexagramEnergy, IchingSignal, LeapMonthPolicy};
use crate::ta::{TaError, TaResult};
use chrono::{DateTime, Utc};

/// Constructs a validated `IchingSignal` from separate channel components.
///
/// Validation rules (all must pass or the result is `Err`):
/// - `original.hexagram ∈ 0..=63` (enforced by `HexagramEnergy::new`)
/// - `original.energy == original.hexagram as f64 - 31.5` (enforced by factory)
/// - `transformed`: if `Some`, same invariant as `original`
/// - `nuclear.hexagram ∈ 0..=63` and `nuclear.energy == nuclear.hexagram as f64 - 31.5`
/// - `moving_line`: if `Some(v)`, then `v ∈ 1..=6`
///
/// This function is `pub(crate)` — only the facade and tests call it.
pub(crate) fn build_signal(
    original: HexagramEnergy,
    transformed: Option<HexagramEnergy>,
    nuclear: HexagramEnergy,
    moving_line: Option<u8>,
) -> TaResult<IchingSignal> {
    validate_channel(&original, "original")?;
    validate_channel(&nuclear, "nuclear")?;
    if let Some(channel) = &transformed {
        validate_channel(channel, "transformed")?;
    }
    if moving_line.is_some_and(|line| !(1..=6).contains(&line)) {
        return Err(TaError::validation("moving_line must be 1..=6"));
    }
    Ok(IchingSignal {
        original,
        transformed,
        nuclear,
        moving_line,
    })
}

fn validate_channel(channel: &HexagramEnergy, name: &str) -> TaResult<()> {
    if channel.hexagram > 63 {
        return Err(TaError::validation(format!(
            "{name} hexagram must be 0..=63"
        )));
    }
    let expected = channel.hexagram as f64 - 31.5;
    if channel.energy != expected {
        return Err(TaError::validation(format!(
            "{name} energy must equal hexagram as f64 - 31.5"
        )));
    }
    Ok(())
}

/// Plum Blossom I-Ching signal with default leap-month policy (`Reject`).
///
/// Time policy: `cst-utc8-fixed-v1`. Deterministic, no I/O, no wall-clock read.
pub fn plum_blossom_signal(datetime: DateTime<Utc>) -> TaResult<IchingSignal> {
    plum_blossom_signal_with_policy(datetime, LeapMonthPolicy::Reject)
}

/// Plum Blossom I-Ching signal with explicit leap-month policy.
///
/// 1. `calendar::to_plum_blossom_input(datetime, policy)` → `PlumBlossomInput`
/// 2. `plum_blossom::compute_channels(input)` → `PlumBlossomResult`
/// 3. `build_signal(original, Some(transformed), nuclear, Some(moving_line))` → `IchingSignal`
pub fn plum_blossom_signal_with_policy(
    datetime: DateTime<Utc>,
    policy: LeapMonthPolicy,
) -> TaResult<IchingSignal> {
    let input = calendar::to_plum_blossom_input(datetime, policy)?;
    let channels = plum_blossom::compute_channels(input)?;
    build_signal(
        channels.original,
        Some(channels.transformed),
        channels.nuclear,
        Some(channels.moving_line),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ta::TaErrorKind;
    use chrono::{Datelike, NaiveDate, TimeZone};

    fn valid_channels() -> (HexagramEnergy, HexagramEnergy, HexagramEnergy) {
        let original = HexagramEnergy::new(10).expect("10 is valid");
        let transformed = HexagramEnergy::new(21).expect("21 is valid");
        let nuclear = HexagramEnergy::new(42).expect("42 is valid");
        (original, transformed, nuclear)
    }

    #[test]
    fn valid_signal_with_all_channels() {
        let (original, transformed, nuclear) = valid_channels();
        let signal = build_signal(original, Some(transformed), nuclear, Some(3))
            .expect("valid inputs must succeed");
        assert_eq!(signal.original, original);
        assert_eq!(signal.transformed, Some(transformed));
        assert_eq!(signal.nuclear, nuclear);
        assert_eq!(signal.moving_line, Some(3));
    }

    #[test]
    fn moving_line_zero_rejected_as_validation() {
        let (original, transformed, nuclear) = valid_channels();
        let err = build_signal(original, Some(transformed), nuclear, Some(0))
            .expect_err("moving_line 0 must be rejected");
        assert_eq!(err.kind, TaErrorKind::Validation);
    }

    #[test]
    fn moving_line_seven_rejected_as_validation() {
        let (original, transformed, nuclear) = valid_channels();
        let err = build_signal(original, Some(transformed), nuclear, Some(7))
            .expect_err("moving_line 7 must be rejected");
        assert_eq!(err.kind, TaErrorKind::Validation);
    }

    #[test]
    fn transformed_none_and_moving_none_succeeds() {
        let (original, _, nuclear) = valid_channels();
        let signal = build_signal(original, None, nuclear, None)
            .expect("None/None must succeed for future methods");
        assert_eq!(signal.original, original);
        assert_eq!(signal.transformed, None);
        assert_eq!(signal.nuclear, nuclear);
        assert_eq!(signal.moving_line, None);
    }

    #[test]
    fn transformed_none_with_moving_some_succeeds() {
        let (original, _, nuclear) = valid_channels();
        let signal = build_signal(original, None, nuclear, Some(1))
            .expect("uncoupled None/Some(1) must succeed");
        assert_eq!(signal.transformed, None);
        assert_eq!(signal.moving_line, Some(1));
    }

    #[test]
    fn raw_out_of_range_original_rejected() {
        let (_, transformed, nuclear) = valid_channels();
        let raw = HexagramEnergy {
            hexagram: 64,
            energy: 64.0_f64 - 31.5,
        };
        let err = build_signal(raw, Some(transformed), nuclear, Some(3))
            .expect_err("hexagram 64 must be rejected");
        assert_eq!(err.kind, TaErrorKind::Validation);
    }

    #[test]
    fn mismatched_energy_original_rejected() {
        let (_, transformed, nuclear) = valid_channels();
        let raw = HexagramEnergy {
            hexagram: 10,
            energy: 0.0,
        };
        let err = build_signal(raw, Some(transformed), nuclear, Some(3))
            .expect_err("mismatched original energy must be rejected");
        assert_eq!(err.kind, TaErrorKind::Validation);
    }

    #[test]
    fn mismatched_energy_nuclear_rejected() {
        let (original, transformed, _) = valid_channels();
        let raw_nuclear = HexagramEnergy {
            hexagram: 5,
            energy: 99.0,
        };
        let err = build_signal(original, Some(transformed), raw_nuclear, Some(2))
            .expect_err("mismatched nuclear energy must be rejected");
        assert_eq!(err.kind, TaErrorKind::Validation);
    }

    #[test]
    fn invalid_transformed_rejected() {
        let (original, _, nuclear) = valid_channels();
        let raw_transformed = HexagramEnergy {
            hexagram: 7,
            energy: 7.0,
        };
        let err = build_signal(original, Some(raw_transformed), nuclear, Some(4))
            .expect_err("mismatched transformed energy must be rejected");
        assert_eq!(err.kind, TaErrorKind::Validation);
    }

    fn find_2020_leap_solar_date() -> NaiveDate {
        let mut cursor = NaiveDate::from_ymd_opt(2020, 5, 23).expect("valid start");
        let end = NaiveDate::from_ymd_opt(2020, 6, 20).expect("valid end");
        while cursor <= end {
            let fields = calendar::resolve_lunar(cursor, LeapMonthPolicy::Allow)
                .expect("solar date in range");
            if fields.is_leap_month {
                return cursor;
            }
            cursor = cursor.succ_opt().expect("valid successor");
        }
        panic!("2020 leap 4th-month window must contain a leap date");
    }

    #[test]
    fn facade_fixed_datetime_populates_all_channels() {
        // 2024-02-10T00:00:00Z is 08:00 CST on Lunar New Year (lunar 2024-01-01).
        let dt = Utc
            .with_ymd_and_hms(2024, 2, 10, 0, 0, 0)
            .single()
            .expect("valid datetime");
        let signal = plum_blossom_signal(dt).expect("fixed datetime must succeed");
        assert!(signal.transformed.is_some(), "transformed must be Some");
        let moving = signal.moving_line.expect("moving_line must be Some");
        assert!((1..=6).contains(&moving), "moving_line must be 1..=6");
        assert!(
            (signal.original.energy - (signal.original.hexagram as f64 - 31.5)).abs()
                < f64::EPSILON
        );
        assert!(
            (signal.nuclear.energy - (signal.nuclear.hexagram as f64 - 31.5)).abs() < f64::EPSILON
        );
        let transformed = signal.transformed.expect("already checked");
        assert!((transformed.energy - (transformed.hexagram as f64 - 31.5)).abs() < f64::EPSILON);
        let input = calendar::to_plum_blossom_input(dt, LeapMonthPolicy::Reject)
            .expect("adapter must succeed");
        let expected = plum_blossom::compute_channels(input).expect("channels must succeed");
        assert_eq!(signal.original, expected.original);
        assert_eq!(signal.transformed, Some(expected.transformed));
        assert_eq!(signal.nuclear, expected.nuclear);
        assert_eq!(signal.moving_line, Some(expected.moving_line));
    }

    #[test]
    fn facade_leap_reject_maps_to_validation() {
        let leap = find_2020_leap_solar_date();
        let dt = Utc
            .with_ymd_and_hms(leap.year(), leap.month(), leap.day(), 4, 0, 0)
            .single()
            .expect("valid datetime");
        let err = plum_blossom_signal_with_policy(dt, LeapMonthPolicy::Reject)
            .expect_err("leap Reject must fail");
        assert_eq!(err.kind, TaErrorKind::Validation);
        let default_err = plum_blossom_signal(dt).expect_err("default Reject must fail");
        assert_eq!(default_err.kind, TaErrorKind::Validation);
    }

    #[test]
    fn facade_leap_allow_succeeds() {
        let leap = find_2020_leap_solar_date();
        let dt = Utc
            .with_ymd_and_hms(leap.year(), leap.month(), leap.day(), 4, 0, 0)
            .single()
            .expect("valid datetime");
        let signal = plum_blossom_signal_with_policy(dt, LeapMonthPolicy::Allow)
            .expect("leap Allow must succeed");
        assert!(signal.transformed.is_some(), "transformed must be Some");
        let moving = signal.moving_line.expect("moving_line must be Some");
        assert!((1..=6).contains(&moving), "moving_line must be 1..=6");
    }

    #[test]
    fn plum_signal_end_to_end_fixed_instant() {
        use crate::ta::iching::types::{Hexagram, Trigram};
        // 2024-02-10T00:00:00Z -> CST+08 2024-02-10 08:00, lunar 2024/1/1.
        let dt = Utc
            .with_ymd_and_hms(2024, 2, 10, 0, 0, 0)
            .single()
            .expect("valid datetime");
        let (local_date, local_time) = calendar::to_local_datetime(dt);
        assert_eq!(
            local_date,
            NaiveDate::from_ymd_opt(2024, 2, 10).expect("valid date")
        );
        assert_eq!(
            local_time,
            chrono::NaiveTime::from_hms_opt(8, 0, 0).expect("valid time")
        );
        let lunar = calendar::resolve_lunar(local_date, LeapMonthPolicy::Reject)
            .expect("lunar new year must resolve");
        assert_eq!(lunar.lunar_year, 2024);
        assert_eq!(lunar.lunar_month, 1);
        assert_eq!(lunar.lunar_day, 1);
        assert!(!lunar.is_leap_month);
        assert_eq!(
            calendar::year_branch(lunar.lunar_year).expect("valid year"),
            5,
            "lunar year 2024 is Chen (5th branch)"
        );
        assert_eq!(
            calendar::hour_branch(local_time),
            5,
            "08:00 local is Chen (5th branch)"
        );
        let input = calendar::to_plum_blossom_input(dt, LeapMonthPolicy::Reject)
            .expect("adapter must succeed");
        assert_eq!(input.year_branch, 5);
        assert_eq!(input.lunar_month, 1);
        assert_eq!(input.lunar_day, 1);
        assert_eq!(input.hour_branch, 5);
        // Manual Plum Blossom formulas: S1=5+1+1=7, S2=7+5=12.
        let s1: u32 = 5 + 1 + 1;
        let s2: u32 = s1 + 5;
        assert_eq!(s1, 7);
        assert_eq!(s2, 12);
        let upper_num = if s1 % 8 == 0 { 8 } else { (s1 % 8) as u8 };
        let lower_num = if s2 % 8 == 0 { 8 } else { (s2 % 8) as u8 };
        let moving: u8 = if s2 % 6 == 0 { 6 } else { (s2 % 6) as u8 };
        assert_eq!(upper_num, 7);
        assert_eq!(lower_num, 4);
        assert_eq!(moving, 6);
        let upper = Trigram::from_num(upper_num).expect("7 is valid");
        let lower = Trigram::from_num(lower_num).expect("4 is valid");
        assert_eq!(upper.lines(), [0, 0, 1]);
        assert_eq!(lower.lines(), [1, 0, 0]);
        let original_hex = Hexagram::from_trigrams(upper, lower);
        assert_eq!(original_hex.binary_index(), 33);
        assert_eq!(original_hex.bits_top_to_bottom(), "100001");
        let transformed_hex = original_hex.flip_line(moving).expect("line 6 valid");
        assert_eq!(transformed_hex.binary_index(), 1);
        let nuclear_hex = original_hex.nuclear();
        assert_eq!(nuclear_hex.binary_index(), 0);
        assert_eq!(nuclear_hex.bits_top_to_bottom(), "000000");
        // Facade must match the manual computation.
        let signal = plum_blossom_signal(dt).expect("fixed instant must succeed");
        assert_eq!(signal.original.hexagram, 33);
        assert_eq!(signal.original.energy, 1.5);
        assert_eq!(signal.moving_line, Some(6));
        let transformed = signal.transformed.expect("transformed must be Some");
        assert_eq!(transformed.hexagram, 1);
        assert_eq!(transformed.energy, -30.5);
        assert_eq!(signal.nuclear.hexagram, 0);
        assert_eq!(signal.nuclear.energy, -31.5);
        assert!(
            (signal.original.energy - (f64::from(signal.original.hexagram) - 31.5)).abs()
                < f64::EPSILON
        );
        assert!(
            (transformed.energy - (f64::from(transformed.hexagram) - 31.5)).abs() < f64::EPSILON
        );
        assert!(
            (signal.nuclear.energy - (f64::from(signal.nuclear.hexagram) - 31.5)).abs()
                < f64::EPSILON
        );
        let expected = plum_blossom::compute_channels(input).expect("channels must succeed");
        assert_eq!(signal.original, expected.original);
        assert_eq!(signal.transformed, Some(expected.transformed));
        assert_eq!(signal.nuclear, expected.nuclear);
        assert_eq!(signal.moving_line, Some(expected.moving_line));
    }

    #[test]
    fn midpoint_energy_endpoints() {
        let lo = HexagramEnergy::new(0).expect("0 is valid");
        assert_eq!(lo.hexagram, 0);
        assert_eq!(lo.energy, -31.5);
        let hi = HexagramEnergy::new(63).expect("63 is valid");
        assert_eq!(hi.hexagram, 63);
        assert_eq!(hi.energy, 31.5);
    }

    #[test]
    fn midpoint_energy_center_adjacent() {
        let below = HexagramEnergy::new(31).expect("31 is valid");
        assert!((below.energy - (-0.5)).abs() < f64::EPSILON);
        assert!(below.energy < 0.0);
        let above = HexagramEnergy::new(32).expect("32 is valid");
        assert!((above.energy - 0.5).abs() < f64::EPSILON);
        assert!(above.energy > 0.0);
        assert_ne!(below.energy, 0.0);
        assert_ne!(above.energy, 0.0);
    }

    #[test]
    fn plum_channels_always_populated() {
        let dt = chrono::DateTime::parse_from_rfc3339("2024-06-15T12:00:00+08:00")
            .expect("valid rfc3339")
            .with_timezone(&Utc);
        let signal = plum_blossom_signal(dt).expect("valid date must succeed");
        assert!(signal.transformed.is_some());
        assert!(signal.moving_line.is_some());
        let moving = signal.moving_line.expect("already checked");
        assert!((1..=6).contains(&moving));
        assert!(
            (signal.original.energy - (f64::from(signal.original.hexagram) - 31.5)).abs()
                < f64::EPSILON
        );
        assert!(
            (signal.nuclear.energy - (f64::from(signal.nuclear.hexagram) - 31.5)).abs()
                < f64::EPSILON
        );
        let transformed = signal.transformed.expect("already checked");
        assert!(
            (transformed.energy - (f64::from(transformed.hexagram) - 31.5)).abs() < f64::EPSILON
        );
    }

    #[test]
    fn channel_energy_matches_hexagram_field() {
        let dt = Utc
            .with_ymd_and_hms(2023, 8, 8, 14, 30, 0)
            .single()
            .expect("valid datetime");
        let signal = plum_blossom_signal(dt).expect("valid date must succeed");
        assert!(
            (signal.original.energy - f64::from(signal.original.hexagram) + 31.5).abs()
                < f64::EPSILON
        );
        assert!(
            (signal.nuclear.energy - f64::from(signal.nuclear.hexagram) + 31.5).abs()
                < f64::EPSILON
        );
        let transformed = signal.transformed.expect("transformed must be Some");
        assert!((transformed.energy - f64::from(transformed.hexagram) + 31.5).abs() < f64::EPSILON);
    }

    #[test]
    fn leap_reject_propagates_as_validation() {
        let leap = find_2020_leap_solar_date();
        let dt = Utc
            .with_ymd_and_hms(leap.year(), leap.month(), leap.day(), 4, 0, 0)
            .single()
            .expect("valid datetime");
        let err = plum_blossom_signal_with_policy(dt, LeapMonthPolicy::Reject)
            .expect_err("leap Reject must fail");
        assert_eq!(err.kind, TaErrorKind::Validation);
    }

    #[test]
    fn leap_allow_succeeds() {
        let leap = find_2020_leap_solar_date();
        let dt = Utc
            .with_ymd_and_hms(leap.year(), leap.month(), leap.day(), 4, 0, 0)
            .single()
            .expect("valid datetime");
        let signal = plum_blossom_signal_with_policy(dt, LeapMonthPolicy::Allow)
            .expect("leap Allow must succeed");
        assert!(signal.transformed.is_some());
        assert!(signal.moving_line.is_some());
        let moving = signal.moving_line.expect("already checked");
        assert!((1..=6).contains(&moving));
    }

    #[test]
    fn determinism_same_input_same_output() {
        let dt = Utc
            .with_ymd_and_hms(2024, 1, 1, 0, 0, 0)
            .single()
            .expect("valid datetime");
        let a = plum_blossom_signal(dt).expect("first call must succeed");
        let b = plum_blossom_signal(dt).expect("second call must succeed");
        assert_eq!(a, b);
    }
}
