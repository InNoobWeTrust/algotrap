//! ATR Gap Zones — detect abnormal candle gaps and report overlap density.
//!
//! A "gap zone" is the body range (min/max of open, close) of a candle that
//! closes outside its ATR band. Overlapping gap zones at a given price identify
//! high-conviction support/resistance levels.

use polars::prelude::*;
use serde::{Deserialize, Serialize};

use super::prelude::*;

/// A single detected gap zone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GapZone {
    /// Bottom of the gap zone (min of open, close).
    pub bottom: f64,
    /// Top of the gap zone (max of open, close).
    pub top: f64,
    /// Body-to-wick ratio: `|close - open| / (high - low)`, range [0, 1].
    pub trust: f64,
    /// Number of bars since this gap was detected (0 = current bar).
    pub age_bars: usize,
}

/// Overlap density at a specific price point.
#[derive(Debug, Clone)]
pub struct OverlapDensity {
    /// Number of gap zones overlapping at the price.
    pub count: usize,
    /// Sum of trust scores of overlapping zones.
    pub weighted_trust: f64,
}

/// Parameters for gap zone detection.
#[derive(Debug, Clone)]
pub struct GapZoneParams {
    /// ATR lookback period for "normal" range.
    pub atr_period: usize,
    /// Maximum number of gap zones to retain.
    pub max_zones: usize,
    /// Minimum body/wick ratio to record a gap.
    pub min_trust: f64,
}

impl Default for GapZoneParams {
    fn default() -> Self {
        Self {
            atr_period: 42,
            max_zones: 50,
            min_trust: 0.3,
        }
    }
}

/// Lazy expression: is this candle an ATR gap?
/// Returns a boolean expression: `close > open + atr` OR `close < open - atr`.
/// Composes from the existing `atr()` in `ta/volatility.rs`.
pub fn is_atr_gap(ohlc: &Ohlc, atr_period: usize) -> Expr {
    let current_atr = atr(ohlc, atr_period);
    let upper = ohlc[0].clone() + current_atr.clone();
    let lower = ohlc[0].clone() - current_atr;
    ohlc[3].clone().gt(upper).or(ohlc[3].clone().lt(lower))
}

/// Lazy expression: body-to-wick ratio.
/// Returns `abs(close - open) / (high - low)`, 0 if zero-range candle.
pub fn body_ratio(ohlc: &Ohlc) -> Expr {
    let body = (ohlc[3].clone() - ohlc[0].clone()).abs();
    let wick = ohlc[1].clone() - ohlc[2].clone();
    when(wick.clone().gt(lit(0.0)))
        .then(body / wick)
        .otherwise(lit(0.0))
}

/// Trait for gap zone lazy expressions on OHLC arrays.
pub trait OhlcGapZones {
    fn is_atr_gap(&self, atr_period: usize) -> Expr;
    fn body_ratio(&self) -> Expr;
}

impl OhlcGapZones for Ohlc {
    fn is_atr_gap(&self, atr_period: usize) -> Expr {
        is_atr_gap(self, atr_period)
    }
    fn body_ratio(&self) -> Expr {
        body_ratio(self)
    }
}

/// Extract gap zones from a materialized DataFrame.
///
/// Reads pre-computed `is_atr_gap` and `body_ratio` columns, plus `rssi` for
/// composite trust scoring. This runs on filtered output (~10-50 rows).
pub fn extract_gap_zones(df: &DataFrame, params: &GapZoneParams) -> Vec<GapZone> {
    let n = df.height();
    if n == 0 {
        return vec![];
    }

    let is_gap = match df.column("is_atr_gap").ok().and_then(|c| c.bool().ok()) {
        Some(col) => col.clone(),
        None => return vec![],
    };
    let br = match df.column("body_ratio").ok().and_then(|c| c.f64().ok()) {
        Some(col) => col.clone(),
        None => return vec![],
    };
    let opens = match df.column("open").ok().and_then(|c| c.f64().ok()) {
        Some(col) => col.clone(),
        None => return vec![],
    };
    let closes = match df.column("close").ok().and_then(|c| c.f64().ok()) {
        Some(col) => col.clone(),
        None => return vec![],
    };
    // RSSI for composite trust — optional (degrades gracefully)
    let rssi_col = df.column("rssi").ok().and_then(|c| c.f64().ok()).cloned();

    let mut zones: Vec<GapZone> = Vec::new();

    for i in 0..n {
        let gap = is_gap.get(i).unwrap_or(false);
        if !gap {
            continue;
        }

        let ratio = br.get(i).unwrap_or(0.0);
        if ratio < params.min_trust {
            continue;
        }

        let open = opens.get(i).unwrap_or(0.0);
        let close = closes.get(i).unwrap_or(0.0);
        let bottom = open.min(close);
        let top = open.max(close);
        let age_bars = n - 1 - i;

        // Composite trust: body_ratio * (0.5 + 0.5 * rssi_strength)
        let trust = match &rssi_col {
            Some(rssi) => {
                let rssi_val = rssi.get(i).unwrap_or(50.0);
                let rssi_strength = (rssi_val - 50.0).abs() / 50.0;
                ratio * (0.5 + 0.5 * rssi_strength)
            }
            None => ratio, // No RSSI available — fall back to body_ratio alone
        };

        zones.push(GapZone {
            bottom,
            top,
            trust,
            age_bars,
        });
    }

    // Enforce max_zones — keep most recent (tail)
    if zones.len() > params.max_zones {
        zones = zones.split_off(zones.len() - params.max_zones);
    }

    // Reverse so most recent is first
    zones.reverse();
    zones
}

/// Compute overlap density at a specific price point.
pub fn overlap_density(zones: &[GapZone], price: f64) -> OverlapDensity {
    let mut count = 0;
    let mut weighted_trust = 0.0;

    for zone in zones {
        if zone.bottom <= price && price <= zone.top {
            count += 1;
            weighted_trust += zone.trust;
        }
    }

    OverlapDensity {
        count,
        weighted_trust,
    }
}

/// Summary of gap zones relative to current price — for LLM context.
#[derive(Debug, Clone)]
pub struct GapZoneSummary {
    pub zones_above: usize,
    pub zones_below: usize,
    pub overlap_at_price: OverlapDensity,
    pub nearest_gap: Option<(f64, f64, f64)>, // (bottom, top, trust)
}

/// Compute a summary of gap zones relative to current price.
pub fn gap_zone_summary(zones: &[GapZone], current_price: f64) -> GapZoneSummary {
    let overlap = overlap_density(zones, current_price);

    let mut above = 0;
    let mut below = 0;
    let mut nearest: Option<(f64, f64, f64, f64)> = None; // (bottom, top, trust, distance)

    for zone in zones {
        let midpoint = (zone.bottom + zone.top) / 2.0;
        if midpoint > current_price {
            above += 1;
        } else {
            below += 1;
        }

        // Distance from price to nearest edge of zone
        let dist = if current_price < zone.bottom {
            zone.bottom - current_price
        } else if current_price > zone.top {
            current_price - zone.top
        } else {
            0.0 // Inside the zone
        };

        match nearest {
            Some((_, _, _, d)) if dist < d => {
                nearest = Some((zone.bottom, zone.top, zone.trust, dist));
            }
            None => {
                nearest = Some((zone.bottom, zone.top, zone.trust, dist));
            }
            _ => {}
        }
    }

    GapZoneSummary {
        zones_above: above,
        zones_below: below,
        overlap_at_price: overlap,
        nearest_gap: nearest.map(|(b, t, tr, _)| (b, t, tr)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_params(atr_period: usize, max_zones: usize, min_trust: f64) -> GapZoneParams {
        GapZoneParams {
            atr_period,
            max_zones,
            min_trust,
        }
    }

    /// Helper: build a DataFrame with OHLC columns and computed indicators.
    fn build_df(
        opens: &[f64],
        highs: &[f64],
        lows: &[f64],
        closes: &[f64],
        atr_period: usize,
    ) -> DataFrame {
        let df = DataFrame::new(vec![
            Column::new("open".into(), opens),
            Column::new("high".into(), highs),
            Column::new("low".into(), lows),
            Column::new("close".into(), closes),
        ])
        .unwrap();

        let ohlc: Ohlc = [col("open"), col("high"), col("low"), col("close")];
        df.lazy()
            .with_columns([
                is_atr_gap(&ohlc, atr_period).alias("is_atr_gap"),
                body_ratio(&ohlc).alias("body_ratio"),
            ])
            .collect()
            .unwrap()
    }

    /// Helper: build a DataFrame with OHLC + rssi columns for composite trust tests.
    fn build_df_with_rssi(
        opens: &[f64],
        highs: &[f64],
        lows: &[f64],
        closes: &[f64],
        rssi: &[f64],
        atr_period: usize,
    ) -> DataFrame {
        let df = DataFrame::new(vec![
            Column::new("open".into(), opens),
            Column::new("high".into(), highs),
            Column::new("low".into(), lows),
            Column::new("close".into(), closes),
            Column::new("rssi".into(), rssi),
        ])
        .unwrap();

        let ohlc: Ohlc = [col("open"), col("high"), col("low"), col("close")];
        df.lazy()
            .with_columns([
                is_atr_gap(&ohlc, atr_period).alias("is_atr_gap"),
                body_ratio(&ohlc).alias("body_ratio"),
            ])
            .collect()
            .unwrap()
    }

    #[test]
    fn test_bullish_gap() {
        let n = 50;
        let mut opens = vec![86000.0; n];
        let mut highs = vec![86200.0; n];
        let mut lows = vec![85800.0; n];
        let mut closes = vec![86100.0; n];
        opens[n - 1] = 86000.0;
        highs[n - 1] = 87200.0;
        lows[n - 1] = 85900.0;
        closes[n - 1] = 87100.0;

        let df = build_df(&opens, &highs, &lows, &closes, 10);
        let params = make_params(10, 50, 0.0);
        let zones = extract_gap_zones(&df, &params);

        assert!(!zones.is_empty(), "Should detect at least one gap");
        let last = &zones[0];
        assert_eq!(last.age_bars, 0);
        assert!(last.bottom < last.top);
        assert_eq!(last.bottom, 86000.0);
        assert_eq!(last.top, 87100.0);
    }

    #[test]
    fn test_bearish_gap() {
        let n = 50;
        let mut opens = vec![87000.0; n];
        let mut highs = vec![87200.0; n];
        let mut lows = vec![86800.0; n];
        let mut closes = vec![87000.0; n];
        opens[n - 1] = 87000.0;
        highs[n - 1] = 87100.0;
        lows[n - 1] = 85800.0;
        closes[n - 1] = 85900.0;

        let df = build_df(&opens, &highs, &lows, &closes, 10);
        let params = make_params(10, 50, 0.0);
        let zones = extract_gap_zones(&df, &params);

        assert!(!zones.is_empty());
        let last = &zones[0];
        assert_eq!(last.bottom, 85900.0);
        assert_eq!(last.top, 87000.0);
    }

    #[test]
    fn test_normal_candle_no_gap() {
        let n = 50;
        let opens = vec![87000.0; n];
        let highs = vec![87200.0; n];
        let lows = vec![86800.0; n];
        let closes = vec![87050.0; n];

        let df = build_df(&opens, &highs, &lows, &closes, 10);
        let params = make_params(10, 50, 0.0);
        let zones = extract_gap_zones(&df, &params);
        assert!(zones.is_empty());
    }

    #[test]
    fn test_queue_limit() {
        let n = 200;
        let opens = vec![86000.0; n];
        let mut highs = vec![86200.0; n];
        let lows = vec![85800.0; n];
        let mut closes = vec![86100.0; n];
        for i in 15..n {
            closes[i] = 88000.0;
            highs[i] = 88100.0;
        }

        let df = build_df(&opens, &highs, &lows, &closes, 10);
        let params = make_params(10, 50, 0.0);
        let zones = extract_gap_zones(&df, &params);
        assert!(zones.len() <= 50, "Queue should be limited to 50");
    }

    #[test]
    fn test_overlap_density() {
        let zones = vec![
            GapZone {
                bottom: 86000.0,
                top: 86500.0,
                trust: 0.9,
                age_bars: 10,
            },
            GapZone {
                bottom: 86200.0,
                top: 86800.0,
                trust: 0.7,
                age_bars: 5,
            },
            GapZone {
                bottom: 87000.0,
                top: 87500.0,
                trust: 0.8,
                age_bars: 2,
            },
        ];
        let density = overlap_density(&zones, 86300.0);
        assert_eq!(density.count, 2);
        assert!((density.weighted_trust - 1.6).abs() < 0.01);
    }

    #[test]
    fn test_no_overlap() {
        let zones = vec![
            GapZone {
                bottom: 86000.0,
                top: 86500.0,
                trust: 0.9,
                age_bars: 10,
            },
            GapZone {
                bottom: 87000.0,
                top: 87500.0,
                trust: 0.8,
                age_bars: 2,
            },
        ];
        let density = overlap_density(&zones, 86700.0);
        assert_eq!(density.count, 0);
    }

    #[test]
    fn test_empty_history() {
        let opens = vec![87000.0; 5];
        let highs = vec![87200.0; 5];
        let lows = vec![86800.0; 5];
        let closes = vec![87050.0; 5];

        let df = build_df(&opens, &highs, &lows, &closes, 42);
        let params = make_params(42, 50, 0.3);
        let zones = extract_gap_zones(&df, &params);
        assert!(zones.is_empty());
    }

    #[test]
    fn test_min_trust_filter() {
        let n = 50;
        let mut opens = vec![86500.0; n];
        let mut highs = vec![86700.0; n];
        let mut lows = vec![86300.0; n];
        let mut closes = vec![86600.0; n];
        opens[n - 1] = 86500.0;
        highs[n - 1] = 87500.0;
        lows[n - 1] = 85500.0;
        closes[n - 1] = 87200.0;

        let df = build_df(&opens, &highs, &lows, &closes, 10);
        let params = make_params(10, 50, 0.5);
        let zones = extract_gap_zones(&df, &params);
        let recent = zones.iter().find(|z| z.age_bars == 0);
        assert!(recent.is_none(), "Low trust gap should be filtered");
    }

    #[test]
    fn test_gap_zone_summary() {
        let zones = vec![
            GapZone {
                bottom: 86200.0,
                top: 86500.0,
                trust: 0.85,
                age_bars: 3,
            },
            GapZone {
                bottom: 86800.0,
                top: 87100.0,
                trust: 0.9,
                age_bars: 5,
            },
            GapZone {
                bottom: 87200.0,
                top: 87600.0,
                trust: 0.7,
                age_bars: 8,
            },
            GapZone {
                bottom: 85500.0,
                top: 85800.0,
                trust: 0.8,
                age_bars: 12,
            },
        ];
        let summary = gap_zone_summary(&zones, 86600.0);
        assert_eq!(summary.zones_above, 2);
        assert_eq!(summary.zones_below, 2);
        assert!(summary.nearest_gap.is_some());
        let (b, t, _) = summary.nearest_gap.unwrap();
        assert_eq!(b, 86200.0);
        assert_eq!(t, 86500.0);
    }

    #[test]
    fn test_composite_trust_extreme_rssi() {
        // Gap with RSSI at 80 (extreme) should have higher trust than neutral RSSI
        let n = 50;
        let mut opens = vec![86000.0; n];
        let mut highs = vec![86200.0; n];
        let mut lows = vec![85800.0; n];
        let mut closes = vec![86100.0; n];
        let mut rssi = vec![50.0; n]; // neutral
        opens[n - 1] = 86000.0;
        highs[n - 1] = 87200.0;
        lows[n - 1] = 85900.0;
        closes[n - 1] = 87100.0;
        rssi[n - 1] = 80.0; // extreme

        let df = build_df_with_rssi(&opens, &highs, &lows, &closes, &rssi, 10);
        let params = make_params(10, 50, 0.0);
        let zones = extract_gap_zones(&df, &params);

        assert!(!zones.is_empty());
        let gap = &zones[0];
        // rssi_strength = |80-50|/50 = 0.6
        // trust = body_ratio * (0.5 + 0.5 * 0.6) = body_ratio * 0.8
        let expected_body_ratio = (87100.0 - 86000.0) / (87200.0 - 85900.0);
        let expected_trust = expected_body_ratio * 0.8;
        assert!(
            (gap.trust - expected_trust).abs() < 0.02,
            "Expected trust ~{expected_trust:.4}, got {:.4}",
            gap.trust
        );
    }

    #[test]
    fn test_composite_trust_neutral_rssi() {
        let n = 50;
        let mut opens = vec![86000.0; n];
        let mut highs = vec![86200.0; n];
        let mut lows = vec![85800.0; n];
        let mut closes = vec![86100.0; n];
        let rssi = vec![50.0; n]; // neutral everywhere
        opens[n - 1] = 86000.0;
        highs[n - 1] = 87200.0;
        lows[n - 1] = 85900.0;
        closes[n - 1] = 87100.0;

        let df = build_df_with_rssi(&opens, &highs, &lows, &closes, &rssi, 10);
        let params = make_params(10, 50, 0.0);
        let zones = extract_gap_zones(&df, &params);

        assert!(!zones.is_empty());
        let gap = &zones[0];
        // rssi_strength = 0, trust = body_ratio * 0.5
        let expected_body_ratio = (87100.0 - 86000.0) / (87200.0 - 85900.0);
        let expected_trust = expected_body_ratio * 0.5;
        assert!(
            (gap.trust - expected_trust).abs() < 0.02,
            "Expected trust ~{expected_trust:.4}, got {:.4}",
            gap.trust
        );
    }
}
