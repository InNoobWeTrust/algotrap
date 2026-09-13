use std::collections::VecDeque;

use crate::ta::{
    TaResult,
    kernel::{Kernel, KernelStep, PriorState},
};

use super::support::{validate_finite_input, validate_finite_output, validate_period};

/// Creates a simple moving-average kernel for the supplied period.
pub fn sma(period: usize) -> Sma {
    Sma { period }
}

/// Immutable simple moving-average configuration.
#[derive(Debug, Clone)]
pub struct Sma {
    period: usize,
}

/// Explicit successor state for [`Sma`].
#[derive(Debug, Clone)]
pub struct SmaState {
    values: VecDeque<f64>,
}

impl Kernel for Sma {
    type Input = f64;
    type Output = Option<f64>;
    type State = SmaState;

    fn transition(
        &self,
        prior: PriorState<'_, Self::State>,
        input: &Self::Input,
    ) -> TaResult<KernelStep<Self::Output, Self::State>> {
        validate_period(self.period)?;
        validate_finite_input("SMA", *input)?;

        let values = match prior {
            PriorState::Initial => None,
            PriorState::Existing(state) => Some(&state.values),
        };
        let len = values.map_or(0, VecDeque::len);
        let at_capacity = len == self.period;
        let retained_sum = match values {
            None => 0.0,
            Some(values) if at_capacity => values.iter().skip(1).sum::<f64>(),
            Some(values) => values.iter().sum::<f64>(),
        };
        let count = if at_capacity { self.period } else { len + 1 };
        let output = (retained_sum + *input) / count as f64;
        validate_finite_output("SMA", output)?;

        let mut next_values = values.map_or_else(VecDeque::new, Clone::clone);
        if at_capacity {
            next_values.pop_front();
        }
        next_values.push_back(*input);
        Ok(KernelStep {
            output: Some(output),
            next_state: SmaState {
                values: next_values,
            },
        })
    }
}

/// Creates an exponentially weighted moving-average kernel for the supplied period.
pub fn ema(period: usize) -> Ema {
    Ema {
        period,
        alpha: 2.0 / (period as f64 + 1.0),
    }
}

/// Immutable exponentially weighted moving-average configuration.
#[derive(Debug, Clone)]
pub struct Ema {
    period: usize,
    alpha: f64,
}

/// Explicit successor state for [`Ema`].
#[derive(Debug, Clone)]
pub struct EmaState {
    previous: Option<f64>,
}

impl Kernel for Ema {
    type Input = f64;
    type Output = Option<f64>;
    type State = EmaState;

    fn transition(
        &self,
        prior: PriorState<'_, Self::State>,
        input: &Self::Input,
    ) -> TaResult<KernelStep<Self::Output, Self::State>> {
        validate_period(self.period)?;
        validate_finite_input("EMA", *input)?;
        let previous = match prior {
            PriorState::Initial => None,
            PriorState::Existing(state) => state.previous,
        };
        let output = smoothed_output(self.alpha, previous, *input);
        validate_finite_output("EMA", output)?;
        Ok(KernelStep {
            output: Some(output),
            next_state: EmaState {
                previous: Some(output),
            },
        })
    }
}

/// Creates a Wilder moving-average kernel for the supplied period.
pub fn rma(period: usize) -> Rma {
    Rma {
        period,
        alpha: 1.0 / period as f64,
    }
}

/// Immutable Wilder moving-average configuration.
#[derive(Debug, Clone)]
pub struct Rma {
    period: usize,
    alpha: f64,
}

/// Explicit successor state for [`Rma`].
#[derive(Debug, Clone)]
pub struct RmaState {
    previous: Option<f64>,
}

impl Kernel for Rma {
    type Input = f64;
    type Output = Option<f64>;
    type State = RmaState;

    fn transition(
        &self,
        prior: PriorState<'_, Self::State>,
        input: &Self::Input,
    ) -> TaResult<KernelStep<Self::Output, Self::State>> {
        validate_period(self.period)?;
        validate_finite_input("RMA", *input)?;
        let previous = match prior {
            PriorState::Initial => None,
            PriorState::Existing(state) => state.previous,
        };
        let output = smoothed_output(self.alpha, previous, *input);
        validate_finite_output("RMA", output)?;
        Ok(KernelStep {
            output: Some(output),
            next_state: RmaState {
                previous: Some(output),
            },
        })
    }
}

fn smoothed_output(alpha: f64, previous: Option<f64>, input: f64) -> f64 {
    previous.map_or(input, |previous| (1.0 - alpha) * previous + alpha * input)
}

#[cfg(test)]
mod immutable_kernel_tests {
    use super::{ema, rma, sma};
    use crate::ta::kernel::{Kernel, PriorState};
    use crate::ta::{TaErrorKind, processor::Processor};

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= 1e-12,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn immutable_moving_average_kernels_lock_fixtures_and_state_adoption() {
        let inputs = [1.0, 2.0, 4.0, 8.0];
        let expected = [
            [1.0, 1.5, 2.3333333333333335, 4.666666666666667],
            [1.0, 1.5, 2.75, 5.375],
            [
                1.0,
                1.3333333333333333,
                2.2222222222222223,
                4.148148148148148,
            ],
        ];

        assert_moving_average(sma(3), inputs, expected[0]);
        assert_moving_average(ema(3), inputs, expected[1]);
        assert_moving_average(rma(3), inputs, expected[2]);
    }

    #[test]
    fn immutable_moving_average_errors_preserve_state_and_categories() {
        assert_eq!(
            transition_error(sma(0), &1.0).kind,
            TaErrorKind::InvalidPeriod
        );
        assert_eq!(
            transition_error(ema(0), &1.0).kind,
            TaErrorKind::InvalidPeriod
        );
        assert_eq!(
            transition_error(rma(0), &1.0).kind,
            TaErrorKind::InvalidPeriod
        );

        let mut sma_processor = Processor::new(sma(2));
        let mut control = Processor::new(sma(2));
        sma_processor.process(&1.0).unwrap();
        control.process(&1.0).unwrap();
        assert_eq!(
            sma_processor.process(&f64::NAN).unwrap_err().kind,
            TaErrorKind::Validation
        );
        assert_eq!(
            sma_processor.process(&2.0).unwrap(),
            control.process(&2.0).unwrap()
        );

        let kernel = sma(2);
        let step = kernel.transition(PriorState::Initial, &f64::MAX).unwrap();
        let error = match kernel.transition(PriorState::Existing(&step.next_state), &f64::MAX) {
            Ok(_) => panic!("overflow must fail"),
            Err(error) => error,
        };
        assert_eq!(error.kind, TaErrorKind::Computation);
    }

    fn assert_moving_average<K>(kernel: K, inputs: [f64; 4], expected: [f64; 4])
    where
        K: Kernel<Input = f64, Output = Option<f64>>,
    {
        let first = kernel.transition(PriorState::Initial, &inputs[0]).unwrap();
        let repeated = kernel.transition(PriorState::Initial, &inputs[0]).unwrap();
        assert_close(first.output.unwrap(), expected[0]);
        assert_eq!(first.output, repeated.output);

        let next_from_first = kernel
            .transition(PriorState::Existing(&first.next_state), &inputs[1])
            .unwrap();
        let next_from_repeated = kernel
            .transition(PriorState::Existing(&repeated.next_state), &inputs[1])
            .unwrap();
        assert_close(next_from_first.output.unwrap(), expected[1]);
        assert_eq!(next_from_first.output, next_from_repeated.output);

        let mut processor = Processor::new(kernel);
        for (input, expected) in inputs.into_iter().zip(expected) {
            assert_close(processor.process(&input).unwrap().unwrap(), expected);
        }
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
