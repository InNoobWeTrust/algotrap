use std::pin::Pin;

use futures::StreamExt;

use super::error::StreamPipelineError;
use super::stamped::Stamped;
use crate::ta::kernel::Kernel;
use crate::ta::processor::Processor;

// Minimal conditional Clone required by the locked `broadcast_source` bounds
// (`Id: Clone, T: Clone`) against `tokio::sync::broadcast`, which requires the
// full message type to be `Clone` for `channel`/`recv`. Manual impl, not a
// derive, and no additional public bound is added to any locked signature.
impl<Id, T> Clone for Stamped<Id, T>
where
    Id: Clone,
    T: Clone,
{
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            value: self.value.clone(),
        }
    }
}

fn kernel_stream_with_normalizer<S, K, Id, SrcErr, E, F>(
    source: S,
    kernel: K,
    normalize_source_error: F,
) -> impl futures::Stream<Item = Result<Stamped<Id, K::Output>, StreamPipelineError<E, Id>>>
where
    S: futures::Stream<Item = Result<Stamped<Id, K::Input>, SrcErr>>,
    K: Kernel,
    F: FnMut(SrcErr) -> StreamPipelineError<E, Id>,
{
    struct SharedState<S, K: Kernel, F> {
        source: Pin<Box<S>>,
        processor: Processor<K>,
        normalize_source_error: F,
        terminal: bool,
    }

    let state = SharedState {
        source: Box::pin(source),
        processor: Processor::new(kernel),
        normalize_source_error,
        terminal: false,
    };

    futures::stream::unfold(state, |mut state| async move {
        if state.terminal {
            return None;
        }
        match state.source.next().await {
            // `Fuse` provides permanently fused `None`; terminal latch prevents
            // further upstream/processor/normalizer polls before fusion.
            None => None,
            Some(Err(source_error)) => {
                state.terminal = true;
                let normalized = (state.normalize_source_error)(source_error);
                Some((Err(normalized), state))
            }
            Some(Ok(Stamped { id, value })) => match state.processor.process(&value) {
                Ok(output) => Some((Ok(Stamped { id, value: output }), state)),
                Err(ta_error) => {
                    state.terminal = true;
                    Some((Err(StreamPipelineError::Ta(ta_error)), state))
                }
            },
        }
    })
    .fuse()
}

/// Runs a stamped source stream through `kernel`, preserving identifiers and mapping source errors.
pub fn kernel_stream<S, K, Id, E, F>(
    source: S,
    kernel: K,
    map_source_error: F,
) -> impl futures::Stream<Item = Result<Stamped<Id, K::Output>, StreamPipelineError<E, Id>>>
where
    S: futures::Stream<Item = Result<Stamped<Id, K::Input>, E>>,
    K: Kernel,
    F: FnMut(E) -> StreamPipelineError<E, Id>,
{
    kernel_stream_with_normalizer(source, kernel, map_source_error)
}

/// Runs an already normalized pipeline stream through `kernel` without nesting errors.
pub fn kernel_stream_pipeline<S, K, Id, E>(
    source: S,
    kernel: K,
) -> impl futures::Stream<Item = Result<Stamped<Id, K::Output>, StreamPipelineError<E, Id>>>
where
    S: futures::Stream<Item = Result<Stamped<Id, K::Input>, StreamPipelineError<E, Id>>>,
    K: Kernel,
{
    kernel_stream_with_normalizer(source, kernel, std::convert::identity)
}

/// Adapts a broadcast receiver into a fused stream, emitting lag errors and ending on close.
pub fn broadcast_source<Id, T>(
    receiver: tokio::sync::broadcast::Receiver<Stamped<Id, T>>,
) -> impl futures::Stream<Item = Result<Stamped<Id, T>, StreamPipelineError<std::convert::Infallible, Id>>>
where
    Id: Clone,
    T: Clone,
{
    struct BroadcastState<Id, T> {
        receiver: tokio::sync::broadcast::Receiver<Stamped<Id, T>>,
        terminal: bool,
    }

    let state = BroadcastState {
        receiver,
        terminal: false,
    };

    futures::stream::unfold(state, |mut state| async move {
        if state.terminal {
            return None;
        }
        match state.receiver.recv().await {
            Ok(stamped) => Some((Ok(stamped), state)),
            // Buffered values drain first; `Closed` becomes fused `None` via `Fuse`.
            Err(tokio::sync::broadcast::error::RecvError::Closed) => None,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                state.terminal = true;
                Some((Err(StreamPipelineError::Lagged { skipped }), state))
            }
        }
    })
    .fuse()
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::marker::PhantomPinned;
    use std::task::{Context, Poll};

    use futures::StreamExt;

    use crate::adapter::{Stamped, StreamPipelineError};
    use crate::adapter::{broadcast_source, kernel_stream, kernel_stream_pipeline};
    use crate::ta::TaResult;
    use crate::ta::kernel::{Kernel, KernelStep, PriorState};

    struct ProbeKernel;

    impl Kernel for ProbeKernel {
        type Input = u32;
        type Output = u32;
        type State = u32;

        fn transition(
            &self,
            prior: PriorState<'_, Self::State>,
            input: &Self::Input,
        ) -> TaResult<KernelStep<Self::Output, Self::State>> {
            let prior_value = match prior {
                PriorState::Initial => 0,
                PriorState::Existing(value) => *value,
            };
            let next_state = prior_value + *input;
            Ok(KernelStep {
                output: next_state,
                next_state,
            })
        }
    }

    #[allow(dead_code)]
    struct NoCloneId(u64);

    #[allow(dead_code)]
    struct NoCloneError(String);

    struct NotUnpin<S> {
        inner: RefCell<S>,
        _marker: PhantomPinned,
    }

    impl<S> futures::Stream for NotUnpin<S>
    where
        S: futures::Stream + Unpin,
    {
        type Item = S::Item;

        fn poll_next(
            self: std::pin::Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Option<Self::Item>> {
            let this: &Self = &self;
            let mut borrowed = this.inner.borrow_mut();
            std::pin::Pin::new(&mut *borrowed).poll_next(cx)
        }
    }

    fn assert_kernel_stream_item<S>(_: S)
    where
        S: futures::Stream<Item = Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>>,
    {
    }

    fn assert_pipeline_stream_item<S>(_: S)
    where
        S: futures::Stream<Item = Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>>,
    {
    }

    fn assert_broadcast_stream_item<S>(_: S)
    where
        S: futures::Stream<
                Item = Result<
                    Stamped<u64, u32>,
                    StreamPipelineError<std::convert::Infallible, u64>,
                >,
            >,
    {
    }

    fn assert_send_sync_static<T>()
    where
        T: Send + Sync + 'static,
    {
    }

    #[test]
    fn stamped_fields_are_exact_public() {
        let stamped = Stamped {
            id: 1u64,
            value: 2u32,
        };
        let Stamped { id, value } = stamped;
        assert_eq!(id, 1u64);
        assert_eq!(value, 2u32);
    }

    #[test]
    fn pipeline_error_variants_have_exact_shapes() {
        let source: StreamPipelineError<String, u64> =
            StreamPipelineError::Source("boom".to_string());
        match source {
            StreamPipelineError::Source(inner) => assert_eq!(inner, "boom"),
            _ => panic!("expected Source"),
        }

        let lagged: StreamPipelineError<String, u64> = StreamPipelineError::Lagged { skipped: 7 };
        match lagged {
            StreamPipelineError::Lagged { skipped } => assert_eq!(skipped, 7u64),
            _ => panic!("expected Lagged"),
        }

        let ta_error = crate::ta::TaError::validation("bad input");
        let ta: StreamPipelineError<String, u64> = StreamPipelineError::Ta(ta_error);
        match ta {
            StreamPipelineError::Ta(_) => {}
            _ => panic!("expected Ta"),
        }

        let alignment: StreamPipelineError<String, u64> = StreamPipelineError::Alignment {
            left: 1u64,
            right: 2u64,
        };
        match alignment {
            StreamPipelineError::Alignment { left, right } => {
                assert_eq!(left, 1u64);
                assert_eq!(right, 2u64);
            }
            _ => panic!("expected Alignment"),
        }

        let closed: StreamPipelineError<String, u64> = StreamPipelineError::OutputClosed;
        match closed {
            StreamPipelineError::OutputClosed => {}
            _ => panic!("expected OutputClosed"),
        }

        let joined: StreamPipelineError<String, u64> = StreamPipelineError::TaskJoin {
            message: "joined".to_string(),
        };
        match joined {
            StreamPipelineError::TaskJoin { message } => assert_eq!(message, "joined"),
            _ => panic!("expected TaskJoin"),
        }
    }

    #[test]
    fn kernel_stream_returns_exact_item_type() {
        let source = futures::stream::empty::<Result<Stamped<u64, u32>, String>>();
        let stream = kernel_stream(source, ProbeKernel, StreamPipelineError::Source);
        assert_kernel_stream_item(stream);
    }

    #[test]
    fn kernel_stream_pipeline_returns_exact_item_type() {
        let source =
            futures::stream::empty::<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>>();
        let stream = kernel_stream_pipeline(source, ProbeKernel);
        assert_pipeline_stream_item(stream);
    }

    #[test]
    fn broadcast_source_returns_exact_item_type() {
        let (_sender, receiver) = tokio::sync::broadcast::channel::<Stamped<u64, u32>>(16);
        let stream = broadcast_source(receiver);
        assert_broadcast_stream_item(stream);
    }

    #[test]
    fn not_unpin_sources_compile_with_both_kernel_adapters() {
        let inner = futures::stream::empty::<Result<Stamped<u64, u32>, String>>();
        let source = NotUnpin {
            inner: RefCell::new(inner),
            _marker: PhantomPinned,
        };
        let stream = kernel_stream(source, ProbeKernel, StreamPipelineError::Source);
        assert_kernel_stream_item(stream);

        let inner =
            futures::stream::empty::<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>>();
        let source = NotUnpin {
            inner: RefCell::new(inner),
            _marker: PhantomPinned,
        };
        let stream = kernel_stream_pipeline(source, ProbeKernel);
        assert_pipeline_stream_item(stream);
    }

    #[test]
    fn non_clone_id_and_error_compile_where_clone_not_required() {
        let source = futures::stream::empty::<Result<Stamped<NoCloneId, u32>, NoCloneError>>();
        let stream = kernel_stream(source, ProbeKernelForNoClone, |e: NoCloneError| {
            StreamPipelineError::Source(e)
        });
        fn assert_no_clone_kernel<S>(_: S)
        where
            S: futures::Stream<
                    Item = Result<
                        Stamped<NoCloneId, u32>,
                        StreamPipelineError<NoCloneError, NoCloneId>,
                    >,
                >,
        {
        }
        assert_no_clone_kernel(stream);

        let source = futures::stream::empty::<
            Result<Stamped<NoCloneId, u32>, StreamPipelineError<NoCloneError, NoCloneId>>,
        >();
        let stream = kernel_stream_pipeline(source, ProbeKernelForNoClone);
        assert_no_clone_kernel(stream);
    }

    struct ProbeKernelForNoClone;

    impl Kernel for ProbeKernelForNoClone {
        type Input = u32;
        type Output = u32;
        type State = u32;

        fn transition(
            &self,
            prior: PriorState<'_, Self::State>,
            input: &Self::Input,
        ) -> TaResult<KernelStep<Self::Output, Self::State>> {
            let prior_value = match prior {
                PriorState::Initial => 0,
                PriorState::Existing(value) => *value,
            };
            let next_state = prior_value + *input;
            Ok(KernelStep {
                output: next_state,
                next_state,
            })
        }
    }

    #[test]
    fn eligible_auto_traits_hold_for_send_sync_static_params() {
        assert_send_sync_static::<Stamped<u64, u32>>();
        assert_send_sync_static::<StreamPipelineError<String, u64>>();
        assert_send_sync_static::<StreamPipelineError<std::convert::Infallible, u64>>();
    }

    #[test]
    fn crate_adapter_path_exposes_all_five_items() {
        let _stamped: crate::adapter::Stamped<u64, u32> = crate::adapter::Stamped {
            id: 0u64,
            value: 0u32,
        };
        let _error: crate::adapter::StreamPipelineError<String, u64> =
            crate::adapter::StreamPipelineError::OutputClosed;
        fn _uses_kernel_stream() -> impl futures::Stream<
            Item = Result<
                crate::adapter::Stamped<u64, u32>,
                crate::adapter::StreamPipelineError<String, u64>,
            >,
        > {
            let source =
                futures::stream::empty::<Result<crate::adapter::Stamped<u64, u32>, String>>();
            crate::adapter::kernel_stream(
                source,
                ProbeKernel,
                crate::adapter::StreamPipelineError::Source,
            )
        }
        fn _uses_pipeline() -> impl futures::Stream<
            Item = Result<
                crate::adapter::Stamped<u64, u32>,
                crate::adapter::StreamPipelineError<String, u64>,
            >,
        > {
            let source = futures::stream::empty::<
                Result<
                    crate::adapter::Stamped<u64, u32>,
                    crate::adapter::StreamPipelineError<String, u64>,
                >,
            >();
            crate::adapter::kernel_stream_pipeline(source, ProbeKernel)
        }
        fn _uses_broadcast(
            receiver: tokio::sync::broadcast::Receiver<crate::adapter::Stamped<u64, u32>>,
        ) -> impl futures::Stream<
            Item = Result<
                crate::adapter::Stamped<u64, u32>,
                crate::adapter::StreamPipelineError<std::convert::Infallible, u64>,
            >,
        > {
            crate::adapter::broadcast_source(receiver)
        }
        let _ = _uses_kernel_stream;
        let _ = _uses_pipeline;
        let _ = _uses_broadcast;
    }

    struct WarmupKernel;

    impl Kernel for WarmupKernel {
        type Input = u32;
        type Output = Option<u32>;
        type State = u32;

        fn transition(
            &self,
            prior: PriorState<'_, Self::State>,
            input: &Self::Input,
        ) -> TaResult<KernelStep<Self::Output, Self::State>> {
            match prior {
                PriorState::Initial => Ok(KernelStep {
                    output: None,
                    next_state: *input,
                }),
                PriorState::Existing(previous) => {
                    let next_state = *previous + *input;
                    Ok(KernelStep {
                        output: Some(next_state),
                        next_state,
                    })
                }
            }
        }
    }

    #[test]
    fn kernel_stream_preserves_supplied_order_ids_and_cardinality() {
        let inputs = vec![
            (10u64, 1u32),
            (20u64, 2u32),
            (30u64, 3u32),
            (40u64, 4u32),
            (50u64, 5u32),
        ];
        let source = futures::stream::iter(
            inputs
                .into_iter()
                .map(|(id, value)| Ok::<_, String>(Stamped { id, value })),
        );
        let stream = kernel_stream(source, ProbeKernel, StreamPipelineError::Source);
        let outputs: Vec<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>> =
            futures::executor::block_on(stream.collect());
        assert_eq!(outputs.len(), 5);
        let mut ids = Vec::new();
        let mut values = Vec::new();
        for item in outputs {
            match item {
                Ok(stamped) => {
                    ids.push(stamped.id);
                    values.push(stamped.value);
                }
                Err(_) => panic!("expected success item"),
            }
        }
        assert_eq!(ids, vec![10u64, 20u64, 30u64, 40u64, 50u64]);
        assert_eq!(values, vec![1u32, 3u32, 6u32, 10u32, 15u32]);
    }

    #[test]
    fn kernel_stream_uses_single_persistent_processor() {
        let source = futures::stream::iter(vec![
            Ok::<_, String>(Stamped {
                id: 1u64,
                value: 1u32,
            }),
            Ok::<_, String>(Stamped {
                id: 2u64,
                value: 2u32,
            }),
            Ok::<_, String>(Stamped {
                id: 3u64,
                value: 3u32,
            }),
        ]);
        let stream = kernel_stream(source, ProbeKernel, StreamPipelineError::Source);
        let outputs: Vec<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>> =
            futures::executor::block_on(stream.collect());
        assert_eq!(outputs.len(), 3);
        let mut values = Vec::new();
        for item in outputs {
            match item {
                Ok(stamped) => values.push(stamped.value),
                Err(_) => panic!("expected success item"),
            }
        }
        // ProbeKernel accumulates through one persistent Processor: 1, 1+2, 1+2+3.
        // Fresh per-item processors would emit 1, 2, 3 instead.
        assert_eq!(values, vec![1u32, 3u32, 6u32]);
    }

    #[test]
    fn kernel_stream_emits_warmup_none_output_stamped() {
        let source = futures::stream::iter(vec![
            Ok::<_, String>(Stamped {
                id: 7u64,
                value: 10u32,
            }),
            Ok::<_, String>(Stamped {
                id: 8u64,
                value: 5u32,
            }),
        ]);
        let stream = kernel_stream(source, WarmupKernel, StreamPipelineError::Source);
        let outputs = futures::executor::block_on(stream.collect::<Vec<_>>());
        assert_eq!(outputs.len(), 2);
        match &outputs[0] {
            Ok(stamped) => {
                assert_eq!(stamped.id, 7u64);
                assert_eq!(stamped.value, None);
            }
            Err(_) => panic!("warm-up None output must still be emitted"),
        }
        match &outputs[1] {
            Ok(stamped) => {
                assert_eq!(stamped.id, 8u64);
                assert_eq!(stamped.value, Some(15u32));
            }
            Err(_) => panic!("expected success item"),
        }
    }

    #[test]
    fn two_kernel_streams_have_independent_state() {
        let make_source = || {
            futures::stream::iter(vec![
                Ok::<_, String>(Stamped {
                    id: 1u64,
                    value: 1u32,
                }),
                Ok::<_, String>(Stamped {
                    id: 2u64,
                    value: 2u32,
                }),
                Ok::<_, String>(Stamped {
                    id: 3u64,
                    value: 3u32,
                }),
            ])
        };
        let first: Vec<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>> =
            futures::executor::block_on(
                kernel_stream(make_source(), ProbeKernel, StreamPipelineError::Source).collect(),
            );
        let second: Vec<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>> =
            futures::executor::block_on(
                kernel_stream(make_source(), ProbeKernel, StreamPipelineError::Source).collect(),
            );
        fn values_of(
            outputs: Vec<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>>,
        ) -> Vec<u32> {
            let mut values = Vec::new();
            for item in outputs {
                match item {
                    Ok(stamped) => values.push(stamped.value),
                    Err(_) => panic!("expected success item"),
                }
            }
            values
        }
        assert_eq!(first.len(), 3);
        assert_eq!(second.len(), 3);
        // Both streams start from Initial with the same kernel configuration.
        // Shared state would continue the second stream from 6 instead of restarting.
        assert_eq!(values_of(first), vec![1u32, 3u32, 6u32]);
        assert_eq!(values_of(second), vec![1u32, 3u32, 6u32]);
    }

    struct CountingKernelU23 {
        calls: std::rc::Rc<std::cell::Cell<usize>>,
    }

    impl Kernel for CountingKernelU23 {
        type Input = u32;
        type Output = u32;
        type State = u32;

        fn transition(
            &self,
            prior: PriorState<'_, Self::State>,
            input: &Self::Input,
        ) -> TaResult<KernelStep<Self::Output, Self::State>> {
            self.calls.set(self.calls.get() + 1);
            let prior_value = match prior {
                PriorState::Initial => 0,
                PriorState::Existing(value) => *value,
            };
            let next_state = prior_value + *input;
            Ok(KernelStep {
                output: next_state,
                next_state,
            })
        }
    }

    struct CountingSourceU23<S> {
        inner: S,
        polls: std::rc::Rc<std::cell::Cell<usize>>,
    }

    impl<S> futures::Stream for CountingSourceU23<S>
    where
        S: futures::Stream + Unpin,
    {
        type Item = S::Item;

        fn poll_next(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Option<Self::Item>> {
            let this = self.as_mut().get_mut();
            this.polls.set(this.polls.get() + 1);
            std::pin::Pin::new(&mut this.inner).poll_next(cx)
        }
    }

    #[test]
    fn kernel_stream_classifies_raw_error_once_and_latches_terminal() {
        use std::cell::Cell;
        use std::rc::Rc;

        let upstream_polls = Rc::new(Cell::new(0usize));
        let kernel_calls = Rc::new(Cell::new(0usize));
        let classifier_calls = Rc::new(Cell::new(0usize));
        let inner = futures::stream::iter(vec![
            Ok::<_, String>(Stamped {
                id: 1u64,
                value: 10u32,
            }),
            Err("boom".to_string()),
            Ok(Stamped {
                id: 99u64,
                value: 99u32,
            }),
        ]);
        let source = CountingSourceU23 {
            inner,
            polls: upstream_polls.clone(),
        };
        let classifier_calls_cloned = classifier_calls.clone();
        let stream = kernel_stream(
            source,
            CountingKernelU23 {
                calls: kernel_calls.clone(),
            },
            move |e: String| {
                classifier_calls_cloned.set(classifier_calls_cloned.get() + 1);
                StreamPipelineError::Source(e)
            },
        );
        futures::pin_mut!(stream);
        let first: Option<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>> =
            futures::executor::block_on(stream.next());
        match first {
            Some(Ok(stamped)) => {
                assert_eq!(stamped.id, 1u64);
                assert_eq!(stamped.value, 10u32);
            }
            _ => panic!("expected first success item"),
        }
        assert_eq!(classifier_calls.get(), 0);
        let kernel_after_first = kernel_calls.get();
        assert_eq!(kernel_after_first, 1);
        let second: Option<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>> =
            futures::executor::block_on(stream.next());
        match second {
            Some(Err(StreamPipelineError::Source(inner))) => {
                assert_eq!(inner, "boom".to_string());
            }
            _ => panic!("expected classifier-selected Source payload"),
        }
        assert_eq!(classifier_calls.get(), 1);
        let polls_at_error = upstream_polls.get();
        let kernel_at_error = kernel_calls.get();
        assert_eq!(kernel_at_error, kernel_after_first);
        for _ in 0..3 {
            let next: Option<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>> =
                futures::executor::block_on(stream.next());
            match next {
                None => {}
                _ => panic!("expected fused None after terminal source error"),
            }
        }
        assert_eq!(classifier_calls.get(), 1);
        assert_eq!(upstream_polls.get(), polls_at_error);
        assert_eq!(kernel_calls.get(), kernel_at_error);
    }

    #[test]
    fn kernel_stream_pipeline_preserves_errors_without_nesting_and_fuses() {
        use std::cell::Cell;
        use std::rc::Rc;

        type PipelineTestItem = Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>;

        fn run_pipeline_case(
            error: StreamPipelineError<String, u64>,
            expect: fn(Option<PipelineTestItem>),
        ) {
            let upstream_polls = Rc::new(Cell::new(0usize));
            let kernel_calls = Rc::new(Cell::new(0usize));
            let inner = futures::stream::iter(vec![
                Ok::<_, StreamPipelineError<String, u64>>(Stamped {
                    id: 1u64,
                    value: 10u32,
                }),
                Err(error),
                Ok(Stamped {
                    id: 99u64,
                    value: 99u32,
                }),
            ]);
            let source = CountingSourceU23 {
                inner,
                polls: upstream_polls.clone(),
            };
            let stream = kernel_stream_pipeline(
                source,
                CountingKernelU23 {
                    calls: kernel_calls.clone(),
                },
            );
            futures::pin_mut!(stream);
            let first: Option<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>> =
                futures::executor::block_on(stream.next());
            match first {
                Some(Ok(stamped)) => {
                    assert_eq!(stamped.id, 1u64);
                    assert_eq!(stamped.value, 10u32);
                }
                _ => panic!("expected first success item"),
            }
            assert_eq!(kernel_calls.get(), 1);
            let second: Option<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>> =
                futures::executor::block_on(stream.next());
            expect(second);
            let polls_at_error = upstream_polls.get();
            let kernel_at_error = kernel_calls.get();
            assert_eq!(kernel_at_error, 1);
            for _ in 0..3 {
                let next: Option<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>> =
                    futures::executor::block_on(stream.next());
                match next {
                    None => {}
                    _ => panic!("expected fused None after terminal pipeline error"),
                }
            }
            assert_eq!(upstream_polls.get(), polls_at_error);
            assert_eq!(kernel_calls.get(), kernel_at_error);
        }

        run_pipeline_case(
            StreamPipelineError::Source("boom".to_string()),
            |second| match second {
                Some(Err(StreamPipelineError::Source(inner))) => {
                    assert_eq!(inner, "boom".to_string());
                }
                _ => panic!("expected unchanged Source without nesting"),
            },
        );
        run_pipeline_case(
            StreamPipelineError::Lagged { skipped: 7 },
            |second| match second {
                Some(Err(StreamPipelineError::Lagged { skipped })) => {
                    assert_eq!(skipped, 7u64);
                }
                _ => panic!("expected unchanged Lagged without nesting"),
            },
        );
        run_pipeline_case(
            StreamPipelineError::Alignment {
                left: 1u64,
                right: 2u64,
            },
            |second| match second {
                Some(Err(StreamPipelineError::Alignment { left, right })) => {
                    assert_eq!(left, 1u64);
                    assert_eq!(right, 2u64);
                }
                _ => panic!("expected unchanged Alignment without nesting"),
            },
        );
        run_pipeline_case(StreamPipelineError::OutputClosed, |second| match second {
            Some(Err(StreamPipelineError::OutputClosed)) => {}
            _ => panic!("expected unchanged OutputClosed without nesting"),
        });
        run_pipeline_case(
            StreamPipelineError::TaskJoin {
                message: "joined".to_string(),
            },
            |second| match second {
                Some(Err(StreamPipelineError::TaskJoin { message })) => {
                    assert_eq!(message, "joined".to_string());
                }
                _ => panic!("expected unchanged TaskJoin without nesting"),
            },
        );
    }

    struct FailOnValueKernelU24 {
        calls: std::rc::Rc<std::cell::Cell<usize>>,
        fail_on: u32,
    }

    impl Kernel for FailOnValueKernelU24 {
        type Input = u32;
        type Output = u32;
        type State = u32;

        fn transition(
            &self,
            prior: PriorState<'_, Self::State>,
            input: &Self::Input,
        ) -> TaResult<KernelStep<Self::Output, Self::State>> {
            self.calls.set(self.calls.get() + 1);
            if *input == self.fail_on {
                return Err(crate::ta::TaError::validation("injected ta failure"));
            }
            let prior_value = match prior {
                PriorState::Initial => 0,
                PriorState::Existing(value) => *value,
            };
            let next_state = prior_value + *input;
            Ok(KernelStep {
                output: next_state,
                next_state,
            })
        }
    }

    #[test]
    fn kernel_stream_maps_selected_ta_failure_to_single_terminal_ta() {
        use std::cell::Cell;
        use std::rc::Rc;

        let upstream_polls = Rc::new(Cell::new(0usize));
        let kernel_calls = Rc::new(Cell::new(0usize));
        let classifier_calls = Rc::new(Cell::new(0usize));
        let inner = futures::stream::iter(vec![
            Ok::<_, String>(Stamped {
                id: 1u64,
                value: 10u32,
            }),
            Ok(Stamped {
                id: 2u64,
                value: 999u32,
            }),
            Ok(Stamped {
                id: 99u64,
                value: 99u32,
            }),
        ]);
        let source = CountingSourceU23 {
            inner,
            polls: upstream_polls.clone(),
        };
        let classifier_calls_cloned = classifier_calls.clone();
        let stream = kernel_stream(
            source,
            FailOnValueKernelU24 {
                calls: kernel_calls.clone(),
                fail_on: 999u32,
            },
            move |e: String| {
                classifier_calls_cloned.set(classifier_calls_cloned.get() + 1);
                StreamPipelineError::Source(e)
            },
        );
        futures::pin_mut!(stream);
        let first: Option<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>> =
            futures::executor::block_on(stream.next());
        match first {
            Some(Ok(stamped)) => {
                assert_eq!(stamped.id, 1u64);
                assert_eq!(stamped.value, 10u32);
            }
            _ => panic!("expected earlier success before injected TA failure"),
        }
        assert_eq!(kernel_calls.get(), 1);
        assert_eq!(classifier_calls.get(), 0);
        let second: Option<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>> =
            futures::executor::block_on(stream.next());
        match second {
            Some(Err(StreamPipelineError::Ta(_))) => {}
            Some(Ok(_)) => panic!("TA failure item must not emit stamped success"),
            Some(Err(StreamPipelineError::TaskJoin { .. })) => {
                panic!("TA failure must not synthesize TaskJoin")
            }
            Some(Err(StreamPipelineError::OutputClosed)) => {
                panic!("TA failure must not synthesize OutputClosed")
            }
            Some(Err(StreamPipelineError::Alignment { .. })) => {
                panic!("TA failure must not synthesize Alignment")
            }
            Some(Err(_)) => panic!("expected exactly Ta for injected processor failure"),
            None => panic!("expected one Ta error for injected processor failure"),
        }
        assert_eq!(kernel_calls.get(), 2);
        assert_eq!(classifier_calls.get(), 0);
        let polls_at_error = upstream_polls.get();
        let kernel_at_error = kernel_calls.get();
        for _ in 0..3 {
            let next: Option<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>> =
                futures::executor::block_on(stream.next());
            match next {
                None => {}
                _ => panic!("expected fused None after terminal TA failure"),
            }
        }
        assert_eq!(classifier_calls.get(), 0);
        assert_eq!(upstream_polls.get(), polls_at_error);
        assert_eq!(kernel_calls.get(), kernel_at_error);
    }

    #[test]
    fn kernel_stream_pipeline_maps_selected_ta_failure_to_single_terminal_ta() {
        use std::cell::Cell;
        use std::rc::Rc;

        let upstream_polls = Rc::new(Cell::new(0usize));
        let kernel_calls = Rc::new(Cell::new(0usize));
        let inner = futures::stream::iter(vec![
            Ok::<_, StreamPipelineError<String, u64>>(Stamped {
                id: 1u64,
                value: 10u32,
            }),
            Ok(Stamped {
                id: 2u64,
                value: 999u32,
            }),
            Ok(Stamped {
                id: 99u64,
                value: 99u32,
            }),
        ]);
        let source = CountingSourceU23 {
            inner,
            polls: upstream_polls.clone(),
        };
        let stream = kernel_stream_pipeline(
            source,
            FailOnValueKernelU24 {
                calls: kernel_calls.clone(),
                fail_on: 999u32,
            },
        );
        futures::pin_mut!(stream);
        let first: Option<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>> =
            futures::executor::block_on(stream.next());
        match first {
            Some(Ok(stamped)) => {
                assert_eq!(stamped.id, 1u64);
                assert_eq!(stamped.value, 10u32);
            }
            _ => panic!("expected earlier success before injected TA failure"),
        }
        assert_eq!(kernel_calls.get(), 1);
        let second: Option<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>> =
            futures::executor::block_on(stream.next());
        match second {
            Some(Err(StreamPipelineError::Ta(_))) => {}
            Some(Ok(_)) => panic!("TA failure item must not emit stamped success"),
            Some(Err(StreamPipelineError::TaskJoin { .. })) => {
                panic!("TA failure must not synthesize TaskJoin")
            }
            Some(Err(StreamPipelineError::OutputClosed)) => {
                panic!("TA failure must not synthesize OutputClosed")
            }
            Some(Err(StreamPipelineError::Alignment { .. })) => {
                panic!("TA failure must not synthesize Alignment")
            }
            Some(Err(_)) => panic!("expected exactly Ta for injected processor failure"),
            None => panic!("expected one Ta error for injected processor failure"),
        }
        assert_eq!(kernel_calls.get(), 2);
        let polls_at_error = upstream_polls.get();
        let kernel_at_error = kernel_calls.get();
        for _ in 0..3 {
            let next: Option<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>> =
                futures::executor::block_on(stream.next());
            match next {
                None => {}
                _ => panic!("expected fused None after terminal TA failure"),
            }
        }
        assert_eq!(upstream_polls.get(), polls_at_error);
        assert_eq!(kernel_calls.get(), kernel_at_error);
    }

    // U2.5 — broadcast close, lag, and composition semantics.
    #[test]
    fn broadcast_close_drains_buffered_then_fuses() {
        let (sender, receiver) = tokio::sync::broadcast::channel::<Stamped<u64, u32>>(16);
        sender
            .send(Stamped {
                id: 10u64,
                value: 1u32,
            })
            .ok()
            .expect("send with live receiver");
        sender
            .send(Stamped {
                id: 20u64,
                value: 2u32,
            })
            .ok()
            .expect("send with live receiver");
        sender
            .send(Stamped {
                id: 30u64,
                value: 3u32,
            })
            .ok()
            .expect("send with live receiver");
        drop(sender);
        let stream = broadcast_source(receiver);
        futures::pin_mut!(stream);
        let first: Option<
            Result<Stamped<u64, u32>, StreamPipelineError<std::convert::Infallible, u64>>,
        > = futures::executor::block_on(stream.next());
        match first {
            Some(Ok(stamped)) => {
                assert_eq!(stamped.id, 10u64);
                assert_eq!(stamped.value, 1u32);
            }
            _ => panic!("expected first buffered broadcast value in order"),
        }
        let second: Option<
            Result<Stamped<u64, u32>, StreamPipelineError<std::convert::Infallible, u64>>,
        > = futures::executor::block_on(stream.next());
        match second {
            Some(Ok(stamped)) => {
                assert_eq!(stamped.id, 20u64);
                assert_eq!(stamped.value, 2u32);
            }
            _ => panic!("expected second buffered broadcast value in order"),
        }
        let third: Option<
            Result<Stamped<u64, u32>, StreamPipelineError<std::convert::Infallible, u64>>,
        > = futures::executor::block_on(stream.next());
        match third {
            Some(Ok(stamped)) => {
                assert_eq!(stamped.id, 30u64);
                assert_eq!(stamped.value, 3u32);
            }
            _ => panic!("expected third buffered broadcast value in order"),
        }
        for _ in 0..3 {
            let next: Option<
                Result<Stamped<u64, u32>, StreamPipelineError<std::convert::Infallible, u64>>,
            > = futures::executor::block_on(stream.next());
            match next {
                None => {}
                _ => panic!("expected fused None after broadcast close"),
            }
        }
    }

    #[test]
    fn broadcast_lagged_is_single_terminal_error_with_exact_skipped() {
        let (sender, receiver) = tokio::sync::broadcast::channel::<Stamped<u64, u32>>(2);
        sender
            .send(Stamped {
                id: 1u64,
                value: 10u32,
            })
            .ok()
            .expect("send with live receiver");
        sender
            .send(Stamped {
                id: 2u64,
                value: 20u32,
            })
            .ok()
            .expect("send with live receiver");
        // Third send overwrites the first buffered value; exactly one message is skipped.
        sender
            .send(Stamped {
                id: 3u64,
                value: 30u32,
            })
            .ok()
            .expect("send with live receiver");
        let stream = broadcast_source(receiver);
        futures::pin_mut!(stream);
        let first: Option<
            Result<Stamped<u64, u32>, StreamPipelineError<std::convert::Infallible, u64>>,
        > = futures::executor::block_on(stream.next());
        match first {
            Some(Err(StreamPipelineError::Lagged { skipped })) => {
                assert_eq!(skipped, 1u64);
            }
            _ => panic!("expected exactly one Lagged with skipped=1"),
        }
        // Behavioral no-recv evidence: the receiver is still owned by the
        // terminal stream, so this send succeeds, yet the fused stream never
        // observes it.
        sender
            .send(Stamped {
                id: 99u64,
                value: 99u32,
            })
            .ok()
            .expect("send after lag with live receiver");
        for _ in 0..3 {
            let next: Option<
                Result<Stamped<u64, u32>, StreamPipelineError<std::convert::Infallible, u64>>,
            > = futures::executor::block_on(stream.next());
            match next {
                None => {}
                _ => panic!("expected fused None after terminal Lagged"),
            }
        }
    }

    #[test]
    fn broadcast_pipeline_composition_preserves_success_ids_and_cardinality() {
        let (sender, receiver) = tokio::sync::broadcast::channel::<Stamped<u64, u32>>(16);
        sender
            .send(Stamped {
                id: 10u64,
                value: 1u32,
            })
            .ok()
            .expect("send with live receiver");
        sender
            .send(Stamped {
                id: 20u64,
                value: 2u32,
            })
            .ok()
            .expect("send with live receiver");
        sender
            .send(Stamped {
                id: 30u64,
                value: 3u32,
            })
            .ok()
            .expect("send with live receiver");
        drop(sender);
        let stream = kernel_stream_pipeline(broadcast_source(receiver), ProbeKernel);
        futures::pin_mut!(stream);
        let first: Option<
            Result<Stamped<u64, u32>, StreamPipelineError<std::convert::Infallible, u64>>,
        > = futures::executor::block_on(stream.next());
        match first {
            Some(Ok(stamped)) => {
                assert_eq!(stamped.id, 10u64);
                assert_eq!(stamped.value, 1u32);
            }
            _ => panic!("expected first composed success with preserved id"),
        }
        let second: Option<
            Result<Stamped<u64, u32>, StreamPipelineError<std::convert::Infallible, u64>>,
        > = futures::executor::block_on(stream.next());
        match second {
            Some(Ok(stamped)) => {
                assert_eq!(stamped.id, 20u64);
                assert_eq!(stamped.value, 3u32);
            }
            _ => panic!("expected second composed success with preserved id"),
        }
        let third: Option<
            Result<Stamped<u64, u32>, StreamPipelineError<std::convert::Infallible, u64>>,
        > = futures::executor::block_on(stream.next());
        match third {
            Some(Ok(stamped)) => {
                assert_eq!(stamped.id, 30u64);
                assert_eq!(stamped.value, 6u32);
            }
            _ => panic!("expected third composed success with preserved id"),
        }
        for _ in 0..2 {
            let next: Option<
                Result<Stamped<u64, u32>, StreamPipelineError<std::convert::Infallible, u64>>,
            > = futures::executor::block_on(stream.next());
            match next {
                None => {}
                _ => panic!("expected fused None after composed close"),
            }
        }
    }

    #[test]
    fn broadcast_pipeline_composition_preserves_lagged_without_nesting() {
        let (sender, receiver) = tokio::sync::broadcast::channel::<Stamped<u64, u32>>(2);
        sender
            .send(Stamped {
                id: 1u64,
                value: 10u32,
            })
            .ok()
            .expect("send with live receiver");
        sender
            .send(Stamped {
                id: 2u64,
                value: 20u32,
            })
            .ok()
            .expect("send with live receiver");
        sender
            .send(Stamped {
                id: 3u64,
                value: 30u32,
            })
            .ok()
            .expect("send with live receiver");
        let stream = kernel_stream_pipeline(broadcast_source(receiver), ProbeKernel);
        futures::pin_mut!(stream);
        let first: Option<
            Result<Stamped<u64, u32>, StreamPipelineError<std::convert::Infallible, u64>>,
        > = futures::executor::block_on(stream.next());
        match first {
            Some(Err(StreamPipelineError::Lagged { skipped })) => {
                assert_eq!(skipped, 1u64);
            }
            Some(Err(_)) => panic!("composed Lagged must not be nested or rewritten"),
            Some(Ok(_)) => panic!("lagged composition must not emit success"),
            None => panic!("expected one terminal Lagged through composition"),
        }
        for _ in 0..3 {
            let next: Option<
                Result<Stamped<u64, u32>, StreamPipelineError<std::convert::Infallible, u64>>,
            > = futures::executor::block_on(stream.next());
            match next {
                None => {}
                _ => panic!("expected fused None after composed Lagged"),
            }
        }
    }

    // U2.6 — pinning and fused terminal behavior.
    #[test]
    fn u26_custom_not_unpin_runs_through_both_kernel_adapters() {
        let inner = futures::stream::iter(vec![
            Ok::<_, String>(Stamped {
                id: 1u64,
                value: 1u32,
            }),
            Ok(Stamped {
                id: 2u64,
                value: 2u32,
            }),
            Ok(Stamped {
                id: 3u64,
                value: 3u32,
            }),
        ]);
        let source = NotUnpin {
            inner: RefCell::new(inner),
            _marker: PhantomPinned,
        };
        let stream = kernel_stream(source, ProbeKernel, StreamPipelineError::Source);
        let outputs: Vec<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>> =
            futures::executor::block_on(stream.collect());
        assert_eq!(outputs.len(), 3);
        let mut ids = Vec::new();
        let mut values = Vec::new();
        for item in outputs {
            match item {
                Ok(stamped) => {
                    ids.push(stamped.id);
                    values.push(stamped.value);
                }
                Err(_) => panic!("custom !Unpin kernel_stream must emit success"),
            }
        }
        assert_eq!(ids, vec![1u64, 2u64, 3u64]);
        assert_eq!(values, vec![1u32, 3u32, 6u32]);

        let inner = futures::stream::iter(vec![
            Ok::<_, StreamPipelineError<String, u64>>(Stamped {
                id: 1u64,
                value: 1u32,
            }),
            Ok(Stamped {
                id: 2u64,
                value: 2u32,
            }),
            Ok(Stamped {
                id: 3u64,
                value: 3u32,
            }),
        ]);
        let source = NotUnpin {
            inner: RefCell::new(inner),
            _marker: PhantomPinned,
        };
        let stream = kernel_stream_pipeline(source, ProbeKernel);
        let outputs: Vec<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>> =
            futures::executor::block_on(stream.collect());
        assert_eq!(outputs.len(), 3);
        let mut ids = Vec::new();
        let mut values = Vec::new();
        for item in outputs {
            match item {
                Ok(stamped) => {
                    ids.push(stamped.id);
                    values.push(stamped.value);
                }
                Err(_) => panic!("custom !Unpin pipeline must emit success"),
            }
        }
        assert_eq!(ids, vec![1u64, 2u64, 3u64]);
        assert_eq!(values, vec![1u32, 3u32, 6u32]);
    }

    #[test]
    fn u26_combinator_not_unpin_runs_through_both_kernel_adapters() {
        // `Filter` pins its inner stream, so `Filter<NotUnpin<..>, _>` remains
        // `!Unpin` via the pinned custom inner. Filtering with an always-true
        // predicate preserves both items to exercise combinator pinning
        // without changing expected values or cardinality. This is a
        // combinator-produced `!Unpin` stream through each applicable kernel
        // adapter.
        let inner = futures::stream::iter(vec![
            Ok::<_, String>(Stamped {
                id: 10u64,
                value: 1u32,
            }),
            Ok(Stamped {
                id: 20u64,
                value: 2u32,
            }),
        ]);
        let not_unpin = NotUnpin {
            inner: RefCell::new(inner),
            _marker: PhantomPinned,
        };
        let combined = futures::StreamExt::filter(not_unpin, |_| futures::future::ready(true));
        let stream = kernel_stream(combined, ProbeKernel, StreamPipelineError::Source);
        let outputs: Vec<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>> =
            futures::executor::block_on(stream.collect());
        assert_eq!(outputs.len(), 2);
        match &outputs[0] {
            Ok(stamped) => {
                assert_eq!(stamped.id, 10u64);
                assert_eq!(stamped.value, 1u32);
            }
            Err(_) => panic!("combinator !Unpin kernel_stream must emit success"),
        }
        match &outputs[1] {
            Ok(stamped) => {
                assert_eq!(stamped.id, 20u64);
                assert_eq!(stamped.value, 3u32);
            }
            Err(_) => panic!("combinator !Unpin kernel_stream must emit success"),
        }

        let inner = futures::stream::iter(vec![
            Ok::<_, StreamPipelineError<String, u64>>(Stamped {
                id: 10u64,
                value: 1u32,
            }),
            Ok(Stamped {
                id: 20u64,
                value: 2u32,
            }),
        ]);
        let not_unpin = NotUnpin {
            inner: RefCell::new(inner),
            _marker: PhantomPinned,
        };
        let combined = futures::StreamExt::filter(not_unpin, |_| futures::future::ready(true));
        let stream = kernel_stream_pipeline(combined, ProbeKernel);
        let outputs: Vec<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>> =
            futures::executor::block_on(stream.collect());
        assert_eq!(outputs.len(), 2);
        match &outputs[0] {
            Ok(stamped) => {
                assert_eq!(stamped.id, 10u64);
                assert_eq!(stamped.value, 1u32);
            }
            Err(_) => panic!("combinator !Unpin pipeline must emit success"),
        }
        match &outputs[1] {
            Ok(stamped) => {
                assert_eq!(stamped.id, 20u64);
                assert_eq!(stamped.value, 3u32);
            }
            Err(_) => panic!("combinator !Unpin pipeline must emit success"),
        }
    }

    #[test]
    fn u26_source_none_fuses_without_further_touch() {
        use std::cell::Cell;
        use std::rc::Rc;

        let upstream_polls = Rc::new(Cell::new(0usize));
        let kernel_calls = Rc::new(Cell::new(0usize));
        let classifier_calls = Rc::new(Cell::new(0usize));
        let inner = futures::stream::empty::<Result<Stamped<u64, u32>, String>>();
        let source = CountingSourceU23 {
            inner,
            polls: upstream_polls.clone(),
        };
        let classifier_cloned = classifier_calls.clone();
        let stream = kernel_stream(
            source,
            CountingKernelU23 {
                calls: kernel_calls.clone(),
            },
            move |e: String| {
                classifier_cloned.set(classifier_cloned.get() + 1);
                StreamPipelineError::Source(e)
            },
        );
        futures::pin_mut!(stream);
        let first: Option<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>> =
            futures::executor::block_on(stream.next());
        match first {
            None => {}
            _ => panic!("expected fused None for empty kernel_stream source"),
        }
        assert_eq!(kernel_calls.get(), 0);
        assert_eq!(classifier_calls.get(), 0);
        let polls_after_first = upstream_polls.get();
        for _ in 0..3 {
            let next: Option<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>> =
                futures::executor::block_on(stream.next());
            match next {
                None => {}
                _ => panic!("expected fused None after source None"),
            }
        }
        assert_eq!(upstream_polls.get(), polls_after_first);
        assert_eq!(kernel_calls.get(), 0);
        assert_eq!(classifier_calls.get(), 0);

        let upstream_polls = Rc::new(Cell::new(0usize));
        let kernel_calls = Rc::new(Cell::new(0usize));
        let inner =
            futures::stream::empty::<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>>();
        let source = CountingSourceU23 {
            inner,
            polls: upstream_polls.clone(),
        };
        let stream = kernel_stream_pipeline(
            source,
            CountingKernelU23 {
                calls: kernel_calls.clone(),
            },
        );
        futures::pin_mut!(stream);
        let first: Option<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>> =
            futures::executor::block_on(stream.next());
        match first {
            None => {}
            _ => panic!("expected fused None for empty pipeline source"),
        }
        assert_eq!(kernel_calls.get(), 0);
        let polls_after_first = upstream_polls.get();
        for _ in 0..3 {
            let next: Option<Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>> =
                futures::executor::block_on(stream.next());
            match next {
                None => {}
                _ => panic!("expected fused None after pipeline source None"),
            }
        }
        assert_eq!(upstream_polls.get(), polls_after_first);
        assert_eq!(kernel_calls.get(), 0);
    }

    #[test]
    fn u26_broadcast_close_fuses_without_further_recv_or_processor_touch() {
        use std::cell::Cell;
        use std::rc::Rc;

        let (sender, receiver) = tokio::sync::broadcast::channel::<Stamped<u64, u32>>(16);
        sender
            .send(Stamped {
                id: 10u64,
                value: 1u32,
            })
            .ok()
            .expect("send with live receiver");
        sender
            .send(Stamped {
                id: 20u64,
                value: 2u32,
            })
            .ok()
            .expect("send with live receiver");
        drop(sender);
        let stream = broadcast_source(receiver);
        futures::pin_mut!(stream);
        let first: Option<
            Result<Stamped<u64, u32>, StreamPipelineError<std::convert::Infallible, u64>>,
        > = futures::executor::block_on(stream.next());
        match first {
            Some(Ok(stamped)) => {
                assert_eq!(stamped.id, 10u64);
                assert_eq!(stamped.value, 1u32);
            }
            _ => panic!("expected first buffered close value"),
        }
        let second: Option<
            Result<Stamped<u64, u32>, StreamPipelineError<std::convert::Infallible, u64>>,
        > = futures::executor::block_on(stream.next());
        match second {
            Some(Ok(stamped)) => {
                assert_eq!(stamped.id, 20u64);
                assert_eq!(stamped.value, 2u32);
            }
            _ => panic!("expected second buffered close value"),
        }
        for _ in 0..3 {
            let next: Option<
                Result<Stamped<u64, u32>, StreamPipelineError<std::convert::Infallible, u64>>,
            > = futures::executor::block_on(stream.next());
            match next {
                None => {}
                _ => panic!("expected fused None after broadcast close"),
            }
        }

        let (sender, receiver) = tokio::sync::broadcast::channel::<Stamped<u64, u32>>(16);
        sender
            .send(Stamped {
                id: 10u64,
                value: 1u32,
            })
            .ok()
            .expect("send with live receiver");
        sender
            .send(Stamped {
                id: 20u64,
                value: 2u32,
            })
            .ok()
            .expect("send with live receiver");
        drop(sender);
        let kernel_calls = Rc::new(Cell::new(0usize));
        let stream = kernel_stream_pipeline(
            broadcast_source(receiver),
            CountingKernelU23 {
                calls: kernel_calls.clone(),
            },
        );
        futures::pin_mut!(stream);
        let first: Option<
            Result<Stamped<u64, u32>, StreamPipelineError<std::convert::Infallible, u64>>,
        > = futures::executor::block_on(stream.next());
        match first {
            Some(Ok(stamped)) => {
                assert_eq!(stamped.id, 10u64);
                assert_eq!(stamped.value, 1u32);
            }
            _ => panic!("expected first composed close success"),
        }
        assert_eq!(kernel_calls.get(), 1);
        let second: Option<
            Result<Stamped<u64, u32>, StreamPipelineError<std::convert::Infallible, u64>>,
        > = futures::executor::block_on(stream.next());
        match second {
            Some(Ok(stamped)) => {
                assert_eq!(stamped.id, 20u64);
                assert_eq!(stamped.value, 3u32);
            }
            _ => panic!("expected second composed close success"),
        }
        assert_eq!(kernel_calls.get(), 2);
        let closed: Option<
            Result<Stamped<u64, u32>, StreamPipelineError<std::convert::Infallible, u64>>,
        > = futures::executor::block_on(stream.next());
        match closed {
            None => {}
            _ => panic!("expected fused None at composed broadcast close"),
        }
        let kernel_at_close = kernel_calls.get();
        for _ in 0..3 {
            let next: Option<
                Result<Stamped<u64, u32>, StreamPipelineError<std::convert::Infallible, u64>>,
            > = futures::executor::block_on(stream.next());
            match next {
                None => {}
                _ => panic!("expected fused None after composed broadcast close"),
            }
        }
        assert_eq!(kernel_calls.get(), kernel_at_close);
    }

    #[test]
    fn u26_broadcast_lag_fuses_without_further_recv_or_processor_touch() {
        use std::cell::Cell;
        use std::rc::Rc;

        let (sender, receiver) = tokio::sync::broadcast::channel::<Stamped<u64, u32>>(2);
        sender
            .send(Stamped {
                id: 1u64,
                value: 10u32,
            })
            .ok()
            .expect("send with live receiver");
        sender
            .send(Stamped {
                id: 2u64,
                value: 20u32,
            })
            .ok()
            .expect("send with live receiver");
        sender
            .send(Stamped {
                id: 3u64,
                value: 30u32,
            })
            .ok()
            .expect("send with live receiver");
        let stream = broadcast_source(receiver);
        futures::pin_mut!(stream);
        let first: Option<
            Result<Stamped<u64, u32>, StreamPipelineError<std::convert::Infallible, u64>>,
        > = futures::executor::block_on(stream.next());
        match first {
            Some(Err(StreamPipelineError::Lagged { skipped })) => {
                assert_eq!(skipped, 1u64);
            }
            _ => panic!("expected exactly one Lagged with skipped=1"),
        }
        sender
            .send(Stamped {
                id: 99u64,
                value: 99u32,
            })
            .ok()
            .expect("send after lag with live receiver");
        for _ in 0..3 {
            let next: Option<
                Result<Stamped<u64, u32>, StreamPipelineError<std::convert::Infallible, u64>>,
            > = futures::executor::block_on(stream.next());
            match next {
                None => {}
                _ => panic!("expected fused None after terminal Lagged without recv"),
            }
        }

        let (sender, receiver) = tokio::sync::broadcast::channel::<Stamped<u64, u32>>(2);
        sender
            .send(Stamped {
                id: 1u64,
                value: 10u32,
            })
            .ok()
            .expect("send with live receiver");
        sender
            .send(Stamped {
                id: 2u64,
                value: 20u32,
            })
            .ok()
            .expect("send with live receiver");
        sender
            .send(Stamped {
                id: 3u64,
                value: 30u32,
            })
            .ok()
            .expect("send with live receiver");
        let kernel_calls = Rc::new(Cell::new(0usize));
        let stream = kernel_stream_pipeline(
            broadcast_source(receiver),
            CountingKernelU23 {
                calls: kernel_calls.clone(),
            },
        );
        futures::pin_mut!(stream);
        let first: Option<
            Result<Stamped<u64, u32>, StreamPipelineError<std::convert::Infallible, u64>>,
        > = futures::executor::block_on(stream.next());
        match first {
            Some(Err(StreamPipelineError::Lagged { skipped })) => {
                assert_eq!(skipped, 1u64);
            }
            Some(Err(_)) => panic!("composed Lagged must not be nested or rewritten"),
            Some(Ok(_)) => panic!("lagged composition must not emit success"),
            None => panic!("expected one terminal Lagged through composition"),
        }
        assert_eq!(kernel_calls.get(), 0);
        // Behavioral no-recv/no-processor evidence: the receiver is still owned
        // by the terminal composition, so this send succeeds, yet the fused
        // composition never observes it. The send must precede the first
        // post-terminal `None`, which drops the owned receiver state.
        sender
            .send(Stamped {
                id: 99u64,
                value: 99u32,
            })
            .ok()
            .expect("send after composed lag with live receiver");
        let kernel_at_lag = kernel_calls.get();
        for _ in 0..3 {
            let next: Option<
                Result<Stamped<u64, u32>, StreamPipelineError<std::convert::Infallible, u64>>,
            > = futures::executor::block_on(stream.next());
            match next {
                None => {}
                _ => panic!("expected fused None after composed Lagged"),
            }
        }
        assert_eq!(kernel_calls.get(), kernel_at_lag);
        for _ in 0..2 {
            let next: Option<
                Result<Stamped<u64, u32>, StreamPipelineError<std::convert::Infallible, u64>>,
            > = futures::executor::block_on(stream.next());
            match next {
                None => {}
                _ => panic!("expected fused None after composed Lagged without processor touch"),
            }
        }
        assert_eq!(kernel_calls.get(), kernel_at_lag);
    }

    // U2.7 — drop cancellation and task-join ownership.
    #[test]
    fn u27_drop_pending_kernel_stream_releases_upstream_and_owned_state_without_another_poll() {
        use std::cell::Cell;
        use std::rc::Rc;
        use std::task::{Context, Poll};

        struct PendingGuardSource {
            polls: Rc<Cell<usize>>,
            drops: Rc<Cell<usize>>,
        }

        impl Drop for PendingGuardSource {
            fn drop(&mut self) {
                self.drops.set(self.drops.get() + 1);
            }
        }

        impl futures::Stream for PendingGuardSource {
            type Item = Result<Stamped<u64, u32>, String>;

            fn poll_next(
                self: std::pin::Pin<&mut Self>,
                _cx: &mut Context<'_>,
            ) -> Poll<Option<Self::Item>> {
                let this = self.get_mut();
                this.polls.set(this.polls.get() + 1);
                Poll::Pending
            }
        }

        struct DropKernel {
            drops: Rc<Cell<usize>>,
        }

        impl Drop for DropKernel {
            fn drop(&mut self) {
                self.drops.set(self.drops.get() + 1);
            }
        }

        impl Kernel for DropKernel {
            type Input = u32;
            type Output = u32;
            type State = u32;

            fn transition(
                &self,
                prior: PriorState<'_, Self::State>,
                input: &Self::Input,
            ) -> TaResult<KernelStep<Self::Output, Self::State>> {
                let prior_value = match prior {
                    PriorState::Initial => 0,
                    PriorState::Existing(value) => *value,
                };
                let next_state = prior_value + *input;
                Ok(KernelStep {
                    output: next_state,
                    next_state,
                })
            }
        }

        struct ClassifierGuard {
            drops: Rc<Cell<usize>>,
        }

        impl Drop for ClassifierGuard {
            fn drop(&mut self) {
                self.drops.set(self.drops.get() + 1);
            }
        }

        let source_polls = Rc::new(Cell::new(0usize));
        let source_drops = Rc::new(Cell::new(0usize));
        let kernel_drops = Rc::new(Cell::new(0usize));
        let classifier_drops = Rc::new(Cell::new(0usize));

        let source = PendingGuardSource {
            polls: source_polls.clone(),
            drops: source_drops.clone(),
        };
        let kernel = DropKernel {
            drops: kernel_drops.clone(),
        };
        let guard = ClassifierGuard {
            drops: classifier_drops.clone(),
        };
        let stream = kernel_stream(source, kernel, move |e: String| {
            let _ = &guard;
            StreamPipelineError::Source(e)
        });
        let mut stream = Box::pin(stream);
        let waker = futures::task::noop_waker();
        let mut cx = Context::from_waker(&waker);
        match futures::Stream::poll_next(stream.as_mut(), &mut cx) {
            Poll::Pending => {}
            _ => panic!("expected Pending for pending kernel_stream source"),
        }
        assert_eq!(source_polls.get(), 1);
        assert_eq!(source_drops.get(), 0);
        assert_eq!(kernel_drops.get(), 0);
        assert_eq!(classifier_drops.get(), 0);
        drop(stream);
        // Dropping the pending adapter synchronously drops upstream,
        // processor-owned kernel, and classifier without another poll and
        // without emitting a synthetic item.
        assert_eq!(source_polls.get(), 1);
        assert_eq!(source_drops.get(), 1);
        assert_eq!(kernel_drops.get(), 1);
        assert_eq!(classifier_drops.get(), 1);
    }

    #[test]
    fn u27_drop_pending_pipeline_releases_upstream_without_another_poll() {
        use std::cell::Cell;
        use std::rc::Rc;
        use std::task::{Context, Poll};

        struct PendingPipelineSource {
            polls: Rc<Cell<usize>>,
            drops: Rc<Cell<usize>>,
        }

        impl Drop for PendingPipelineSource {
            fn drop(&mut self) {
                self.drops.set(self.drops.get() + 1);
            }
        }

        impl futures::Stream for PendingPipelineSource {
            type Item = Result<Stamped<u64, u32>, StreamPipelineError<String, u64>>;

            fn poll_next(
                self: std::pin::Pin<&mut Self>,
                _cx: &mut Context<'_>,
            ) -> Poll<Option<Self::Item>> {
                let this = self.get_mut();
                this.polls.set(this.polls.get() + 1);
                Poll::Pending
            }
        }

        struct DropPipelineKernel {
            drops: Rc<Cell<usize>>,
        }

        impl Drop for DropPipelineKernel {
            fn drop(&mut self) {
                self.drops.set(self.drops.get() + 1);
            }
        }

        impl Kernel for DropPipelineKernel {
            type Input = u32;
            type Output = u32;
            type State = u32;

            fn transition(
                &self,
                prior: PriorState<'_, Self::State>,
                input: &Self::Input,
            ) -> TaResult<KernelStep<Self::Output, Self::State>> {
                let prior_value = match prior {
                    PriorState::Initial => 0,
                    PriorState::Existing(value) => *value,
                };
                let next_state = prior_value + *input;
                Ok(KernelStep {
                    output: next_state,
                    next_state,
                })
            }
        }

        let source_polls = Rc::new(Cell::new(0usize));
        let source_drops = Rc::new(Cell::new(0usize));
        let kernel_drops = Rc::new(Cell::new(0usize));

        let source = PendingPipelineSource {
            polls: source_polls.clone(),
            drops: source_drops.clone(),
        };
        let kernel = DropPipelineKernel {
            drops: kernel_drops.clone(),
        };
        let stream = kernel_stream_pipeline(source, kernel);
        let mut stream = Box::pin(stream);
        let waker = futures::task::noop_waker();
        let mut cx = Context::from_waker(&waker);
        match futures::Stream::poll_next(stream.as_mut(), &mut cx) {
            Poll::Pending => {}
            _ => panic!("expected Pending for pending pipeline source"),
        }
        assert_eq!(source_polls.get(), 1);
        assert_eq!(source_drops.get(), 0);
        assert_eq!(kernel_drops.get(), 0);
        drop(stream);
        assert_eq!(source_polls.get(), 1);
        assert_eq!(source_drops.get(), 1);
        assert_eq!(kernel_drops.get(), 1);
    }

    #[test]
    fn u27_drop_broadcast_source_releases_receiver() {
        use std::task::{Context, Poll};

        let (sender, receiver) = tokio::sync::broadcast::channel::<Stamped<u64, u32>>(16);
        assert_eq!(sender.receiver_count(), 1);
        let stream = broadcast_source(receiver);
        // The adapter now owns the sole receiver.
        assert_eq!(sender.receiver_count(), 1);
        let mut stream = Box::pin(stream);
        let waker = futures::task::noop_waker();
        let mut cx = Context::from_waker(&waker);
        match futures::Stream::poll_next(stream.as_mut(), &mut cx) {
            Poll::Pending => {}
            _ => panic!("expected Pending for empty broadcast_source"),
        }
        // Still owned while pending; no synthetic close or lag item.
        assert_eq!(sender.receiver_count(), 1);
        drop(stream);
        // Dropping the pending adapter drops its receiver and any pending
        // receive future; no synthetic OutputClosed or TaskJoin is emitted.
        assert_eq!(sender.receiver_count(), 0);
    }

    #[test]
    fn u27_application_supervisor_converts_join_error_via_to_string() {
        // Manually built root Tokio runtime; no Tokio macros are used.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build current-thread runtime without macros");
        runtime.block_on(async {
            // Application owns the spawned consuming task; adapters spawn none.
            let handle: tokio::task::JoinHandle<()> = tokio::spawn(async move {
                let source = futures::stream::iter(vec![Ok::<_, String>(Stamped {
                    id: 1u64,
                    value: 1u32,
                })]);
                let consuming = kernel_stream(source, ProbeKernel, StreamPipelineError::Source);
                futures::pin_mut!(consuming);
                match futures::StreamExt::next(&mut consuming).await {
                    Some(Ok(stamped)) => {
                        assert_eq!(stamped.id, 1u64);
                    }
                    _ => panic!("expected consuming task to observe one success"),
                }
                panic!("deliberate application consuming-task failure");
            });
            match handle.await {
                Ok(()) => panic!("expected JoinError from deliberately failing task"),
                Err(join_error) => {
                    // Supervisor boundary consumes JoinError via to_string().
                    let message: String = join_error.to_string();
                    assert!(!message.is_empty());
                    let normalized: StreamPipelineError<String, u64> =
                        StreamPipelineError::TaskJoin {
                            message: message.clone(),
                        };
                    match normalized {
                        StreamPipelineError::TaskJoin { message: inner } => {
                            assert_eq!(inner, message);
                        }
                        _ => panic!("expected exact TaskJoin at supervisor boundary"),
                    }
                }
            }
        });
    }
}
