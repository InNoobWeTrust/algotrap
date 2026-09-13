use super::TaResult;

/// Describes whether a kernel transition starts without state or borrows prior state.
pub enum PriorState<'a, S> {
    /// No state has been committed by an earlier transition.
    Initial,
    /// Borrows state committed by the preceding successful transition.
    Existing(&'a S),
}

/// Output and successor state produced by one kernel transition.
pub struct KernelStep<O, S> {
    pub output: O,
    pub next_state: S,
}

/// Defines a stateful indicator as an immutable transition function.
///
/// Each transition receives borrowed prior state and returns both the current-row output and
/// successor state.
pub trait Kernel {
    /// Row value consumed by one transition.
    type Input;
    /// Value emitted for the current row.
    type Output;
    /// Owned state committed after a successful transition.
    type State;

    /// Computes the current output and successor state from prior state and input.
    fn transition(
        &self,
        prior: PriorState<'_, Self::State>,
        input: &Self::Input,
    ) -> TaResult<KernelStep<Self::Output, Self::State>>;
}

#[cfg(test)]
mod tests {
    use crate::ta::{TaError, TaResult};

    use super::{Kernel, KernelStep, PriorState};

    struct TransitionProbe;

    impl Kernel for TransitionProbe {
        type Input = i32;
        type Output = i32;
        type State = i32;

        fn transition(
            &self,
            prior: PriorState<'_, Self::State>,
            input: &Self::Input,
        ) -> TaResult<KernelStep<Self::Output, Self::State>> {
            let prior_value = match prior {
                PriorState::Initial => 0,
                PriorState::Existing(value) => *value,
            };

            if *input < 0 {
                return Err(TaError::validation("negative probe input"));
            }

            let next_state = prior_value + *input;
            Ok(KernelStep {
                output: next_state,
                next_state,
            })
        }
    }

    #[test]
    fn transition_use_site_has_exact_associated_types_and_borrowed_inputs() {
        let state = 5;
        let input = 2;
        let step = TransitionProbe
            .transition(PriorState::Existing(&state), &input)
            .unwrap();

        assert_eq!(step.output, 7);
        assert_eq!(step.next_state, 7);
    }
}
