use chrono::{DateTime, Datelike, Duration, Local, Timelike};

pub fn is_time_multiple_of_period(period: Duration, now: DateTime<Local>) -> bool {
    // Calculate the total seconds from the start of the day
    let total_seconds_since_midnight = now.num_seconds_from_midnight() as i64;

    // Convert the period to seconds
    let period_seconds = period.num_seconds();

    // Check if the total seconds is a multiple of the period seconds
    if period_seconds == 0 {
        return false; // Avoid division by zero
    }

    total_seconds_since_midnight % period_seconds == 0
}

pub fn is_closing_timeframe(tf: &str, now: DateTime<Local>) -> Result<bool, String> {
    let (value_str, unit) = tf.split_at(tf.len() - 1);
    let value: i64 = value_str
        .parse()
        .map_err(|_| format!("Invalid timeframe value: {}", value_str))?;

    match unit {
        "m" => Ok(is_time_multiple_of_period(Duration::minutes(value), now)),
        "h" => Ok(is_time_multiple_of_period(Duration::hours(value), now)),
        "d" => Ok(is_time_multiple_of_period(Duration::days(value), now)),
        "w" => Ok(is_time_multiple_of_period(Duration::weeks(value), now)),
        "M" => Ok(now.day() == 1
            && now.num_seconds_from_midnight() < 60
            && (now.month() as i64 % value == 0)),
        _ => Err(format!("Invalid timeframe unit: {}", unit)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_is_closing_timeframe() {
        let dt_jan_1_00_00 = Local.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        let dt_jan_1_00_01 = Local.with_ymd_and_hms(2025, 1, 1, 0, 0, 1).unwrap();
        let dt_jan_2_00_00 = Local.with_ymd_and_hms(2025, 1, 2, 0, 0, 0).unwrap();
        let dt_feb_1_00_00 = Local.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
        let dt_mar_1_00_00 = Local.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap();

        // Test 'M' (month)
        assert!(is_closing_timeframe("1M", dt_jan_1_00_00).unwrap());
        assert!(!is_closing_timeframe("1M", dt_jan_1_00_01).unwrap());
        assert!(!is_closing_timeframe("1M", dt_jan_2_00_00).unwrap());
        assert!(is_closing_timeframe("1M", dt_feb_1_00_00).unwrap());

        assert!(is_closing_timeframe("2M", dt_feb_1_00_00).unwrap()); // Feb is 2nd month
        assert!(!is_closing_timeframe("2M", dt_mar_1_00_00).unwrap()); // Mar is 3rd month

        let dt_apr_1_00_00 = Local.with_ymd_and_hms(2025, 4, 1, 0, 0, 0).unwrap();
        let dt_jun_1_00_00 = Local.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
        let dt_aug_1_00_00 = Local.with_ymd_and_hms(2025, 8, 1, 0, 0, 0).unwrap();
        let dt_sep_1_00_00 = Local.with_ymd_and_hms(2025, 9, 1, 0, 0, 0).unwrap();
        let dt_dec_1_00_00 = Local.with_ymd_and_hms(2025, 12, 1, 0, 0, 0).unwrap();

        assert!(is_closing_timeframe("3M", dt_mar_1_00_00).unwrap());
        assert!(!is_closing_timeframe("3M", dt_apr_1_00_00).unwrap());
        assert!(is_closing_timeframe("3M", dt_jun_1_00_00).unwrap());
        assert!(is_closing_timeframe("3M", dt_sep_1_00_00).unwrap());
        assert!(is_closing_timeframe("3M", dt_dec_1_00_00).unwrap());

        assert!(is_closing_timeframe("4M", dt_apr_1_00_00).unwrap());
        assert!(!is_closing_timeframe("4M", dt_jun_1_00_00).unwrap());
        assert!(is_closing_timeframe("4M", dt_aug_1_00_00).unwrap());
        assert!(is_closing_timeframe("4M", dt_dec_1_00_00).unwrap());

        // Test 'm' (minutes)
        let dt_00_15_00 = Local.with_ymd_and_hms(2025, 1, 1, 0, 15, 0).unwrap();
        let dt_00_15_01 = Local.with_ymd_and_hms(2025, 1, 1, 0, 15, 1).unwrap();
        assert!(is_closing_timeframe("15m", dt_00_15_00).unwrap());
        assert!(!is_closing_timeframe("15m", dt_00_15_01).unwrap());

        // Test 'h' (hours)
        let dt_04_00_00 = Local.with_ymd_and_hms(2025, 1, 1, 4, 0, 0).unwrap();
        let dt_04_00_01 = Local.with_ymd_and_hms(2025, 1, 1, 4, 0, 1).unwrap();
        assert!(is_closing_timeframe("4h", dt_04_00_00).unwrap());
        assert!(!is_closing_timeframe("4h", dt_04_00_01).unwrap());

        // Test 'd' (days)
        let dt_jan_1_00_00 = Local.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        let dt_jan_2_00_00 = Local.with_ymd_and_hms(2025, 1, 2, 0, 0, 0).unwrap();
        assert!(is_closing_timeframe("1d", dt_jan_1_00_00).unwrap());
        assert!(is_closing_timeframe("1d", dt_jan_2_00_00).unwrap());

        // Test 'w' (weeks) - This is tricky as weeks start on different days
        // For simplicity, we'll assume a week starts on Monday (ISO 8601)
        let dt_jan_6_00_00 = Local.with_ymd_and_hms(2025, 1, 6, 0, 0, 0).unwrap(); // Monday
        let dt_jan_13_00_00 = Local.with_ymd_and_hms(2025, 1, 13, 0, 0, 0).unwrap(); // Monday
        assert!(is_closing_timeframe("1w", dt_jan_6_00_00).unwrap());
        assert!(is_closing_timeframe("1w", dt_jan_13_00_00).unwrap());
        assert!(!is_closing_timeframe("1w", dt_jan_1_00_00).unwrap()); // Not a Monday
    }
}
