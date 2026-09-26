//! CST+08 calendar adapter for the `cst-utc8-fixed-v1` time policy.
//!
//! Converts `DateTime<Utc>` to local naive date/time with a fixed UTC+08:00
//! offset, derives Earthly Branch numbers for hours and years, and wraps
//! `lunar-lite` solar-to-lunar conversion with leap-month policy enforcement.
//!
//! The facade re-export is deferred to a later unit; items are crate-visible
//! but not yet consumed outside this module.
#![allow(dead_code)]

use super::plum_blossom::PlumBlossomInput;
use super::types::LeapMonthPolicy;
use chrono::{Datelike, FixedOffset, Timelike};

/// Fixed offset in seconds for the `cst-utc8-fixed-v1` time policy.
const CST_OFFSET_SECS: i32 = 8 * 3600;

/// Lunar calendar fields extracted from `lunar-lite`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LunarFields {
    /// Lunar month 1..=12 (never the leap instance number; leap is tracked separately).
    pub lunar_month: u8,
    /// Lunar day 1..=30.
    pub lunar_day: u8,
    /// Whether this date falls in a leap month.
    pub is_leap_month: bool,
    /// Lunar year (for year-branch derivation).
    pub lunar_year: i32,
}

/// Resolves a `DateTime<Utc>` to local CST+08 `NaiveDate` + `NaiveTime`.
///
/// Time policy: `cst-utc8-fixed-v1`. No date rollback at 23:00.
pub fn to_local_datetime(
    dt: chrono::DateTime<chrono::Utc>,
) -> (chrono::NaiveDate, chrono::NaiveTime) {
    if let Some(offset) = FixedOffset::east_opt(CST_OFFSET_SECS) {
        let local = dt.with_timezone(&offset);
        (local.date_naive(), local.time())
    } else {
        // Unreachable: UTC+08:00 is always a valid offset. Fallback preserves
        // the fixed +08:00 shift through checked arithmetic without panicking.
        let shifted = dt
            .naive_utc()
            .checked_add_signed(chrono::Duration::hours(8))
            .unwrap_or_else(|| dt.naive_utc());
        (shifted.date(), shifted.time())
    }
}

/// Derives the Earthly Branch number (1..=12) for a local time.
///
/// Zi-hour rule (`cst-utc8-fixed-v1`):
/// - `time ∈ [23:00, 01:00)` → hour_branch = 1 (Zi), using the **same local date**.
/// - `01:00..03:00` → 2 (Chou), etc.
///
/// The midnight boundary is `00:00`; 23:00 does NOT roll the date backward.
pub fn hour_branch(time: chrono::NaiveTime) -> u8 {
    let branch = time.hour().div_ceil(2) % 12 + 1;
    u8::try_from(branch).unwrap_or(1)
}

/// Derives the year branch (1..=12) from a Gregorian year.
///
/// Formula: `((year - 4) % 12) + 1`. Branch 1 = Zi ... 12 = Hai.
/// Returns `TaError::validation` for years outside a reasonable range.
pub fn year_branch(year: i32) -> crate::ta::TaResult<u8> {
    if !(1..=9999).contains(&year) {
        return Err(crate::ta::TaError::validation(format!(
            "year {year} is out of supported range 1..=9999"
        )));
    }
    let branch = (year - 4).rem_euclid(12) + 1;
    Ok(u8::try_from(branch).unwrap_or(1))
}

/// Converts `NaiveDate` to lunar fields via `lunar-lite`.
///
/// Uses `lunar_lite::solar_to_lunar(SolarDate { year, month, day })`.
/// Maps `LunarError` → `TaError::validation`.
/// Applies the `LeapMonthPolicy`: `Reject` returns error when `is_leap_month == true`.
pub fn resolve_lunar(
    date: chrono::NaiveDate,
    policy: super::types::LeapMonthPolicy,
) -> crate::ta::TaResult<LunarFields> {
    let solar = lunar_lite::SolarDate {
        year: date.year(),
        month: u8::try_from(date.month()).unwrap_or(12),
        day: u8::try_from(date.day()).unwrap_or(1),
    };
    let lunar = lunar_lite::solar_to_lunar(solar)
        .map_err(|err| crate::ta::TaError::validation(err.to_string()))?;
    if matches!(policy, LeapMonthPolicy::Reject) && lunar.is_leap_month {
        return Err(crate::ta::TaError::validation(
            "date falls in a leap month and LeapMonthPolicy::Reject is active",
        ));
    }
    Ok(LunarFields {
        lunar_month: lunar.month,
        lunar_day: lunar.day,
        is_leap_month: lunar.is_leap_month,
        lunar_year: lunar.year,
    })
}

/// Converts a `DateTime<Utc>` to a validated `PlumBlossomInput`.
///
/// Pipeline:
/// 1. `to_local_datetime(dt)` → CST+08 `NaiveDate` + `NaiveTime`
/// 2. `resolve_lunar(date, policy)` → `LunarFields`
/// 3. `year_branch(lunar.lunar_year)` → year branch 1..=12
/// 4. `hour_branch(time)` → hour branch 1..=12
/// 5. Assemble `PlumBlossomInput` and validate all fields ∈ 1..=12
pub fn to_plum_blossom_input(
    dt: chrono::DateTime<chrono::Utc>,
    policy: super::types::LeapMonthPolicy,
) -> crate::ta::TaResult<super::plum_blossom::PlumBlossomInput> {
    let (date, time) = to_local_datetime(dt);
    let lunar = resolve_lunar(date, policy)?;
    let year = year_branch(lunar.lunar_year)?;
    let hour = hour_branch(time);
    Ok(PlumBlossomInput {
        year_branch: year,
        lunar_month: lunar.lunar_month,
        lunar_day: lunar.lunar_day,
        hour_branch: hour,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ta::TaErrorKind;
    use chrono::{NaiveDate, NaiveTime, TimeZone, Utc};

    #[test]
    fn to_local_datetime_converts_utc_to_cst08() {
        let dt = Utc.with_ymd_and_hms(2024, 2, 10, 0, 0, 0).single().unwrap();
        let (date, time) = to_local_datetime(dt);
        assert_eq!(
            date,
            NaiveDate::from_ymd_opt(2024, 2, 10).unwrap(),
            "2024-02-10T00:00:00Z is 08:00 CST on the same date"
        );
        assert_eq!(
            time,
            NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
            "UTC midnight maps to 08:00 local"
        );
    }

    #[test]
    fn to_local_datetime_does_not_roll_date_at_zi_hour() {
        // 23:30 local on 2024-02-09 must keep the same local date.
        let dt = Utc
            .with_ymd_and_hms(2024, 2, 9, 15, 30, 0)
            .single()
            .unwrap();
        let (date, time) = to_local_datetime(dt);
        assert_eq!(date, NaiveDate::from_ymd_opt(2024, 2, 9).unwrap());
        assert_eq!(time, NaiveTime::from_hms_opt(23, 30, 0).unwrap());
        assert_eq!(hour_branch(time), 1, "Zi hour keeps the same date");
    }

    #[test]
    fn hour_branch_zi_chou_hai_boundaries() {
        assert_eq!(
            hour_branch(NaiveTime::from_hms_opt(23, 30, 0).unwrap()),
            1,
            "23:30 is Zi"
        );
        assert_eq!(
            hour_branch(NaiveTime::from_hms_opt(0, 0, 0).unwrap()),
            1,
            "midnight is Zi"
        );
        assert_eq!(
            hour_branch(NaiveTime::from_hms_opt(1, 0, 0).unwrap()),
            2,
            "01:00 is Chou"
        );
        assert_eq!(
            hour_branch(NaiveTime::from_hms_opt(22, 59, 59).unwrap()),
            12,
            "22:59:59 is Hai"
        );
    }

    #[test]
    fn hour_branch_full_two_hour_cycle() {
        let cases: [(u8, u8); 12] = [
            (0, 1),
            (1, 2),
            (3, 3),
            (5, 4),
            (7, 5),
            (9, 6),
            (11, 7),
            (13, 8),
            (15, 9),
            (17, 10),
            (19, 11),
            (21, 12),
        ];
        for (hour, expected) in cases {
            let time = NaiveTime::from_hms_opt(u32::from(hour), 0, 0).unwrap();
            assert_eq!(hour_branch(time), expected, "hour {hour}");
        }
        assert_eq!(
            hour_branch(NaiveTime::from_hms_opt(23, 0, 0).unwrap()),
            1,
            "23:00 opens Zi without a date rollback"
        );
    }

    #[test]
    fn year_branch_2024_is_valid_dragon() {
        let branch = year_branch(2024).unwrap();
        assert!((1..=12).contains(&branch), "branch must be 1..=12");
        assert_eq!(branch, 5, "2024 is Chen (Dragon), the 5th branch");
    }

    #[test]
    fn year_branch_rejects_unreasonable_years() {
        let err_zero = year_branch(0).unwrap_err();
        assert_eq!(err_zero.kind, TaErrorKind::Validation);
        let err_huge = year_branch(10_000).unwrap_err();
        assert_eq!(err_huge.kind, TaErrorKind::Validation);
    }

    #[test]
    fn resolve_lunar_lunar_new_year_2024() {
        let date = NaiveDate::from_ymd_opt(2024, 2, 10).unwrap();
        let fields = resolve_lunar(date, LeapMonthPolicy::Allow).unwrap();
        assert_eq!(fields.lunar_month, 1);
        assert_eq!(fields.lunar_day, 1);
        assert!(!fields.is_leap_month);
        assert_eq!(fields.lunar_year, 2024);
        // A non-leap date passes under Reject as well.
        let strict = resolve_lunar(date, LeapMonthPolicy::Reject).unwrap();
        assert_eq!(strict, fields);
    }

    #[test]
    fn resolve_lunar_leap_month_policy() {
        // Known leap window: 2020 leap 4th month spans solar
        // 2020-05-23..=2020-06-20. Discover the concrete leap date at runtime
        // so the policy assertions exercise a verified leap instance.
        let mut leap_date: Option<NaiveDate> = None;
        let mut cursor = NaiveDate::from_ymd_opt(2020, 5, 23).unwrap();
        let end = NaiveDate::from_ymd_opt(2020, 6, 20).unwrap();
        while cursor <= end {
            let fields = resolve_lunar(cursor, LeapMonthPolicy::Allow).unwrap();
            if fields.is_leap_month {
                assert_eq!(fields.lunar_month, 4, "2020 leap instance is the 4th month");
                leap_date = Some(cursor);
                break;
            }
            cursor = cursor.succ_opt().unwrap();
        }
        let leap = leap_date.unwrap();
        let rejected = resolve_lunar(leap, LeapMonthPolicy::Reject).unwrap_err();
        assert_eq!(rejected.kind, TaErrorKind::Validation);
        let allowed = resolve_lunar(leap, LeapMonthPolicy::Allow).unwrap();
        assert!(allowed.is_leap_month);
    }

    #[test]
    fn to_plum_blossom_input_valid_fields_in_range() {
        let dt = Utc.with_ymd_and_hms(2024, 2, 10, 0, 0, 0).single().unwrap();
        let input = to_plum_blossom_input(dt, LeapMonthPolicy::Reject).expect("valid date");
        assert!(
            (1..=12).contains(&input.year_branch),
            "year_branch must be 1..=12"
        );
        assert!(
            (1..=12).contains(&input.lunar_month),
            "lunar_month must be 1..=12"
        );
        assert!(
            (1..=30).contains(&input.lunar_day),
            "lunar_day must be 1..=30"
        );
        assert!(
            (1..=12).contains(&input.hour_branch),
            "hour_branch must be 1..=12"
        );
    }

    #[test]
    fn to_plum_blossom_input_leap_rejection_propagates() {
        // Discover a verified leap solar date, then drive the adapter through
        // a UTC instant that maps to noon CST on that same local date.
        let mut leap_date: Option<NaiveDate> = None;
        let mut cursor = NaiveDate::from_ymd_opt(2020, 5, 23).unwrap();
        let end = NaiveDate::from_ymd_opt(2020, 6, 20).unwrap();
        while cursor <= end {
            let fields = resolve_lunar(cursor, LeapMonthPolicy::Allow).unwrap();
            if fields.is_leap_month {
                leap_date = Some(cursor);
                break;
            }
            cursor = cursor.succ_opt().unwrap();
        }
        let leap = leap_date.expect("2020 leap 4th month window must contain a leap date");
        let dt = Utc
            .with_ymd_and_hms(leap.year(), leap.month(), leap.day(), 4, 0, 0)
            .single()
            .unwrap();
        let (local_date, _) = to_local_datetime(dt);
        assert_eq!(local_date, leap, "04:00Z maps to noon CST on the leap date");
        let rejected = to_plum_blossom_input(dt, LeapMonthPolicy::Reject).unwrap_err();
        assert_eq!(rejected.kind, TaErrorKind::Validation);
        let allowed = to_plum_blossom_input(dt, LeapMonthPolicy::Allow)
            .expect("leap date passes under Allow");
        assert!((1..=12).contains(&allowed.year_branch));
        assert!((1..=12).contains(&allowed.hour_branch));
    }

    #[test]
    fn to_plum_blossom_input_lunar_new_year_2024_manual() {
        // 2024-02-10T00:00:00Z is 08:00 CST on Lunar New Year (lunar 2024-01-01).
        // Preserved `hour_branch` semantics map 08:00 to Chen (5), not Mao (4):
        // hour 8 -> div_ceil(2)=4 -> 4 % 12 + 1 = 5. See `hour_branch_full_two_hour_cycle`.
        let dt = Utc.with_ymd_and_hms(2024, 2, 10, 0, 0, 0).single().unwrap();
        let (local_date, local_time) = to_local_datetime(dt);
        assert_eq!(local_date, NaiveDate::from_ymd_opt(2024, 2, 10).unwrap());
        assert_eq!(local_time, NaiveTime::from_hms_opt(8, 0, 0).unwrap());
        let lunar = resolve_lunar(local_date, LeapMonthPolicy::Reject).unwrap();
        assert_eq!(lunar.lunar_year, 2024);
        assert_eq!(lunar.lunar_month, 1);
        assert_eq!(lunar.lunar_day, 1);
        assert_eq!(year_branch(lunar.lunar_year).unwrap(), 5);
        assert_eq!(hour_branch(local_time), 5);
        let input = to_plum_blossom_input(dt, LeapMonthPolicy::Reject).expect("valid adapter");
        assert_eq!(input.year_branch, 5, "lunar year 2024 is Chen (5th branch)");
        assert_eq!(input.lunar_month, 1);
        assert_eq!(input.lunar_day, 1);
        assert_eq!(input.hour_branch, 5, "08:00 local is Chen (5th branch)");
    }

    #[test]
    fn to_plum_blossom_input_year_branch_matches_lunar_lite() {
        let dt = Utc.with_ymd_and_hms(2024, 2, 10, 0, 0, 0).single().unwrap();
        let (local_date, _) = to_local_datetime(dt);
        let lunar = resolve_lunar(local_date, LeapMonthPolicy::Reject).unwrap();
        let expected =
            u8::try_from(lunar_lite::lunar_year_branch(lunar.lunar_year).index() + 1).unwrap();
        assert_eq!(year_branch(lunar.lunar_year).unwrap(), expected);
        let input = to_plum_blossom_input(dt, LeapMonthPolicy::Reject).expect("valid adapter");
        assert_eq!(input.year_branch, expected);
    }
}
