//! Pure materialization of scheduled I-Ching bars and their rendered pane values.

use super::{IchingBarTrajectory, iching_bar_trajectory};
use crate::ta::TaResult;

/// One scheduled half-open bar with its I-Ching cast trajectory.
#[derive(Debug, Clone, PartialEq)]
pub struct IchingForecastBar {
    pub bar_open_ms: i64,
    pub bar_close_ms: i64,
    pub trajectory: IchingBarTrajectory,
}

/// The five values rendered in the I-Ching pane for one bar.
#[derive(Debug, Clone, PartialEq)]
pub struct IchingRenderedPane3Values {
    pub original: f64,
    pub transformed: f64,
    pub mutual_high: f64,
    pub mutual_low: f64,
    pub mutual_mean: f64,
}

impl IchingForecastBar {
    /// Returns the five plotted I-Ching pane values derived from this bar's trajectory.
    pub fn rendered_pane3_values(&self) -> IchingRenderedPane3Values {
        let trajectory = &self.trajectory;
        IchingRenderedPane3Values {
            original: (trajectory.energy_open
                + trajectory.energy_high
                + trajectory.energy_low
                + trajectory.energy_close)
                / 4.0,
            transformed: trajectory.transformed_close,
            mutual_high: trajectory.mutual_high,
            mutual_low: trajectory.mutual_low,
            mutual_mean: trajectory.mutual_mean,
        }
    }
}

/// Computes trajectories for the supplied half-open intervals in input order.
///
/// Returns the first trajectory error unchanged if an interval is invalid.
pub fn materialize_iching_forecast(bars: &[(i64, i64)]) -> TaResult<Vec<IchingForecastBar>> {
    bars.iter()
        .map(|&(bar_open_ms, bar_close_ms)| {
            Ok(IchingForecastBar {
                bar_open_ms,
                bar_close_ms,
                trajectory: iching_bar_trajectory(bar_open_ms, bar_close_ms)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BARS: [(i64, i64); 3] = [
        (1_704_067_200_000, 1_704_070_800_000),
        (1_704_070_800_000, 1_704_078_000_000),
        (1_704_078_000_000, 1_704_092_400_000),
    ];

    #[test]
    fn materializer_preserves_order_and_half_open_bounds() {
        let forecast = materialize_iching_forecast(&BARS).expect("valid bars");
        assert_eq!(forecast.len(), BARS.len());
        for (bar, &(open, close)) in forecast.iter().zip(&BARS) {
            assert_eq!((bar.bar_open_ms, bar.bar_close_ms), (open, close));
        }
    }

    #[test]
    fn materializer_matches_direct_trajectory() {
        let forecast = materialize_iching_forecast(&BARS).expect("valid bars");
        for (bar, &(open, close)) in forecast.iter().zip(&BARS) {
            assert_eq!(
                bar.trajectory,
                iching_bar_trajectory(open, close).expect("direct trajectory")
            );
        }
    }

    #[test]
    fn materializer_propagates_invalid_interval() {
        let invalid = (BARS[1].1, BARS[1].0);
        let expected = iching_bar_trajectory(invalid.0, invalid.1).unwrap_err();
        let actual = materialize_iching_forecast(&[BARS[0], invalid]).unwrap_err();
        assert_eq!(actual.to_string(), expected.to_string());
    }

    #[test]
    fn materializer_handles_empty_input() {
        assert!(
            materialize_iching_forecast(&[])
                .expect("empty input")
                .is_empty()
        );
    }

    #[test]
    fn rendered_values_match_template_formula() {
        let bar = materialize_iching_forecast(&BARS)
            .expect("valid bars")
            .remove(2);
        let raw = &bar.trajectory;
        let rendered = bar.rendered_pane3_values();
        assert_eq!(
            rendered.original,
            (raw.energy_open + raw.energy_high + raw.energy_low + raw.energy_close) / 4.0
        );
        assert_eq!(rendered.transformed, raw.transformed_close);
        assert_eq!(rendered.mutual_high, raw.mutual_high);
        assert_eq!(rendered.mutual_low, raw.mutual_low);
        assert_eq!(rendered.mutual_mean, raw.mutual_mean);
    }

    #[test]
    fn forecast_bar_is_timeline_free() {
        let source = include_str!("forecast.rs");
        for token in [
            ["fu", "tures"].concat(),
            ["to", "kio"].concat(),
            ["Tok", "io"].concat(),
            ["as", "ync"].concat(),
            ["Utc::", "now"].concat(),
            ["SystemTime::", "now"].concat(),
        ] {
            assert!(!source.contains(&token), "unexpected dependency: {token}");
        }
    }
}
