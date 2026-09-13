use crate::{
    model::kline::Kline,
    ta::{
        TaResult,
        kernel::{Kernel, KernelStep, PriorState},
    },
};

use super::averages::{Rma, RmaState, rma};
use super::support::{validate_finite_input, validate_finite_output, validate_period};

/// Creates an ATR kernel using the canonical true-range and Wilder smoothing formulas.
pub fn atr(period: usize) -> Atr {
    Atr {
        period,
        smoother: rma(period),
    }
}

/// Immutable ATR configuration owning the period and Wilder smoothing child.
#[derive(Debug, Clone)]
pub struct Atr {
    period: usize,
    smoother: Rma,
}

/// Explicit successor state for [`Atr`].
#[derive(Debug, Clone)]
pub struct AtrState {
    previous_close: Option<f64>,
    smoother: RmaState,
}

impl Kernel for Atr {
    type Input = Kline;
    type Output = Option<f64>;
    type State = AtrState;

    fn transition(
        &self,
        prior: PriorState<'_, Self::State>,
        input: &Self::Input,
    ) -> TaResult<KernelStep<Self::Output, Self::State>> {
        validate_period(self.period)?;
        validate_kline(input)?;
        let previous_close = match prior {
            PriorState::Initial => input.close,
            PriorState::Existing(state) => state.previous_close.unwrap_or(input.close),
        };
        let true_range = (input.high - input.low)
            .max((input.high - previous_close).abs())
            .max((input.low - previous_close).abs());
        validate_finite_output("OHLC true range", true_range)?;
        let smoother_prior = match prior {
            PriorState::Initial => PriorState::Initial,
            PriorState::Existing(state) => PriorState::Existing(&state.smoother),
        };
        let smoother_step = self.smoother.transition(smoother_prior, &true_range)?;
        Ok(KernelStep {
            output: smoother_step.output,
            next_state: AtrState {
                previous_close: Some(input.close),
                smoother: smoother_step.next_state,
            },
        })
    }
}

/// Creates a bar-bias kernel from one OHLC candle.
pub fn bar_bias() -> BarBias {
    BarBias
}

/// Immutable bar-bias configuration.
#[derive(Debug, Clone, Copy)]
pub struct BarBias;

/// Explicit successor state for [`BarBias`].
#[derive(Debug, Clone, Copy)]
pub struct BarBiasState;

impl Kernel for BarBias {
    type Input = Kline;
    type Output = Option<f64>;
    type State = BarBiasState;

    fn transition(
        &self,
        _prior: PriorState<'_, Self::State>,
        input: &Self::Input,
    ) -> TaResult<KernelStep<Self::Output, Self::State>> {
        validate_kline(input)?;
        let output =
            (input.close - input.open) + (input.high - input.open) - (input.open - input.low);
        validate_finite_output("OHLC bar bias", output)?;
        Ok(KernelStep {
            output: Some(output),
            next_state: BarBiasState,
        })
    }
}

/// Creates a body-ratio kernel from one OHLC candle.
pub fn body_ratio() -> BodyRatio {
    BodyRatio
}

/// Immutable body-ratio configuration.
#[derive(Debug, Clone, Copy)]
pub struct BodyRatio;

/// Explicit successor state for [`BodyRatio`].
#[derive(Debug, Clone, Copy)]
pub struct BodyRatioState;

impl Kernel for BodyRatio {
    type Input = Kline;
    type Output = Option<f64>;
    type State = BodyRatioState;

    fn transition(
        &self,
        _prior: PriorState<'_, Self::State>,
        input: &Self::Input,
    ) -> TaResult<KernelStep<Self::Output, Self::State>> {
        validate_kline(input)?;
        let range = input.high - input.low;
        let output = if range == 0.0 {
            0.0
        } else {
            (input.close - input.open).abs() / range
        };
        validate_finite_output("OHLC body ratio", output)?;
        Ok(KernelStep {
            output: Some(output),
            next_state: BodyRatioState,
        })
    }
}

fn validate_kline(input: &Kline) -> TaResult<()> {
    for (name, value) in [
        ("open", input.open),
        ("high", input.high),
        ("low", input.low),
        ("close", input.close),
        ("volume", input.volume),
    ] {
        validate_finite_input(name, value)?;
    }
    Ok(())
}

#[cfg(test)]
mod immutable_kernel_tests {
    use super::{AtrState, BarBiasState, BodyRatioState, atr, bar_bias, body_ratio};
    use crate::{
        model::kline::Kline,
        ta::{
            TaErrorKind,
            kernel::{Kernel, PriorState},
            processor::Processor,
        },
    };

    fn kline(open: f64, high: f64, low: f64, close: f64) -> Kline {
        Kline {
            open,
            high,
            low,
            close,
            volume: 1.0,
            time: 1,
            adjclose: None,
        }
    }

    #[test]
    fn immutable_candle_kernels_emit_first_values_and_are_pure_until_adopted() {
        let candle = kline(10.0, 14.0, 9.0, 13.0);

        let atr = atr(3);
        let first = atr.transition(PriorState::Initial, &candle).unwrap();
        let repeated = atr.transition(PriorState::Initial, &candle).unwrap();
        assert_eq!(first.output, Some(5.0));
        assert_eq!(first.output, repeated.output);
        let second = atr
            .transition(PriorState::Existing(&first.next_state), &candle)
            .unwrap();
        assert_eq!(second.output, Some(5.0));

        let mut bar_bias_processor = Processor::new(bar_bias());
        let mut body_ratio_processor = Processor::new(body_ratio());
        assert_eq!(bar_bias_processor.process(&candle).unwrap(), Some(6.0));
        assert_eq!(body_ratio_processor.process(&candle).unwrap(), Some(0.6));
    }

    #[test]
    fn immutable_candle_kernels_reject_invalid_input_without_commit() {
        let valid = kline(10.0, 14.0, 9.0, 13.0);
        let invalid = kline(f64::NAN, 14.0, 9.0, 13.0);
        let mut processor = Processor::new(atr(3));
        let mut control = Processor::new(atr(3));
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

        let overflow = kline(f64::MAX, f64::MAX, -f64::MAX, f64::MAX);
        assert_eq!(
            transition_error(bar_bias(), &overflow).kind,
            TaErrorKind::Computation
        );
        assert_eq!(
            transition_error(body_ratio(), &invalid).kind,
            TaErrorKind::Validation
        );

        let _: Option<AtrState> = None;
        let _: Option<BarBiasState> = None;
        let _: Option<BodyRatioState> = None;
    }

    fn transition_error<K>(kernel: K, input: &Kline) -> crate::ta::TaError
    where
        K: Kernel<Input = Kline, Output = Option<f64>>,
    {
        match kernel.transition(PriorState::Initial, input) {
            Ok(_) => panic!("transition must fail"),
            Err(error) => error,
        }
    }
}
