use crate::ta::{
    TaResult,
    kernel::{Kernel, KernelStep, PriorState},
};

use super::support::{validate_finite_input, validate_finite_output, validate_multiplier};

/// Input point for reversion calculations around ATR bands.
#[derive(Debug, Clone, Copy)]
pub struct BandPoint {
    pub open: f64,
    pub atr: f64,
    pub signal: f64,
}

/// Creates signed distance from an open-centered ATR band.
pub fn band_reversion(multiplier: f64) -> BandReversion {
    BandReversion { multiplier }
}

/// Immutable signed-reversion configuration.
#[derive(Debug, Clone, Copy)]
pub struct BandReversion {
    multiplier: f64,
}

/// Explicit successor state for [`BandReversion`].
#[derive(Debug, Clone, Copy)]
pub struct BandReversionState;

impl Kernel for BandReversion {
    type Input = BandPoint;
    type Output = Option<f64>;
    type State = BandReversionState;

    fn transition(
        &self,
        _prior: PriorState<'_, Self::State>,
        input: &Self::Input,
    ) -> TaResult<KernelStep<Self::Output, Self::State>> {
        validate_multiplier("band reversion multiplier", self.multiplier)?;
        validate_band_point(*input)?;
        let oscillation = input.atr * self.multiplier;
        let upper = input.open + oscillation;
        let lower = input.open - oscillation;
        let output = if lower <= input.signal && upper >= input.signal {
            0.0
        } else if input.signal - upper > 0.0 {
            input.signal - upper
        } else {
            (input.signal - lower).min(0.0)
        };
        validate_finite_output("band reversion", output)?;
        Ok(KernelStep {
            output: Some(output),
            next_state: BandReversionState,
        })
    }
}

/// Creates band reversion normalized by its ATR oscillation.
pub fn band_reversion_percent(multiplier: f64) -> BandReversionPercent {
    BandReversionPercent {
        reversion: band_reversion(multiplier),
        multiplier,
    }
}

/// Immutable normalized-reversion configuration owning its child reversion.
#[derive(Debug, Clone, Copy)]
pub struct BandReversionPercent {
    reversion: BandReversion,
    multiplier: f64,
}

/// Explicit successor state for [`BandReversionPercent`].
#[derive(Debug, Clone, Copy)]
pub struct BandReversionPercentState {
    reversion: BandReversionState,
}

impl Kernel for BandReversionPercent {
    type Input = BandPoint;
    type Output = Option<f64>;
    type State = BandReversionPercentState;

    fn transition(
        &self,
        prior: PriorState<'_, Self::State>,
        input: &Self::Input,
    ) -> TaResult<KernelStep<Self::Output, Self::State>> {
        validate_multiplier(
            "band reversion percent multiplier",
            self.multiplier,
        )?;
        let child_prior = match prior {
            PriorState::Initial => PriorState::Initial,
            PriorState::Existing(state) => PriorState::Existing(&state.reversion),
        };
        let reversion_step = self.reversion.transition(child_prior, input)?;
        let reversion = reversion_step
            .output
            .expect("band reversion emits every point");
        let oscillation = input.atr * self.multiplier;
        let output = if oscillation == 0.0 {
            0.0
        } else {
            100.0 * reversion / oscillation
        };
        validate_finite_output("band reversion percent", output)?;
        Ok(KernelStep {
            output: Some(output),
            next_state: BandReversionPercentState {
                reversion: reversion_step.next_state,
            },
        })
    }
}

/// Creates an ATR-gap flag kernel.
pub fn is_atr_gap(multiplier: f64) -> IsAtrGap {
    IsAtrGap { multiplier }
}

/// Immutable ATR-gap configuration.
#[derive(Debug, Clone, Copy)]
pub struct IsAtrGap {
    multiplier: f64,
}

/// Explicit successor state for [`IsAtrGap`].
#[derive(Debug, Clone, Copy)]
pub struct IsAtrGapState;

impl Kernel for IsAtrGap {
    type Input = BandPoint;
    type Output = Option<bool>;
    type State = IsAtrGapState;

    fn transition(
        &self,
        _prior: PriorState<'_, Self::State>,
        input: &Self::Input,
    ) -> TaResult<KernelStep<Self::Output, Self::State>> {
        validate_multiplier("ATR gap multiplier", self.multiplier)?;
        validate_band_point(*input)?;
        let oscillation = input.atr * self.multiplier;
        Ok(KernelStep {
            output: Some(
                input.signal > input.open + oscillation
                    || input.signal < input.open - oscillation,
            ),
            next_state: IsAtrGapState,
        })
    }
}

fn validate_band_point(input: BandPoint) -> TaResult<()> {
    validate_finite_input("band point open", input.open)?;
    validate_finite_input("band point ATR", input.atr)?;
    validate_finite_input("band point signal", input.signal)
}

#[cfg(test)]
mod immutable_kernel_tests {
    use super::{
        BandPoint, BandReversionPercentState, BandReversionState, IsAtrGapState, band_reversion,
        band_reversion_percent, is_atr_gap,
    };
    use crate::ta::kernel::{Kernel, PriorState};
    use crate::ta::{TaErrorKind, processor::Processor};

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= 1e-12,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn immutable_band_kernels_lock_boundary_fixture_and_direct_purity() {
        let point = BandPoint {
            open: 100.0,
            atr: 2.0,
            signal: 105.0,
        };
        let kernel = band_reversion(1.618);
        let first = kernel.transition(PriorState::Initial, &point).unwrap();
        let repeated = kernel.transition(PriorState::Initial, &point).unwrap();
        assert_close(first.output.unwrap(), 1.764);
        assert_eq!(first.output, repeated.output);
        assert_eq!(
            kernel
                .transition(PriorState::Existing(&first.next_state), &point)
                .unwrap()
                .output,
            first.output
        );

        let mut percent = Processor::new(band_reversion_percent(1.618));
        let mut gap = Processor::new(is_atr_gap(1.0));
        assert_close(
            percent
                .process(&BandPoint {
                    open: 100.0,
                    atr: 0.0,
                    signal: 100.0,
                })
                .unwrap()
                .unwrap(),
            0.0,
        );
        assert_eq!(
            gap.process(&BandPoint {
                open: 100.0,
                atr: 2.0,
                signal: 102.0
            })
            .unwrap(),
            Some(false)
        );
        assert_eq!(
            gap.process(&BandPoint {
                open: 100.0,
                atr: 2.0,
                signal: 102.1
            })
            .unwrap(),
            Some(true)
        );
    }

    #[test]
    fn immutable_band_kernels_validate_and_retry_without_state_commit() {
        let valid = BandPoint {
            open: 100.0,
            atr: 2.0,
            signal: 105.0,
        };
        let invalid = BandPoint {
            open: f64::NAN,
            atr: 2.0,
            signal: 105.0,
        };
        let mut processor = Processor::new(band_reversion_percent(1.618));
        let mut control = Processor::new(band_reversion_percent(1.618));
        processor.process(&valid).unwrap();
        control.process(&valid).unwrap();
        assert_eq!(
            processor.process(&invalid).unwrap_err().kind,
            TaErrorKind::Validation
        );
        assert_eq!(
            processor.process(&valid).unwrap(),
            control.process(&valid).unwrap()
        );

        assert_eq!(
            transition_error(band_reversion(1.0), &invalid).kind,
            TaErrorKind::Validation
        );
        let _: Option<BandReversionState> = None;
        let _: Option<BandReversionPercentState> = None;
        let _: Option<IsAtrGapState> = None;
    }

    #[test]
    fn explicit_multipliers_use_stored_value_and_reject_non_positive_non_finite() {
        // Explicit 1.0 preserves the historical hidden-1.0 gap boundary.
        let mut gap_one = Processor::new(is_atr_gap(1.0));
        assert_eq!(
            gap_one
                .process(&BandPoint {
                    open: 100.0,
                    atr: 2.0,
                    signal: 102.0
                })
                .unwrap(),
            Some(false)
        );
        assert_eq!(
            gap_one
                .process(&BandPoint {
                    open: 100.0,
                    atr: 2.0,
                    signal: 102.1
                })
                .unwrap(),
            Some(true)
        );
        // Stored multiplier widens the band: 103.0 gaps at 1.0 but not at 1.618.
        let inside_wide = BandPoint {
            open: 100.0,
            atr: 2.0,
            signal: 103.0,
        };
        assert_eq!(
            is_atr_gap(1.0)
                .transition(PriorState::Initial, &inside_wide)
                .unwrap()
                .output,
            Some(true)
        );
        assert_eq!(
            is_atr_gap(1.618)
                .transition(PriorState::Initial, &inside_wide)
                .unwrap()
                .output,
            Some(false)
        );
        // Valid existing multipliers preserve formulas.
        let point = BandPoint {
            open: 100.0,
            atr: 2.0,
            signal: 105.0,
        };
        assert_close(
            band_reversion(1.618)
                .transition(PriorState::Initial, &point)
                .unwrap()
                .output
                .unwrap(),
            1.764,
        );
        assert_close(
            band_reversion(1.0)
                .transition(PriorState::Initial, &point)
                .unwrap()
                .output
                .unwrap(),
            3.0,
        );

        for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let reversion_error = transition_error(band_reversion(invalid), &point);
            assert_eq!(reversion_error.kind, TaErrorKind::Validation);
            assert_eq!(
                reversion_error.message,
                "band reversion multiplier must be finite and strictly positive"
            );
            let percent_error = transition_error(band_reversion_percent(invalid), &point);
            assert_eq!(percent_error.kind, TaErrorKind::Validation);
            assert_eq!(
                percent_error.message,
                "band reversion percent multiplier must be finite and strictly positive"
            );
            let gap_error = transition_gap_error(is_atr_gap(invalid), &point);
            assert_eq!(gap_error.kind, TaErrorKind::Validation);
            assert_eq!(
                gap_error.message,
                "ATR gap multiplier must be finite and strictly positive"
            );

            // No state commit: retry with the same invalid kernel repeats validation.
            let mut reversion_processor = Processor::new(band_reversion(invalid));
            let first = reversion_processor.process(&point).unwrap_err();
            let retry = reversion_processor.process(&point).unwrap_err();
            assert_eq!(first.kind, TaErrorKind::Validation);
            assert_eq!(first.message, retry.message);
            assert_eq!(
                first.message,
                "band reversion multiplier must be finite and strictly positive"
            );

            let mut percent_processor = Processor::new(band_reversion_percent(invalid));
            let first = percent_processor.process(&point).unwrap_err();
            let retry = percent_processor.process(&point).unwrap_err();
            assert_eq!(first.kind, TaErrorKind::Validation);
            assert_eq!(first.message, retry.message);
            assert_eq!(
                first.message,
                "band reversion percent multiplier must be finite and strictly positive"
            );

            let mut gap_processor = Processor::new(is_atr_gap(invalid));
            let first = gap_processor.process(&point).unwrap_err();
            let retry = gap_processor.process(&point).unwrap_err();
            assert_eq!(first.kind, TaErrorKind::Validation);
            assert_eq!(first.message, retry.message);
            assert_eq!(
                first.message,
                "ATR gap multiplier must be finite and strictly positive"
            );
        }
    }

    fn transition_error<K>(kernel: K, input: &BandPoint) -> crate::ta::TaError
    where
        K: Kernel<Input = BandPoint, Output = Option<f64>>,
    {
        match kernel.transition(PriorState::Initial, input) {
            Ok(_) => panic!("transition must fail"),
            Err(error) => error,
        }
    }

    fn transition_gap_error(
        kernel: super::IsAtrGap,
        input: &BandPoint,
    ) -> crate::ta::TaError {
        match kernel.transition(PriorState::Initial, input) {
            Ok(_) => panic!("transition must fail"),
            Err(error) => error,
        }
    }
}
