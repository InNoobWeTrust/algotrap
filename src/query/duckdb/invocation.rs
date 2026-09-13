//! Per-session handoff of completed frames to the DuckDB table callback.
//!
//! Newcomer guide: DuckDB calls our `vtab` scan back on the same thread that
//! ran the SQL, but that callback signature cannot take our `SourceFrame` as a
//! parameter. So `with_frame` parks the frame in the `INVOCATION_SLOT`
//! thread-local, the `bind` phase reclaims it by value, and `claim_next_chunk`
//! hands out monotonically increasing row windows. Only one frame may be
//! parked per thread — a nested `project` errors instead of overwriting the
//! outer frame — and the slot is always cleared by RAII.

use std::cell::RefCell;
use std::sync::Arc;

use crate::engine::error::MarketError;
use crate::engine::frame::SourceFrame;

/// Parked frame plus how far the single-threaded scan has progressed.
/// `Arc` lets `project` share the frame with the slot until `bind` unwraps
/// sole ownership; `scan_cursor` is a plain offset because the table function
/// is pinned to one thread (`MAX_SCAN_THREADS = 1`), so no atomic or lock is
/// needed — parallel scans would race this cursor.
struct InvocationSlot {
    frame: Option<Arc<SourceFrame>>,
    scan_cursor: usize,
}

thread_local! {
    static INVOCATION_SLOT: RefCell<InvocationSlot> = const {
        RefCell::new(InvocationSlot {
            frame: None,
            scan_cursor: 0,
        })
    };
}

/// RAII reset: held for the whole `with_frame` operation, clearing the parked
/// frame and cursor on drop so a panic or `?` error never leaks one query's
/// frame into the next query on this thread.
struct InvocationSlotReset;

impl Drop for InvocationSlotReset {
    fn drop(&mut self) {
        INVOCATION_SLOT.with(|slot| {
            let mut slot = slot.borrow_mut();
            slot.frame.take();
            slot.scan_cursor = 0;
        });
    }
}

/// Parks `frame` for the DuckDB scan callback while `operation` runs.
/// Rejects nesting (an already-parked frame errors) so two concurrent scans
/// can never share one slot; the `_reset` guard clears the slot even when
/// `operation` fails.
pub(super) fn with_frame<T>(
    frame: Arc<SourceFrame>,
    operation: impl FnOnce() -> Result<T, MarketError>,
) -> Result<T, MarketError> {
    INVOCATION_SLOT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.frame.is_some() {
            return Err(MarketError::invocation_lifecycle(
                "a frame query is already active on this worker",
            ));
        }
        slot.frame = Some(frame);
        slot.scan_cursor = 0;
        Ok(())
    })?;
    let _reset = InvocationSlotReset;
    operation()
}

/// Called once by `bind` to reclaim the parked frame.
/// Taking (not cloning) enforces the single-owner rule: after this the slot is
/// empty, so a second `computed()` reference in the same SQL cannot steal the
/// same frame, and calling `computed()` with no active `project` errors.
pub(super) fn take_frame_for_bind() -> Result<Arc<SourceFrame>, MarketError> {
    INVOCATION_SLOT.with(|slot| {
        slot.borrow_mut().frame.take().ok_or_else(|| {
            MarketError::invocation_lifecycle("computed was called without an active frame query")
        })
    })
}

/// Claims the next `[start, start + capacity)` row window for the scan.
/// Returns `None` once `scan_cursor` reaches `row_count`, which tells `func`
/// to emit an empty chunk and end the scan. Safe with plain `usize` math only
/// because the scan is pinned to one thread; `saturating_add` guards against
/// overflow on absurd capacities.
pub(super) fn claim_next_chunk(row_count: usize, capacity: usize) -> Option<usize> {
    INVOCATION_SLOT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.scan_cursor >= row_count {
            return None;
        }
        let start = slot.scan_cursor;
        slot.scan_cursor = slot.scan_cursor.saturating_add(capacity);
        Some(start)
    })
}
