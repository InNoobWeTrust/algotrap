use super::TaResult;
use super::kernel::{Kernel, PriorState};

/// Owns a kernel and its committed state across input rows.
pub struct Processor<K: Kernel> {
    kernel: K,
    state: Option<K::State>,
}

impl<K: Kernel> Processor<K> {
    /// Creates a processor with no committed state.
    pub fn new(kernel: K) -> Self {
        Self {
            kernel,
            state: None,
        }
    }

    /// Feeds one input to the kernel and commits returned state only on success.
    pub fn process(&mut self, input: &K::Input) -> TaResult<K::Output> {
        let prior = match self.state.as_ref() {
            None => PriorState::Initial,
            Some(state) => PriorState::Existing(state),
        };
        let step = self.kernel.transition(prior, input)?;
        self.state = Some(step.next_state);
        Ok(step.output)
    }
}

#[cfg(test)]
mod tests {
    use super::Processor;
    use crate::ta::kernel::{Kernel, KernelStep, PriorState};

    struct PriorDependentProbe;

    impl Kernel for PriorDependentProbe {
        type Input = i32;
        type Output = i32;
        type State = i32;

        fn transition(
            &self,
            prior: PriorState<'_, Self::State>,
            input: &Self::Input,
        ) -> crate::ta::TaResult<KernelStep<Self::Output, Self::State>> {
            let prior_value = match prior {
                PriorState::Initial => 0,
                PriorState::Existing(value) => *value,
            };

            if *input < 0 {
                return Err(crate::ta::TaError::validation("negative probe input"));
            }

            let next_state = prior_value + *input;
            Ok(KernelStep {
                output: next_state,
                next_state,
            })
        }
    }

    #[test]
    fn processor_selects_initial_state_and_returns_output_only() {
        let mut processor = Processor::new(PriorDependentProbe);

        assert_eq!(processor.process(&3).unwrap(), 3);
    }

    #[test]
    fn processor_selects_existing_state_after_successful_commit() {
        let mut processor = Processor::new(PriorDependentProbe);

        assert_eq!(processor.process(&3).unwrap(), 3);
        assert_eq!(processor.process(&4).unwrap(), 7);
    }

    #[test]
    fn processor_failure_preserves_last_committed_state_for_retry() {
        let mut processor = Processor::new(PriorDependentProbe);
        let mut control = Processor::new(PriorDependentProbe);

        assert_eq!(processor.process(&3).unwrap(), 3);
        assert_eq!(control.process(&3).unwrap(), 3);
        assert!(processor.process(&-1).is_err());
        let retry = processor.process(&4).unwrap();
        let control_output = control.process(&4).unwrap();

        assert_eq!(retry, control_output);
    }

    #[test]
    fn exact_generic_processor_use_site_compiles() {
        fn process_probe<P>(
            processor: &mut Processor<P>,
            input: &P::Input,
        ) -> crate::ta::TaResult<P::Output>
        where
            P: Kernel<Input = i32, Output = i32, State = i32>,
        {
            processor.process(input)
        }

        let mut processor = Processor::new(PriorDependentProbe);
        assert_eq!(process_probe(&mut processor, &2).unwrap(), 2);
    }
}
