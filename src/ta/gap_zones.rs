//! Pure typed ATR gap-zone domain kernels.

use super::{TaError, TaResult};
use super::{ohlc::Ohlc, validate_finite_output, validate_finite_value};

/// Returns ATR-gap flags where close lies strictly beyond its open-centered ATR bands.
pub fn is_atr_gap(ohlc: Ohlc<'_>, atr_period: usize) -> Result<Vec<bool>, TaError> {
    let result = ohlc
        .close()
        .iter()
        .zip(ohlc.open())
        .zip(ohlc.atr(atr_period)?)
        .map(|((&close, &open), atr)| close > open + atr || close < open - atr)
        .collect();
    Ok(result)
}

/// Returns ATR-gap flags from an already computed ATR vector, avoiding a second
/// ATR pass. Formula is identical to [`is_atr_gap`].
pub(crate) fn is_atr_gap_with_atr(ohlc: Ohlc<'_>, atr: &[f64]) -> Result<Vec<bool>, TaError> {
    if atr.len() != ohlc.open().len() {
        return Err(TaError::alignment(
            "ATR-gap ATR vector must match OHLC length",
        ));
    }
    let result = ohlc
        .close()
        .iter()
        .zip(ohlc.open())
        .zip(atr)
        .map(|((&close, &open), &atr)| close > open + atr || close < open - atr)
        .collect();
    Ok(result)
}

/// Returns the shared zero-range-safe body ratio for every bar.
pub fn body_ratio(ohlc: Ohlc<'_>) -> TaResult<Vec<f64>> {
    ohlc.body_ratio()
}

/// A typed upstream indicator row used to extract a gap zone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GapCandle {
    pub open: f64,
    pub close: f64,
    pub is_atr_gap: bool,
    pub body_ratio: f64,
    pub rssi: Option<f64>,
    /// Number of chronological source bars since this candle, supplied upstream.
    pub age_bars: usize,
}

impl GapCandle {
    /// Validates the finite upstream values supplied by the engine adapter.
    pub fn validate(self) -> TaResult<Self> {
        validate_finite_value("gap candle open", self.open)?;
        validate_finite_value("gap candle close", self.close)?;
        validate_finite_value("gap candle body_ratio", self.body_ratio)?;
        if let Some(rssi) = self.rssi {
            validate_finite_value("gap candle rssi", rssi)?;
        }
        Ok(self)
    }
}

/// A price range created by an abnormal candle.
#[derive(Debug, Clone, PartialEq)]
pub struct GapZone {
    pub bottom: f64,
    pub top: f64,
    pub trust: f64,
    pub age_bars: usize,
    pub bullish: bool,
}

/// Aggregate zone coverage at one price.
#[derive(Debug, Clone, PartialEq)]
pub struct OverlapDensity {
    pub count: usize,
    pub weighted_trust: f64,
}

/// Gap-zone extraction parameters.
///
/// `atr_period` is required metadata for the upstream `is_atr_gap` column;
/// ATR itself is calculated before this pure extraction boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct GapZoneParams {
    pub atr_period: usize,
    pub max_zones: usize,
    pub min_trust: f64,
}

impl GapZoneParams {
    /// Validates extraction parameters and the upstream ATR metadata contract.
    pub fn validate(&self) -> TaResult<()> {
        if self.atr_period == 0 {
            return Err(TaError::validation(
                "gap-zone ATR period must be greater than zero",
            ));
        }
        validate_finite_value("gap-zone minimum trust", self.min_trust)
    }
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

/// Extracts recent qualifying zones from typed, chronological upstream indicator rows.
pub fn extract_gap_zones(
    candles: &[GapCandle],
    params: &GapZoneParams,
) -> Result<Vec<GapZone>, TaError> {
    params.validate()?;
    for candle in candles {
        candle.validate()?;
    }
    if params.max_zones == 0 {
        return Ok(Vec::new());
    }

    let mut zones = candles
        .iter()
        .copied()
        .filter_map(|candle| {
            if !candle.is_atr_gap || candle.body_ratio < params.min_trust {
                return None;
            }
            let trust = candle.rssi.map_or(candle.body_ratio, |rssi| {
                candle.body_ratio * (0.5 + 0.5 * ((rssi - 50.0).abs() / 50.0))
            });
            Some(GapZone {
                bottom: candle.open.min(candle.close),
                top: candle.open.max(candle.close),
                trust,
                age_bars: candle.age_bars,
                bullish: candle.close > candle.open,
            })
        })
        .collect::<Vec<_>>();
    validate_finite_output(
        "gap-zone trusts",
        &zones.iter().map(|zone| zone.trust).collect::<Vec<_>>(),
    )?;

    let retained_from = zones.len().saturating_sub(params.max_zones);
    zones.drain(..retained_from);
    zones.reverse();
    Ok(zones)
}

fn validate_zones(zones: &[GapZone]) -> TaResult<()> {
    for (index, zone) in zones.iter().enumerate() {
        for (name, value) in [
            ("bottom", zone.bottom),
            ("top", zone.top),
            ("trust", zone.trust),
        ] {
            validate_finite_value(&format!("gap zone {index} {name}"), value)?;
        }
        if zone.bottom > zone.top {
            return Err(TaError::validation(format!(
                "gap zone {index} bottom must not exceed top"
            )));
        }
    }
    Ok(())
}

/// Computes zone overlap at a finite price.
pub fn overlap_density(zones: &[GapZone], price: f64) -> TaResult<OverlapDensity> {
    validate_finite_value("gap-zone overlap price", price)?;
    validate_zones(zones)?;
    let (count, weighted_trust) = zones
        .iter()
        .filter(|zone| zone.bottom <= price && price <= zone.top)
        .fold((0, 0.0), |(count, trust), zone| {
            (count + 1, trust + zone.trust)
        });
    validate_finite_output("gap-zone overlap density", &[weighted_trust])?;
    Ok(OverlapDensity {
        count,
        weighted_trust,
    })
}

/// Gap-zone context for LLM consumers.
#[derive(Debug, Clone, PartialEq)]
pub struct GapZoneSummary {
    pub zones_above: usize,
    pub zones_below: usize,
    pub overlap_at_price: OverlapDensity,
    pub nearest_gap: Option<(f64, f64, f64)>,
}

/// Summarizes valid zones relative to a finite current price.
pub fn gap_zone_summary(zones: &[GapZone], current_price: f64) -> TaResult<GapZoneSummary> {
    validate_finite_value("gap-zone current price", current_price)?;
    validate_zones(zones)?;
    let (zones_above, zones_below) = zones.iter().fold((0, 0), |(above, below), zone| {
        if (zone.bottom + zone.top) / 2.0 > current_price {
            (above + 1, below)
        } else {
            (above, below + 1)
        }
    });
    let nearest_gap = zones
        .iter()
        .min_by(|left, right| {
            let distance = |zone: &GapZone| {
                if current_price < zone.bottom {
                    zone.bottom - current_price
                } else if current_price > zone.top {
                    current_price - zone.top
                } else {
                    0.0
                }
            };
            distance(left).total_cmp(&distance(right))
        })
        .map(|zone| (zone.bottom, zone.top, zone.trust));
    Ok(GapZoneSummary {
        zones_above,
        zones_below,
        overlap_at_price: overlap_density(zones, current_price)?,
        nearest_gap,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_gap_kernels_preserve_zero_range_and_first_row_behavior() {
        let ohlc = Ohlc::new(&[10.0, 10.0], &[10.0, 15.0], &[10.0, 9.0], &[10.0, 14.0]).unwrap();
        assert_eq!(body_ratio(ohlc).unwrap(), vec![0.0, 2.0 / 3.0]);
        assert_eq!(is_atr_gap(ohlc, 2).unwrap(), vec![false, true]);
    }

    #[test]
    fn typed_candles_preserve_gap_zone_semantics() {
        let zones = extract_gap_zones(
            &[GapCandle {
                open: 100.0,
                close: 105.0,
                is_atr_gap: true,
                body_ratio: 0.5,
                rssi: Some(70.0),
                age_bars: 0,
            }],
            &GapZoneParams::default(),
        )
        .unwrap();
        assert_eq!(zones[0].bottom, 100.0);
        assert_eq!(zones[0].top, 105.0);
        assert_eq!(zones[0].trust, 0.35);
    }

    #[test]
    fn typed_gap_inputs_and_parameters_reject_invalid_values() {
        assert!(
            GapCandle {
                open: f64::NAN,
                close: 1.0,
                is_atr_gap: true,
                body_ratio: 1.0,
                rssi: None,
                age_bars: 0,
            }
            .validate()
            .is_err()
        );
        assert!(
            extract_gap_zones(
                &[],
                &GapZoneParams {
                    atr_period: 0,
                    ..GapZoneParams::default()
                }
            )
            .is_err()
        );
        assert!(overlap_density(&[], f64::INFINITY).is_err());
    }
}
