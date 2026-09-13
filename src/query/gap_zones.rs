//! Ephemeral recent-gap-zone relation over a completed source frame.
//!
//! This module materializes the bounded, time-sorted raw zone relation for the
//! proprietary gap-zone signal (see `crate::ta::ops::gap_candidate`). The signal is
//! a proprietary quantized price-movement indicator: it is not conventional
//! support/resistance, not a generic volume profile, and not a filled-gap
//! heuristic. Qualifying candle-body boundaries represent inferred
//! high-confidence concentration boundaries; sparse boundary regions indicate
//! lower trade concentration.
//!
//! Contract:
//!
//! - Input is the SOURCE [`SourceFrame`] (pre-DuckDB projection) carrying the
//!   unconditional scalar candidate columns emitted by the application
//!   aggregates. The projected chart frame deliberately hides those columns.
//! - Only rows with `gap_candidate_qualifies IS TRUE` and `time` strictly
//!   before the supplied decision time participate: the decision candle never
//!   contributes a usable zone to its own decision.
//! - The latest `max_zones` qualifying prior candidates are retained and
//!   returned in ascending chronological order. The relation is bounded and
//!   ephemeral — it exists only for the current analysis invocation and must
//!   not be persisted, cached, or carried as long-lived stream state.
//! - Records are raw: full candle metadata plus candidate body bounds and
//!   direction. No RSSI/trust weighting, no overlap aggregation, no
//!   nearest-limit collapse, and no touch/fill/cross invalidation is applied
//!   here; downstream consumers (charts, LLM tooling) compose the raw zones
//!   with other signals themselves.
//! - Duplicate `time` ties resolve in single-threaded frame insertion order
//!   (the DuckDB scan is single-threaded). This is documented, not handled.
//!
//! SQL trust boundary: the query is source-controlled. The only injected
//! values are `decision_time_ms` and `max_zones`, both rendered as validated
//! integer literals by [`build_recent_zones_sql`]. No caller-owned string or
//! identifier ever reaches the SQL text.
//!
//! # Newcomer guide: how this query flows
//!
//! - Two-stage ordering: SQL first picks the newest `max_zones` rows (`ORDER BY
//!   time DESC LIMIT`), then re-sorts just that small window oldest-first
//!   (`ORDER BY time ASC`). That is why you keep the latest zones but still read
//!   them chronologically.
//! - Required vs optional columns: [`REQUIRED_COLUMNS`] must exist or the call
//!   fails before any SQL runs; `body_ratio` is the one deliberate exception —
//!   it is selected only when the source frame actually carries it.
//! - SQL to Rust: every projected row is decoded by `decode_zone_row` into a
//!   [`GapZoneRecord`]. Timestamps are validated to be exact integers and
//!   direction text maps through a closed three-way match, so bad data fails
//!   fast instead of silently rounding or defaulting.

use crate::engine::error::MarketError;
use crate::engine::frame::SourceFrame;
use crate::engine::traits::ComputedFrame;
use crate::query::RawQuery;
use crate::query::duckdb::DuckDBQuery;

/// Direction of the candle body that created a qualifying gap zone.
/// Newcomer note: this is a closed set — SQL text must be exactly one of
/// `"bullish"`, `"bearish"`, or `"flat"` (see `decode_direction`). Anything
/// else is a contract violation and errors, which keeps new signal variants
/// from slipping through as a wrong default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapZoneDirection {
    /// Candle closed above its open.
    Bullish,
    /// Candle closed below its open.
    Bearish,
    /// Candle closed equal to its open.
    Flat,
}

/// One raw qualifying gap zone with full source candle metadata.
/// Each value comes straight from one decoded SQL row (`decode_zone_row`):
/// numbers stay numbers, `time` becomes an exact `i64`, and direction text
/// becomes [`GapZoneDirection`]. `body_ratio` is `None` when the source frame
/// carries no such column or when the selected value is SQL NULL.
#[derive(Debug, Clone, PartialEq)]
pub struct GapZoneRecord {
    /// Source candle time in epoch milliseconds (exact integer).
    pub time_ms: i64,
    /// Candle open price.
    pub open: f64,
    /// Candle high price.
    pub high: f64,
    /// Candle low price.
    pub low: f64,
    /// Candle close price.
    pub close: f64,
    /// Candle volume.
    pub volume: f64,
    /// Lower boundary of the qualifying candle body.
    pub body_bottom: f64,
    /// Upper boundary of the qualifying candle body.
    pub body_top: f64,
    /// Candle body direction.
    pub direction: GapZoneDirection,
    /// Body ratio when the source frame carries the (config-gated) column.
    pub body_ratio: Option<f64>,
}

/// Bounded, ephemeral, ascending-chronological recent-zone relation.
/// Holds at most `max_zones` rows, already sorted oldest-first so callers can
/// iterate without re-sorting; empty means “no qualifying prior zones”, not an
/// error.
#[derive(Debug, Clone, PartialEq)]
pub struct RecentGapZones {
    /// The retained qualifying prior zones, oldest first.
    pub zones: Vec<GapZoneRecord>,
}

/// Source-frame columns the analyzer requires before executing any SQL.
/// Why check up front: failing here avoids a confusing DuckDB “column not
/// found” error later and proves the frame is a true source frame (with
/// `gap_candidate_*` scalars) rather than an already-projected chart frame.
/// `body_ratio` is intentionally absent — it is optional and gated separately.
const REQUIRED_COLUMNS: [&str; 10] = [
    "time",
    "open",
    "high",
    "low",
    "close",
    "volume",
    "gap_candidate_qualifies",
    "gap_candidate_body_bottom",
    "gap_candidate_body_top",
    "gap_candidate_direction",
];

/// Builds the source-controlled recent-zone SQL.
///
/// Only `decision_time_ms` and `max_zones` are injected, both as integer
/// literals. `body_ratio` enters the projection only when the caller-verified
/// source frame actually carries that (config-gated) column.
///
/// Why two `ORDER BY`s: the inner query sorts newest-first so `LIMIT` keeps the
/// `max_zones` rows closest to (but strictly before) the decision time; the
/// outer query then flips just that bounded window oldest-first for stable
/// chronological output. `time < decision_time_ms` keeps the decision candle
/// itself out of its own zone set.
fn build_recent_zones_sql(
    decision_time_ms: i64,
    max_zones: usize,
    include_body_ratio: bool,
) -> String {
    let ratio_select = if include_body_ratio { ", body_ratio" } else { "" };
    format!(
        "SELECT time, open, high, low, close, volume, \
         gap_candidate_body_bottom AS body_bottom, \
         gap_candidate_body_top AS body_top, \
         gap_candidate_direction AS direction{ratio_select} \
         FROM ( \
           SELECT * FROM computed() \
           WHERE gap_candidate_qualifies IS TRUE AND time < {decision_time_ms} \
           ORDER BY time DESC LIMIT {max_zones} \
         ) ORDER BY time ASC"
    )
}

/// Materializes the latest `max_zones` qualifying gap zones strictly prior to
/// `decision_time_ms` from a completed source frame.
///
/// Returns records in ascending chronological order. A frame with no
/// qualifying prior candidates yields an empty relation, which is a valid
/// domain absence rather than an error. Contract violations (zero capacity,
/// missing required columns, null candidate fields on qualifying rows,
/// undecodable direction text, non-integer timestamps) fail fast.
///
/// Flow for newcomers: validate capacity → probe required columns → decide
/// whether `body_ratio` can be selected → run the two-stage SQL via
/// `DuckDBQuery::project` → decode each projected row into [`GapZoneRecord`].
pub fn recent_gap_zones(
    frame: SourceFrame,
    decision_time_ms: i64,
    max_zones: usize,
) -> Result<RecentGapZones, MarketError> {
    if max_zones == 0 {
        return Err(MarketError::validation(
            "gap-zone max_zones must be greater than zero",
        ));
    }
    let names = frame.column_names();
    for required in REQUIRED_COLUMNS {
        if !names.contains(&required) {
            return Err(MarketError::computation(format!(
                "gap-zone source frame is missing required column {required}"
            )));
        }
    }
    let include_body_ratio = names.contains(&"body_ratio");
    let sql = build_recent_zones_sql(decision_time_ms, max_zones, include_body_ratio);
    let result = DuckDBQuery::new().project(frame, RawQuery::source_controlled(sql))?;

    let mut zones = Vec::with_capacity(result.len());
    for row in 0..result.len() {
        zones.push(decode_zone_row(&result, row, include_body_ratio)?);
    }
    Ok(RecentGapZones { zones })
}

/// Decodes one projected SQL row into the domain [`GapZoneRecord`].
/// Newcomer note: SQL aliases (`body_bottom`, `direction`, …) are what we read
/// here — not the longer `gap_candidate_*` source names. Every required field
/// must be non-null on a qualifying row; `body_ratio` follows the
/// caller-verified `include_body_ratio` flag so we never ask DuckDB for a
/// column it did not project.
fn decode_zone_row(
    frame: &impl ComputedFrame,
    row: usize,
    include_body_ratio: bool,
) -> Result<GapZoneRecord, MarketError> {
    let time_ms = decode_time(frame.f64_at("time", row)?, row)?;
    let open = required_f64(frame, "open", row)?;
    let high = required_f64(frame, "high", row)?;
    let low = required_f64(frame, "low", row)?;
    let close = required_f64(frame, "close", row)?;
    let volume = required_f64(frame, "volume", row)?;
    let body_bottom = required_f64(frame, "body_bottom", row)?;
    let body_top = required_f64(frame, "body_top", row)?;
    let direction = decode_direction(frame.string_at("direction", row)?, row)?;
    let body_ratio = if include_body_ratio {
        frame.f64_at("body_ratio", row)?
    } else {
        None
    };
    Ok(GapZoneRecord {
        time_ms,
        open,
        high,
        low,
        close,
        volume,
        body_bottom,
        body_top,
        direction,
        body_ratio,
    })
}

/// Validates that a DuckDB `DOUBLE` time is an exact millisecond integer.
/// Rejects nulls, infinities/NaN, fractional values, and out-of-`i64` ranges
/// instead of truncating: silent rounding would attach a zone to the wrong
/// candle, so the check fails fast with the row index for debugging.
fn decode_time(value: Option<f64>, row: usize) -> Result<i64, MarketError> {
    let value = value.ok_or_else(|| {
        MarketError::computation(format!("gap-zone row {row} has a null time"))
    })?;
    if !value.is_finite()
        || value.fract() != 0.0
        || value < i64::MIN as f64
        || value > i64::MAX as f64
    {
        return Err(MarketError::computation(format!(
            "gap-zone row {row} time {value} is not an exact integer millisecond timestamp"
        )));
    }
    Ok(value as i64)
}

/// Reads a required numeric field, rejecting nulls on qualifying rows.
/// A qualifying zone without its body bounds or OHLCV metadata is corrupt
/// input, not a defaultable gap — hence an error naming row and column.
fn required_f64(
    frame: &impl ComputedFrame,
    column: &str,
    row: usize,
) -> Result<f64, MarketError> {
    frame.f64_at(column, row)?.ok_or_else(|| {
        MarketError::computation(format!(
            "gap-zone row {row} has a null {column} on a qualifying zone"
        ))
    })
}

/// Maps DuckDB direction text onto the closed [`GapZoneDirection`] set.
/// Only `"bullish"`, `"bearish"`, and `"flat"` are accepted; nulls and any
/// other spelling error with the offending text so a renamed upstream signal
/// cannot silently become the wrong direction.
fn decode_direction(value: Option<String>, row: usize) -> Result<GapZoneDirection, MarketError> {
    let text = value.ok_or_else(|| {
        MarketError::computation(format!(
            "gap-zone row {row} has a null direction on a qualifying zone"
        ))
    })?;
    match text.as_str() {
        "bullish" => Ok(GapZoneDirection::Bullish),
        "bearish" => Ok(GapZoneDirection::Bearish),
        "flat" => Ok(GapZoneDirection::Flat),
        other => Err(MarketError::computation(format!(
            "gap-zone row {row} has unknown direction text \"{other}\""
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::frame::SourceColumnData;

    struct RowSpec {
        time: i64,
        open: f64,
        high: f64,
        low: f64,
        close: f64,
        volume: f64,
        qualifies: bool,
        bottom: Option<f64>,
        top: Option<f64>,
        direction: Option<&'static str>,
        body_ratio: Option<f64>,
    }

    fn spec(
        time: i64,
        qualifies: bool,
        direction: Option<&'static str>,
    ) -> RowSpec {
        let (open, close) = if direction == Some("bearish") {
            (110.0, 100.0)
        } else {
            (100.0, 110.0)
        };
        RowSpec {
            time,
            open,
            high: 115.0,
            low: 95.0,
            close,
            volume: 1_000.0,
            qualifies,
            bottom: qualifies.then_some(open.min(close)),
            top: qualifies.then_some(open.max(close)),
            direction: qualifies.then(|| direction.unwrap_or("bullish")),
            body_ratio: Some(0.8),
        }
    }

    fn frame_from(rows: &[RowSpec], with_body_ratio: bool) -> SourceFrame {
        SourceFrame::from_columns({
            let mut columns = vec![
                (
                    "time".into(),
                    SourceColumnData::Number(rows.iter().map(|row| Some(row.time as f64)).collect()),
                ),
                (
                    "open".into(),
                    SourceColumnData::Number(rows.iter().map(|row| Some(row.open)).collect()),
                ),
                (
                    "high".into(),
                    SourceColumnData::Number(rows.iter().map(|row| Some(row.high)).collect()),
                ),
                (
                    "low".into(),
                    SourceColumnData::Number(rows.iter().map(|row| Some(row.low)).collect()),
                ),
                (
                    "close".into(),
                    SourceColumnData::Number(rows.iter().map(|row| Some(row.close)).collect()),
                ),
                (
                    "volume".into(),
                    SourceColumnData::Number(rows.iter().map(|row| Some(row.volume)).collect()),
                ),
                (
                    "gap_candidate_qualifies".into(),
                    SourceColumnData::Boolean(rows.iter().map(|row| Some(row.qualifies)).collect()),
                ),
                (
                    "gap_candidate_body_bottom".into(),
                    SourceColumnData::Number(rows.iter().map(|row| row.bottom).collect()),
                ),
                (
                    "gap_candidate_body_top".into(),
                    SourceColumnData::Number(rows.iter().map(|row| row.top).collect()),
                ),
                (
                    "gap_candidate_direction".into(),
                    SourceColumnData::Text(
                        rows.iter()
                            .map(|row| row.direction.map(str::to_owned))
                            .collect(),
                    ),
                ),
            ];
            if with_body_ratio {
                columns.push((
                    "body_ratio".into(),
                    SourceColumnData::Number(rows.iter().map(|row| row.body_ratio).collect()),
                ));
            }
            columns
        })
        .expect("fixture frame must be valid")
    }

    fn empty_frame(with_body_ratio: bool) -> SourceFrame {
        frame_from(&[], with_body_ratio)
    }

    #[test]
    fn returns_only_qualifying_strictly_prior_rows_with_full_metadata() {
        let frame = frame_from(
            &[
                spec(1_000, true, Some("bullish")),
                spec(2_000, false, None),
                spec(3_000, true, Some("bearish")),
                spec(4_000, true, Some("bullish")),
            ],
            true,
        );
        let result = recent_gap_zones(frame, 4_000, 64).unwrap();
        assert_eq!(result.zones.len(), 2);
        let first = &result.zones[0];
        assert_eq!(first.time_ms, 1_000);
        assert_eq!(first.open, 100.0);
        assert_eq!(first.high, 115.0);
        assert_eq!(first.low, 95.0);
        assert_eq!(first.close, 110.0);
        assert_eq!(first.volume, 1_000.0);
        assert_eq!(first.body_bottom, 100.0);
        assert_eq!(first.body_top, 110.0);
        assert_eq!(first.direction, GapZoneDirection::Bullish);
        assert_eq!(first.body_ratio, Some(0.8));
        let second = &result.zones[1];
        assert_eq!(second.time_ms, 3_000);
        assert_eq!(second.direction, GapZoneDirection::Bearish);
        assert_eq!(second.open, 110.0);
        assert_eq!(second.close, 100.0);
    }

    #[test]
    fn retains_latest_n_candidates_in_ascending_order() {
        let rows: Vec<RowSpec> = (1..=5)
            .map(|index| spec(index * 1_000, true, Some("bullish")))
            .collect();
        let result = recent_gap_zones(frame_from(&rows, true), 99_000, 3).unwrap();
        assert_eq!(
            result
                .zones
                .iter()
                .map(|zone| zone.time_ms)
                .collect::<Vec<_>>(),
            vec![3_000, 4_000, 5_000]
        );
    }

    #[test]
    fn maps_all_direction_texts_and_rejects_unknown_text() {
        let frame = frame_from(
            &[
                spec(1_000, true, Some("bullish")),
                spec(2_000, true, Some("bearish")),
                spec(3_000, true, Some("flat")),
            ],
            true,
        );
        let result = recent_gap_zones(frame, 9_000, 64).unwrap();
        assert_eq!(
            result
                .zones
                .iter()
                .map(|zone| zone.direction)
                .collect::<Vec<_>>(),
            vec![
                GapZoneDirection::Bullish,
                GapZoneDirection::Bearish,
                GapZoneDirection::Flat,
            ]
        );

        let mut bad = spec(1_000, true, Some("bullish"));
        bad.direction = Some("sideways");
        let error = recent_gap_zones(frame_from(&[bad], true), 9_000, 64)
            .expect_err("unknown direction text must fail");
        assert!(error.message.contains("sideways"));
    }

    #[test]
    fn empty_frames_and_frames_without_candidates_return_empty_zones() {
        let empty = recent_gap_zones(empty_frame(true), 9_000, 64).unwrap();
        assert!(empty.zones.is_empty());
        let no_candidates =
            recent_gap_zones(frame_from(&[spec(1_000, false, None)], true), 9_000, 64).unwrap();
        assert!(no_candidates.zones.is_empty());
        let none_prior =
            recent_gap_zones(frame_from(&[spec(9_000, true, None)], true), 9_000, 64).unwrap();
        assert!(none_prior.zones.is_empty());
    }

    #[test]
    fn zero_max_zones_and_missing_columns_fail_fast() {
        let zero = recent_gap_zones(empty_frame(true), 9_000, 0)
            .expect_err("max_zones == 0 must fail");
        assert_eq!(zero.kind, crate::engine::error::ErrorKind::ValidationError);

        let missing = frame_from(&[spec(1_000, true, None)], true);
        let names = missing.column_names();
        assert!(names.contains(&"time"));
        // Rebuild without the `volume` column to prove the pre-SQL probe.
        let rows = [spec(1_000, true, None)];
        let frame = SourceFrame::from_columns(vec![
            (
                "time".into(),
                SourceColumnData::Number(rows.iter().map(|row| Some(row.time as f64)).collect()),
            ),
            (
                "gap_candidate_qualifies".into(),
                SourceColumnData::Boolean(rows.iter().map(|row| Some(row.qualifies)).collect()),
            ),
            (
                "gap_candidate_body_bottom".into(),
                SourceColumnData::Number(rows.iter().map(|row| row.bottom).collect()),
            ),
            (
                "gap_candidate_body_top".into(),
                SourceColumnData::Number(rows.iter().map(|row| row.top).collect()),
            ),
            (
                "gap_candidate_direction".into(),
                SourceColumnData::Text(
                    rows.iter()
                        .map(|row| row.direction.map(str::to_owned))
                        .collect(),
                ),
            ),
        ])
        .unwrap();
        let error = recent_gap_zones(frame, 9_000, 64).expect_err("missing columns must fail");
        assert!(error.message.contains("missing required column"));
        assert_eq!(missing.len(), 1);
    }

    #[test]
    fn null_candidate_field_on_qualifying_row_errors() {
        let mut row = spec(1_000, true, Some("bullish"));
        row.bottom = None;
        let error = recent_gap_zones(frame_from(&[row], true), 9_000, 64)
            .expect_err("null body_bottom must fail");
        assert!(error.message.contains("body_bottom"));

        let mut null_direction = spec(2_000, true, Some("bullish"));
        null_direction.direction = None;
        let error = recent_gap_zones(frame_from(&[null_direction], true), 9_000, 64)
            .expect_err("null direction must fail");
        assert!(error.message.contains("direction"));
    }

    #[test]
    fn body_ratio_is_included_only_when_the_column_exists() {
        let rows = vec![spec(1_000, true, Some("bullish"))];
        let with = recent_gap_zones(frame_from(&rows, true), 9_000, 64).unwrap();
        assert_eq!(with.zones[0].body_ratio, Some(0.8));
        let without = recent_gap_zones(frame_from(&rows, false), 9_000, 64).unwrap();
        assert_eq!(without.zones[0].body_ratio, None);
    }

    #[test]
    fn repeat_invocation_is_deterministic() {
        let rows: Vec<RowSpec> = (1..=3)
            .map(|index| spec(index * 1_000, true, Some("bullish")))
            .collect();
        let first = recent_gap_zones(frame_from(&rows, true), 99_000, 64).unwrap();
        let second = recent_gap_zones(frame_from(&rows, true), 99_000, 64).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn sql_builder_injects_only_integer_literals() {
        let sql = build_recent_zones_sql(1_700_000_000_000, 64, true);
        assert!(sql.contains("time < 1700000000000"));
        assert!(sql.contains("LIMIT 64"));
        assert!(sql.contains("gap_candidate_qualifies IS TRUE"));
        assert!(sql.contains(", body_ratio"));
        assert!(!sql.contains("1700000000000.0"));
        assert!(!sql.contains("64.0"));

        let without_ratio = build_recent_zones_sql(-5, 1, false);
        assert!(without_ratio.contains("time < -5"));
        assert!(without_ratio.contains("LIMIT 1"));
        assert!(!without_ratio.contains(", body_ratio"));
        assert!(without_ratio.contains("ORDER BY time DESC"));
        assert!(without_ratio.contains("ORDER BY time ASC"));
    }
}
