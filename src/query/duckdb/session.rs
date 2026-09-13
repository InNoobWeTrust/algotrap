//! Nestable, in-memory DuckDB connection scopes for frame projections.
//!
//! Newcomer guide: DuckDB `Connection`s are not `Send`/`Sync`-friendly to share
//! across threads, so each OS thread lazily owns one in-memory connection in
//! the `SESSION` thread-local. `with_session` hands out a short `&Connection`
//! borrow for exactly one `project` step — callers never own or close it
//! directly. `depth` counts nested scopes on the same thread so an outer query
//! reuses the same connection instead of opening a second one; the connection
//! is dropped only when the outermost scope exits.

use std::cell::RefCell;
use std::rc::Rc;

use duckdb::Connection;

use crate::engine::error::MarketError;

/// Thread-local connection plus nesting depth.
/// `Rc` (not `Arc`) is enough because everything here stays on one thread;
/// `depth` is what makes the scope nestable rather than exclusive.
struct SessionState {
    connection: Option<Rc<Connection>>,
    depth: usize,
}

thread_local! {
    static SESSION: RefCell<SessionState> = const {
        RefCell::new(SessionState {
            connection: None,
            depth: 0,
        })
    };
}

/// RAII exit guard: every `with_session` holds one until the operation ends.
/// Dropping it decrements `depth` and drops the thread-local connection only
/// at zero, so early returns and `?` errors still unwind the scope correctly
/// without leaking a connection into the next unrelated query.
struct SessionExit;

impl Drop for SessionExit {
    fn drop(&mut self) {
        SESSION.with(|session| {
            let mut state = session.borrow_mut();
            state.depth = state.depth.saturating_sub(1);
            if state.depth == 0 {
                state.connection.take();
            }
        });
    }
}

/// Runs an operation within a nestable in-memory DuckDB connection scope.
/// Opens the thread-local connection on first use, bumps `depth`, runs the
/// closure, then lets `SessionExit` unwind the depth on drop. Nested calls
/// reuse the same connection; the `Rc` clone keeps it alive only for the
/// duration of the closure.
pub(super) fn with_session<T>(
    operation: impl FnOnce(&Connection) -> Result<T, MarketError>,
) -> Result<T, MarketError> {
    let connection = SESSION.with(|session| {
        let mut state = session.borrow_mut();
        if state.connection.is_none() {
            state.connection = Some(Rc::new(Connection::open_in_memory().map_err(|error| {
                MarketError::data_access(format!(
                    "failed to open in-memory DuckDB connection: {error}"
                ))
            })?));
        }
        state.depth += 1;
        state.connection.as_ref().cloned().ok_or_else(|| {
            MarketError::invocation_lifecycle("active query session has no connection")
        })
    })?;
    let _exit = SessionExit;
    operation(connection.as_ref())
}
