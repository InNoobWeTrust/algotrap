//! ATR Gap Zones — detect abnormal candle gaps and report overlap density.
//!
//! A "gap zone" is the body range (min/max of open, close) of a candle that
//! closes outside its ATR band. Overlapping gap zones at a given price identify
//! high-conviction support/resistance levels.

use serde::{Deserialize, Serialize};

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

/// Detect gap zones from OHLC data.
///
/// This is a stateful computation — each call recomputes from the full series.
/// ATR is computed using a simple rolling average of true range (matching the
/// algotrap ATR implementation behavior).
///
/// Returns a Vec of GapZones sorted by age (most recent first).
pub fn detect_gap_zones(
    opens: &[f64],
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    params: &GapZoneParams,
) -> Vec<GapZone> {
    let n = opens.len();
    if n < params.atr_period + 1 {
        return vec![];
    }

    // Compute true range series
    let mut tr = vec![0.0; n];
    tr[0] = highs[0] - lows[0]; // First bar: just HL range
    for i in 1..n {
        let hl = highs[i] - lows[i];
        let hc = (highs[i] - closes[i - 1]).abs();
        let lc = (lows[i] - closes[i - 1]).abs();
        tr[i] = hl.max(hc).max(lc);
    }

    // Compute ATR using RMA (recursive moving average)
    let mut atr = vec![0.0; n];
    // Seed: simple average of first `atr_period` true ranges
    let seed: f64 = tr[..params.atr_period].iter().sum::<f64>() / params.atr_period as f64;
    atr[params.atr_period - 1] = seed;
    let alpha = 1.0 / params.atr_period as f64;
    for i in params.atr_period..n {
        atr[i] = alpha * tr[i] + (1.0 - alpha) * atr[i - 1];
    }

    // Scan for abnormal candles starting from where ATR is valid
    let mut zones: Vec<GapZone> = Vec::new();
    for i in params.atr_period..n {
        let open = opens[i];
        let high = highs[i];
        let low = lows[i];
        let close = closes[i];
        let current_atr = atr[i];

        // Abnormal: close outside ATR band around open
        let upper = open + current_atr;
        let lower = open - current_atr;
        let is_abnormal = close > upper || close < lower;

        if !is_abnormal {
            continue;
        }

        // Trust score: body / wick ratio
        let wick = high - low;
        let trust = if wick > 0.0 {
            ((close - open).abs() / wick).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // Filter by min_trust
        if trust < params.min_trust {
            continue;
        }

        let bottom = open.min(close);
        let top = open.max(close);
        let age_bars = n - 1 - i; // Distance from last bar

        zones.push(GapZone {
            bottom,
            top,
            trust,
            age_bars,
        });
    }

    // Enforce max_zones — keep most recent (tail of the list)
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
        GapZoneParams { atr_period, max_zones, min_trust }
    }

    #[test]
    fn test_bullish_gap() {
        // Scenario 1: Bullish gap — close above upper band
        let n = 50;
        let mut opens = vec![86000.0; n];
        let mut highs = vec![86200.0; n];
        let mut lows = vec![85800.0; n];
        let mut closes = vec![86100.0; n];
        // Last candle: strong bullish gap
        opens[n - 1] = 86000.0;
        highs[n - 1] = 87200.0;
        lows[n - 1] = 85900.0;
        closes[n - 1] = 87100.0;

        let params = make_params(10, 50, 0.0);
        let zones = detect_gap_zones(&opens, &highs, &lows, &closes, &params);

        assert!(!zones.is_empty(), "Should detect at least one gap");
        let last = &zones[0]; // Most recent
        assert_eq!(last.age_bars, 0);
        assert!(last.bottom < last.top);
        assert_eq!(last.bottom, 86000.0);
        assert_eq!(last.top, 87100.0);
        let expected_trust = (87100.0 - 86000.0) / (87200.0 - 85900.0);
        assert!((last.trust - expected_trust).abs() < 0.01);
    }

    #[test]
    fn test_bearish_gap() {
        // Scenario 2: Bearish gap — close below lower band
        let n = 50;
        let mut opens = vec![87000.0; n];
        let mut highs = vec![87200.0; n];
        let mut lows = vec![86800.0; n];
        let mut closes = vec![87000.0; n];
        // Last candle: strong bearish gap
        opens[n - 1] = 87000.0;
        highs[n - 1] = 87100.0;
        lows[n - 1] = 85800.0;
        closes[n - 1] = 85900.0;

        let params = make_params(10, 50, 0.0);
        let zones = detect_gap_zones(&opens, &highs, &lows, &closes, &params);

        assert!(!zones.is_empty());
        let last = &zones[0];
        assert_eq!(last.bottom, 85900.0);
        assert_eq!(last.top, 87000.0);
    }

    #[test]
    fn test_normal_candle_no_gap() {
        // Scenario 3: Normal candle — no gap
        let n = 50;
        let opens = vec![87000.0; n];
        let highs = vec![87200.0; n];
        let lows = vec![86800.0; n];
        let closes = vec![87050.0; n];

        let params = make_params(10, 50, 0.0);
        let zones = detect_gap_zones(&opens, &highs, &lows, &closes, &params);
        // All normal candles (ATR ≈ 400, body = 50) — no gaps
        assert!(zones.is_empty());
    }

    #[test]
    fn test_queue_limit() {
        // Scenario 6: Queue size limit
        let n = 200;
        let opens = vec![86000.0; n];
        let mut highs = vec![86200.0; n];
        let lows = vec![85800.0; n];
        let mut closes = vec![86100.0; n];
        // Create many abnormal candles
        for i in 15..n {
            closes[i] = 88000.0; // Way above ATR band
            highs[i] = 88100.0;
        }

        let params = make_params(10, 50, 0.0);
        let zones = detect_gap_zones(&opens, &highs, &lows, &closes, &params);
        assert!(zones.len() <= 50, "Queue should be limited to 50");
    }

    #[test]
    fn test_overlap_density() {
        // Scenario 7: Overlap density
        let zones = vec![
            GapZone { bottom: 86000.0, top: 86500.0, trust: 0.9, age_bars: 10 },
            GapZone { bottom: 86200.0, top: 86800.0, trust: 0.7, age_bars: 5 },
            GapZone { bottom: 87000.0, top: 87500.0, trust: 0.8, age_bars: 2 },
        ];

        let density = overlap_density(&zones, 86300.0);
        assert_eq!(density.count, 2); // Gaps A and B
        assert!((density.weighted_trust - 1.6).abs() < 0.01);
    }

    #[test]
    fn test_no_overlap() {
        // Scenario 8: No gaps at price
        let zones = vec![
            GapZone { bottom: 86000.0, top: 86500.0, trust: 0.9, age_bars: 10 },
            GapZone { bottom: 87000.0, top: 87500.0, trust: 0.8, age_bars: 2 },
        ];

        let density = overlap_density(&zones, 86700.0);
        assert_eq!(density.count, 0);
    }

    #[test]
    fn test_empty_history() {
        // Scenario 10: Not enough bars for ATR
        let opens = vec![87000.0; 5];
        let highs = vec![87200.0; 5];
        let lows = vec![86800.0; 5];
        let closes = vec![87050.0; 5];

        let params = make_params(42, 50, 0.3);
        let zones = detect_gap_zones(&opens, &highs, &lows, &closes, &params);
        assert!(zones.is_empty());
    }

    #[test]
    fn test_min_trust_filter() {
        // Scenario 13: Gap rejected by min_trust filter
        let n = 50;
        let mut opens = vec![86500.0; n];
        let mut highs = vec![86700.0; n];
        let mut lows = vec![86300.0; n];
        let mut closes = vec![86600.0; n];
        // Doji with abnormal close but low trust
        opens[n - 1] = 86500.0;
        highs[n - 1] = 87500.0;
        lows[n - 1] = 85500.0;
        closes[n - 1] = 87200.0;
        // Trust = |87200 - 86500| / (87500 - 85500) = 700 / 2000 = 0.35

        let params = make_params(10, 50, 0.5); // min_trust = 0.5
        let zones = detect_gap_zones(&opens, &highs, &lows, &closes, &params);
        // The doji gap should be filtered out (trust 0.35 < 0.5)
        let recent = zones.iter().find(|z| z.age_bars == 0);
        assert!(recent.is_none(), "Low trust gap should be filtered");
    }

    #[test]
    fn test_gap_zone_summary() {
        // Scenario 9: Summary formatting
        let zones = vec![
            GapZone { bottom: 86200.0, top: 86500.0, trust: 0.85, age_bars: 3 },
            GapZone { bottom: 86800.0, top: 87100.0, trust: 0.9, age_bars: 5 },
            GapZone { bottom: 87200.0, top: 87600.0, trust: 0.7, age_bars: 8 },
            GapZone { bottom: 85500.0, top: 85800.0, trust: 0.8, age_bars: 12 },
        ];

        let summary = gap_zone_summary(&zones, 86600.0);
        assert_eq!(summary.zones_above, 2); // 86800-87100, 87200-87600
        assert_eq!(summary.zones_below, 2); // 86200-86500, 85500-85800
        assert!(summary.nearest_gap.is_some());
        let (b, t, _) = summary.nearest_gap.unwrap();
        // Nearest should be 86200-86500 (100 away) or 86800-87100 (200 away)
        assert_eq!(b, 86200.0);
        assert_eq!(t, 86500.0);
    }
}
