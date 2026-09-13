use std::collections::VecDeque;

use crate::ta::{
    TaResult,
    kernel::{Kernel, KernelStep, PriorState},
};

use super::averages::{Sma, SmaState, sma};
use super::support::{validate_finite_input, validate_finite_output, validate_period};

/// Creates a canonical rolling Sharpe transform.
pub fn sharpe(period: usize) -> Sharpe {
    Sharpe {
        period,
        close_sma: sma(period),
    }
}

/// Carries the rolling windows used by the canonical Sharpe calculation.
#[derive(Debug, Clone)]
pub struct Sharpe {
    period: usize,
    close_sma: Sma,
}

/// Explicit successor state for [`Sharpe`].
#[derive(Debug, Clone)]
pub struct SharpeState {
    close_sma: SmaState,
    closes: VecDeque<f64>,
    deviations: VecDeque<f64>,
}

impl Kernel for Sharpe {
    type Input = f64;
    type Output = Option<f64>;
    type State = SharpeState;

    fn transition(
        &self,
        prior: PriorState<'_, Self::State>,
        input: &Self::Input,
    ) -> TaResult<KernelStep<Self::Output, Self::State>> {
        validate_period(self.period)?;
        validate_finite_input("Sharpe close", *input)?;
        let sma_prior = match prior {
            PriorState::Initial => PriorState::Initial,
            PriorState::Existing(state) => PriorState::Existing(&state.close_sma),
        };
        let sma_step = self.close_sma.transition(sma_prior, input)?;
        let close_sma = sma_step.output.expect("SMA emits every point");
        let mut next_closes = match prior {
            PriorState::Initial => VecDeque::new(),
            PriorState::Existing(state) => state.closes.clone(),
        };
        let mut next_deviations = match prior {
            PriorState::Initial => VecDeque::new(),
            PriorState::Existing(state) => state.deviations.clone(),
        };
        push_window(&mut next_closes, *input, self.period);
        push_window(&mut next_deviations, *input - close_sma, self.period);
        let output = if next_closes.len() < 2 {
            0.0
        } else {
            let mean = next_closes.iter().sum::<f64>() / next_closes.len() as f64;
            let stdev = (next_closes
                .iter()
                .map(|value| (value - mean).powi(2))
                .sum::<f64>()
                / (next_closes.len() - 1) as f64)
                .sqrt();
            if stdev == 0.0 {
                0.0
            } else {
                next_deviations.iter().sum::<f64>() / self.period as f64 / stdev
            }
        };
        validate_finite_output("Sharpe", output)?;
        Ok(KernelStep {
            output: Some(output),
            next_state: SharpeState {
                close_sma: sma_step.next_state,
                closes: next_closes,
                deviations: next_deviations,
            },
        })
    }
}

fn push_window(values: &mut VecDeque<f64>, input: f64, period: usize) {
    if values.len() == period {
        values.pop_front();
    }
    values.push_back(input);
}

#[cfg(test)]
mod immutable_kernel_tests {
    use super::{SharpeState, sharpe};
    use crate::ta::kernel::{Kernel, PriorState};
    use crate::ta::{TaErrorKind, processor::Processor};

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= 1e-12,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn immutable_sharpe_locks_rolling_fixture_and_state_adoption() {
        let inputs = [1.0, 2.0, 4.0, 8.0];
        let expected = [
            0.0,
            0.2357022603955158,
            0.4728054288446501,
            0.6000991981489789,
        ];
        let kernel = sharpe(3);
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
    fn immutable_sharpe_rejects_invalid_input_without_window_commit() {
        assert_eq!(
            transition_error(sharpe(0), &1.0).kind,
            TaErrorKind::InvalidPeriod
        );
        let mut processor = Processor::new(sharpe(3));
        let mut control = Processor::new(sharpe(3));
        processor.process(&1.0).unwrap();
        control.process(&1.0).unwrap();
        assert_eq!(
            processor.process(&f64::NAN).unwrap_err().kind,
            TaErrorKind::Validation
        );
        assert_eq!(
            processor.process(&2.0).unwrap(),
            control.process(&2.0).unwrap()
        );
        let _: Option<SharpeState> = None;
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
