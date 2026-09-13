use crate::ta::{
    TaError, TaResult,
    kernel::{Kernel, KernelStep, PriorState},
};

use super::averages::{Rma, RmaState, rma};
use super::support::{validate_finite_input, validate_finite_output, validate_period};

const RSI_NEUTRAL: f64 = 50.0;
const RSI_MAXIMUM: f64 = 100.0;

/// Creates an RSI kernel with Wilder gain/loss smoothing for the supplied period.
pub fn rsi(period: usize) -> Rsi {
    Rsi {
        period,
        gains: rma(period),
        losses: rma(period),
    }
}

/// Immutable RSI configuration owning the period and Wilder gain/loss children.
#[derive(Debug, Clone)]
pub struct Rsi {
    period: usize,
    gains: Rma,
    losses: Rma,
}

/// Explicit successor state for [`Rsi`].
#[derive(Debug, Clone)]
pub struct RsiState {
    previous: Option<f64>,
    gains: RmaState,
    losses: RmaState,
}

impl Kernel for Rsi {
    type Input = f64;
    type Output = Option<f64>;
    type State = RsiState;

    fn transition(
        &self,
        prior: PriorState<'_, Self::State>,
        input: &Self::Input,
    ) -> TaResult<KernelStep<Self::Output, Self::State>> {
        validate_period(self.period)?;
        validate_finite_input("RSI", *input)?;
        let previous = match prior {
            PriorState::Initial => None,
            PriorState::Existing(state) => state.previous,
        };
        let difference = previous.map_or(0.0, |previous| *input - previous);
        let gains_prior = match prior {
            PriorState::Initial => PriorState::Initial,
            PriorState::Existing(state) => PriorState::Existing(&state.gains),
        };
        let losses_prior = match prior {
            PriorState::Initial => PriorState::Initial,
            PriorState::Existing(state) => PriorState::Existing(&state.losses),
        };
        let gains_step = self.gains.transition(gains_prior, &difference.max(0.0))?;
        let losses_step = self
            .losses
            .transition(losses_prior, &(-difference).max(0.0))?;
        let gain = gains_step.output.expect("RMA emits every point");
        let loss = losses_step.output.expect("RMA emits every point");
        let output = if loss == 0.0 && gain == 0.0 {
            RSI_NEUTRAL
        } else if loss == 0.0 {
            RSI_MAXIMUM
        } else {
            RSI_MAXIMUM - RSI_MAXIMUM / (1.0 + gain / loss)
        };
        validate_finite_output("RSI", output)?;
        Ok(KernelStep {
            output: Some(output),
            next_state: RsiState {
                previous: Some(*input),
                gains: gains_step.next_state,
                losses: losses_step.next_state,
            },
        })
    }
}

/// Creates a reverse-RSI kernel for a target strictly inside 0..100.
pub fn reverse_rsi(period: usize, target: f64) -> ReverseRsi {
    ReverseRsi {
        period,
        target,
        gains: rma(period),
        losses: rma(period),
    }
}

/// Immutable reverse-RSI configuration owning period, target, and Wilder children.
#[derive(Debug, Clone)]
pub struct ReverseRsi {
    period: usize,
    target: f64,
    gains: Rma,
    losses: Rma,
}

/// Explicit successor state for [`ReverseRsi`].
#[derive(Debug, Clone)]
pub struct ReverseRsiState {
    previous: Option<f64>,
    gains: RmaState,
    losses: RmaState,
}

impl Kernel for ReverseRsi {
    type Input = f64;
    type Output = Option<f64>;
    type State = ReverseRsiState;

    fn transition(
        &self,
        prior: PriorState<'_, Self::State>,
        input: &Self::Input,
    ) -> TaResult<KernelStep<Self::Output, Self::State>> {
        validate_period(self.period)?;
        validate_finite_input("reverse RSI", *input)?;
        validate_finite_input("reverse RSI target", self.target)?;
        if !(0.0 < self.target && self.target < RSI_MAXIMUM) {
            return Err(TaError::validation(
                "reverse RSI target must be strictly between 0 and 100",
            ));
        }
        let previous = match prior {
            PriorState::Initial => None,
            PriorState::Existing(state) => state.previous,
        };
        let difference = previous.map_or(0.0, |previous| *input - previous);
        let gains_prior = match prior {
            PriorState::Initial => PriorState::Initial,
            PriorState::Existing(state) => PriorState::Existing(&state.gains),
        };
        let losses_prior = match prior {
            PriorState::Initial => PriorState::Initial,
            PriorState::Existing(state) => PriorState::Existing(&state.losses),
        };
        let gains_step = self.gains.transition(gains_prior, &difference.max(0.0))?;
        let losses_step = self
            .losses
            .transition(losses_prior, &(-difference).max(0.0))?;
        let gain = gains_step.output.expect("RMA emits every point");
        let loss = losses_step.output.expect("RMA emits every point");
        let target_ratio = self.target / (RSI_MAXIMUM - self.target);
        let reverse_ratio = (RSI_MAXIMUM - self.target) / self.target;
        let change = (self.period - 1) as f64 * (loss * target_ratio - gain);
        let output = if change >= 0.0 {
            *input + change
        } else {
            *input + change * reverse_ratio
        };
        validate_finite_output("reverse RSI", output)?;
        Ok(KernelStep {
            output: Some(output),
            next_state: ReverseRsiState {
                previous: Some(*input),
                gains: gains_step.next_state,
                losses: losses_step.next_state,
            },
        })
    }
}

#[cfg(test)]
mod immutable_kernel_tests {
    use super::{ReverseRsiState, RsiState, reverse_rsi, rsi};
    use crate::ta::kernel::{Kernel, PriorState};
    use crate::ta::{TaErrorKind, processor::Processor};

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= 1e-12,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn immutable_rsi_kernels_lock_fixtures_and_direct_transition_purity() {
        let inputs = [10.0, 10.0, 12.0, 9.0, 9.0, 12.0];
        let rsi_expected = [
            50.0,
            50.0,
            100.0,
            30.769230769230774,
            30.769230769230774,
            72.93233082706766,
        ];
        let reverse_expected = [
            10.0,
            10.0,
            10.666666666666666,
            10.11111111111111,
            9.74074074074074,
            10.493827160493828,
        ];

        let rsi_kernel = rsi(3);
        let first = rsi_kernel
            .transition(PriorState::Initial, &inputs[0])
            .unwrap();
        let repeated = rsi_kernel
            .transition(PriorState::Initial, &inputs[0])
            .unwrap();
        assert_eq!(first.output, repeated.output);
        assert_close(first.output.unwrap(), rsi_expected[0]);
        assert_close(
            rsi_kernel
                .transition(PriorState::Existing(&first.next_state), &inputs[1])
                .unwrap()
                .output
                .unwrap(),
            rsi_expected[1],
        );

        let mut rsi_processor = Processor::new(rsi_kernel);
        let mut reverse_processor = Processor::new(reverse_rsi(3, 50.0));
        for ((input, expected_rsi), expected_reverse) in
            inputs.into_iter().zip(rsi_expected).zip(reverse_expected)
        {
            assert_close(
                rsi_processor.process(&input).unwrap().unwrap(),
                expected_rsi,
            );
            assert_close(
                reverse_processor.process(&input).unwrap().unwrap(),
                expected_reverse,
            );
        }
    }

    #[test]
    fn immutable_rsi_kernels_validate_and_retry_without_state_commit() {
        assert_eq!(
            transition_error(rsi(0), &10.0).kind,
            TaErrorKind::InvalidPeriod
        );
        for target in [0.0, 100.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                transition_error(reverse_rsi(3, target), &10.0).kind,
                TaErrorKind::Validation
            );
        }

        let mut processor = Processor::new(rsi(3));
        let mut control = Processor::new(rsi(3));
        processor.process(&10.0).unwrap();
        control.process(&10.0).unwrap();
        assert_eq!(
            processor.process(&f64::NAN).unwrap_err().kind,
            TaErrorKind::Validation
        );
        assert_eq!(
            processor.process(&12.0).unwrap(),
            control.process(&12.0).unwrap()
        );

        let _: Option<RsiState> = None;
        let _: Option<ReverseRsiState> = None;
    }

    fn transition_error<K>(kernel: K, input: &f64) -> crate::ta::TaError
    where
        K: Kernel<Input = f64, Output = Option<f64>>,
    {
        match kernel.transition(PriorState::Initial, input) {
            Ok(_) => panic!("transition must fail"),
            Err(error) => error,
        }
    }
}
