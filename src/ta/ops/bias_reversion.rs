use crate::ta::{
    TaResult,
    kernel::{Kernel, KernelStep, PriorState},
};

use super::averages::{Rma, RmaState, Sma, SmaState, rma, sma};
use super::support::validate_finite_input;

/// Creates the complete bias-reversion recurrence.
pub fn bias_reversion(period: usize) -> BiasReversion {
    BiasReversion {
        smoother: rma(period),
        average: sma(period),
    }
}

/// Named input for bias reversion.
#[derive(Debug, Clone, Copy)]
pub struct BiasReversionInput {
    pub open: f64,
    pub bias: f64,
}

/// Carries the RMA-then-SMA pipeline for bias reversion.
#[derive(Debug, Clone)]
pub struct BiasReversion {
    smoother: Rma,
    average: Sma,
}

/// Explicit successor state for [`BiasReversion`].
#[derive(Debug, Clone)]
pub struct BiasReversionState {
    smoother: RmaState,
    average: SmaState,
}

impl Kernel for BiasReversion {
    type Input = BiasReversionInput;
    type Output = Option<f64>;
    type State = BiasReversionState;

    fn transition(
        &self,
        prior: PriorState<'_, Self::State>,
        input: &Self::Input,
    ) -> TaResult<KernelStep<Self::Output, Self::State>> {
        validate_finite_input("bias reversion open", input.open)?;
        validate_finite_input("bias reversion bias", input.bias)?;
        let smoother_prior = match prior {
            PriorState::Initial => PriorState::Initial,
            PriorState::Existing(state) => PriorState::Existing(&state.smoother),
        };
        let average_prior = match prior {
            PriorState::Initial => PriorState::Initial,
            PriorState::Existing(state) => PriorState::Existing(&state.average),
        };
        let smoother_step = self.smoother.transition(smoother_prior, &input.bias)?;
        let smoothed_bias = smoother_step.output.expect("RMA emits every point");
        let average_step = self
            .average
            .transition(average_prior, &(input.open - smoothed_bias))?;
        Ok(KernelStep {
            output: average_step.output,
            next_state: BiasReversionState {
                smoother: smoother_step.next_state,
                average: average_step.next_state,
            },
        })
    }
}

#[cfg(test)]
mod immutable_kernel_tests {
    use super::{BiasReversionInput, BiasReversionState, bias_reversion};
    use crate::ta::kernel::{Kernel, PriorState};
    use crate::ta::{TaErrorKind, processor::Processor};

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= 1e-12,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn immutable_bias_reversion_uses_named_input_and_preserves_child_transition_purity() {
        let inputs = [
            BiasReversionInput {
                open: 10.0,
                bias: 1.0,
            },
            BiasReversionInput {
                open: 12.0,
                bias: 2.0,
            },
            BiasReversionInput {
                open: 15.0,
                bias: 4.0,
            },
        ];
        let expected = [9.0, 9.833333333333334, 10.814814814814813];
        let kernel = bias_reversion(3);
        let first = kernel.transition(PriorState::Initial, &inputs[0]).unwrap();
        let repeated = kernel.transition(PriorState::Initial, &inputs[0]).unwrap();
        assert_eq!(first.output, repeated.output);
        assert_close(first.output.unwrap(), expected[0]);
        assert_close(
            kernel
                .transition(PriorState::Existing(&first.next_state), &inputs[1])
                .unwrap()
                .output
                .unwrap(),
            expected[1],
        );

        let mut processor = Processor::new(kernel);
        for (input, expected) in inputs.into_iter().zip(expected) {
            assert_close(processor.process(&input).unwrap().unwrap(), expected);
        }
    }

    #[test]
    fn immutable_bias_reversion_error_does_not_commit_rma_successor() {
        let valid = BiasReversionInput {
            open: 10.0,
            bias: 1.0,
        };
        let invalid = BiasReversionInput {
            open: f64::NAN,
            bias: 2.0,
        };
        let mut processor = Processor::new(bias_reversion(3));
        let mut control = Processor::new(bias_reversion(3));
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

        let _: Option<BiasReversionState> = None;
    }
}
