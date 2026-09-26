# Phase 3 — Public Signal Facade + Integration Tests

**Parent L1:** [`l1.md`](../l1.md) §7.3
**Goal:** Wire the public `plum_blossom_signal` entry points, enforce the `IchingSignal` struct-field contract at the public boundary, and deliver the full integration test suite that satisfies all research §6 Stage 1 verification gates.

---

## p3-u1-iching-signal-validation-factory

### Goal
Implement the `IchingSignal` construction and field-validation logic in `signal.rs`, ensuring every field contract is enforced at the public boundary before any facade function exists.

### Writable surface
| File | Action |
|------|--------|
| `src/ta/iching/signal.rs` | `[CREATE]` — `IchingSignal` validated construction and internal helpers |
| `src/ta/iching/mod.rs` | `[MODIFY]` — add `pub mod signal;` (Build-Progression Rule; signal is the public facade submodule) |

### Locked contracts
```rust
// src/ta/iching/mod.rs — addition (signal submodule now exists)
pub mod signal;
```
```rust
// src/ta/iching/signal.rs

use crate::ta::{TaError, TaResult};
use super::types::{HexagramEnergy, IchingSignal};

/// Constructs a validated `IchingSignal` from separate channel components.
///
/// Validation rules (all must pass or the result is `Err`):
/// - `original.hexagram ∈ 0..=63` (enforced by `HexagramEnergy::new`)
/// - `original.energy == original.hexagram as f64 - 31.5` (enforced by factory)
/// - `transformed`: if `Some`, same invariant as `original`
/// - `mutual.hexagram ∈ 0..=63` and `mutual.energy == mutual.hexagram as f64 - 31.5`
/// - `moving_line`: if `Some(v)`, then `v ∈ 1..=6`
///
/// This function is `pub(crate)` — only the facade and tests call it.
pub(crate) fn build_signal(
    original: HexagramEnergy,
    transformed: Option<HexagramEnergy>,
    mutual: HexagramEnergy,
    moving_line: Option<u8>,
) -> TaResult<IchingSignal>;
```

**Implementation approach:**
- Validate every supplied `HexagramEnergy` at the boundary. Although production callers use `HexagramEnergy::new`, its public fields allow an invalid literal; `build_signal` must reject `hexagram > 63` or `energy != hexagram as f64 - 31.5` (compare exact values because both sides use the same integer-to-`f64` conversion and subtraction).
- Validate `original`, `mutual`, and `transformed` when present before constructing the signal.
- Validate `moving_line` range: `if let Some(ml) = moving_line { if !(1..=6).contains(&ml) { return Err(...) } }`.
- Construct and return `IchingSignal { original, transformed, mutual, moving_line }` only after all validations pass.

### Acceptance criteria
- [ ] `build_signal(original, Some(transformed), mutual, Some(3))` returns `Ok(IchingSignal)` for valid inputs
- [ ] `build_signal(..., moving_line: Some(0))` returns `Err` with `kind == Validation`
- [ ] `build_signal(..., moving_line: Some(7))` returns `Err` with `kind == Validation`
- [ ] `build_signal(..., transformed: None, moving_line: None)` returns `Ok` (valid for future methods)
- [ ] `build_signal(..., transformed: None, moving_line: Some(1))` returns `Ok` (Plum Blossom always has `Some` for both, but the builder doesn't enforce coupling)
- [ ] A raw invalid `HexagramEnergy` with `hexagram > 63` or a mismatched `energy` is rejected with `kind == Validation`
- [ ] All tests pass under `cargo test -p algotrap --lib ta::iching::signal`

### Stop condition
Builder validation logic passes all edge-case tests. Ready for facade functions.

### Dependencies
- `p1-u3-hexagram-energy-factory` (needs `HexagramEnergy::new` to create test fixtures)
- `p1-u2-dto-field-definitions` (needs `IchingSignal`, `HexagramEnergy` types)

---

## p3-u2-plum-blossom-signal-facade

### Goal
Implement the two public entry points — `plum_blossom_signal()` and `plum_blossom_signal_with_policy()` — in `signal.rs`, wiring the calendar adapter and Plum Blossom casting pipeline into a single-call public API.

### Writable surface
| File | Action |
|------|--------|
| `src/ta/iching/signal.rs` | `[MODIFY]` — add public facade functions |

### Rationale for MODIFY
The facade functions extend the same `signal.rs` file created in `p3-u1`. They are the only production-code addition to `signal.rs`.

### Locked contracts
```rust
use chrono::{DateTime, Utc};
use super::calendar;
use super::types::LeapMonthPolicy;
use super::plum_blossom;

/// Plum Blossom I-Ching signal with default leap-month policy (`Reject`).
///
/// Time policy: `cst-utc8-fixed-v1`. Deterministic, no I/O, no wall-clock read.
pub fn plum_blossom_signal(datetime: DateTime<Utc>) -> TaResult<IchingSignal> {
    plum_blossom_signal_with_policy(datetime, LeapMonthPolicy::Reject)
}

/// Plum Blossom I-Ching signal with explicit leap-month policy.
///
/// 1. `calendar::to_plum_blossom_input(datetime, policy)` → `PlumBlossomInput`
/// 2. `plum_blossom::compute_channels(input)` → `PlumBlossomResult`
/// 3. `build_signal(original, Some(transformed), mutual, Some(moving_line))` → `IchingSignal`
pub fn plum_blossom_signal_with_policy(
    datetime: DateTime<Utc>,
    policy: LeapMonthPolicy,
) -> TaResult<IchingSignal>;
```

### Acceptance criteria
- [ ] `plum_blossom_signal(fixed_utc)` returns `Ok(IchingSignal)` with all three channels and `moving_line == Some(_)`
- [ ] The returned `IchingSignal.original`, `.transformed`, and `.mutual` all have valid `HexagramEnergy` values
- [ ] `moving_line` is always `Some(1..=6)` for Plum Blossom — never `None`
- [ ] `transformed` is always `Some(_)` for Plum Blossom — never `None`
- [ ] Leap `Reject` propagates as `Validation` through the facade for a known leap-month datetime
- [ ] Leap `Allow` succeeds for the same leap-month datetime
- [ ] No `ext`, `engine`, or `query` imports in `signal.rs`

### Stop condition
Facade functions work end-to-end. Ready for integration tests.

### Dependencies
- `p3-u1-iching-signal-validation-factory` (needs `build_signal`)
- `p2-u3-calendar-to-plum-input-adapter` (needs `to_plum_blossom_input`)
- `p2-u4-plum-blossom-energy-channel-construction` (needs `compute_channels`)

---

## p3-u3-signal-facade-integration-tests

### Goal
Write the comprehensive integration test suite for the full `plum_blossom_signal` pipeline, covering end-to-end computation, midpoint energy fixtures, channel separation, leap policy propagation, and determinism.

### Writable surface
| File | Action |
|------|--------|
| `src/ta/iching/signal.rs` | `[MODIFY]` — add `#[cfg(test)] mod tests` block |

### Rationale for MODIFY
Integration tests are the final addition to `signal.rs`. They exercise the complete pipeline from `DateTime<Utc>` to `IchingSignal`, validating all research §6 gates.

### Locked contracts
No new production types. Tests exercise:
- End-to-end: fixed UTC instant → `plum_blossom_signal()` → verify channels match manual computation
- Midpoint energy: `HexagramEnergy` endpoint and center fixtures
- Channel separation: three channels are independently computed and distinct
- Leap policy: `Reject` vs `Allow` for a known leap-month fixture
- Determinism: calling `plum_blossom_signal(same_dt)` twice yields identical `IchingSignal`
- No `None` channels: Plum Blossom always populates all three `HexagramEnergy` fields

### Test fixtures (normative, L1 §8.2)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plum_signal_end_to_end_fixed_instant() {
        // 2024-02-10T00:00:00Z → CST+08 2024-02-10 08:00
        // Lunar date: 2024-01-01 (Lunar New Year, dragon year)
        // Compute Plum Blossom manually and compare
    }

    #[test]
    fn midpoint_energy_endpoints() {
        // hexagram=0 → energy=-31.5
        // hexagram=63 → energy=+31.5
        let lo = HexagramEnergy::new(0).unwrap();
        assert_eq!(lo.energy, -31.5);
        let hi = HexagramEnergy::new(63).unwrap();
        assert_eq!(hi.energy, 31.5);
    }

    #[test]
    fn midpoint_energy_center_adjacent() {
        // hexagram=31 → energy=-0.5 (below zero)
        // hexagram=32 → energy=+0.5 (above zero)
        // No integer hexagram equals exactly 31.5
        let below = HexagramEnergy::new(31).unwrap();
        assert!((below.energy - (-0.5)).abs() < f64::EPSILON);
        let above = HexagramEnergy::new(32).unwrap();
        assert!((above.energy - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn plum_channels_always_populated() {
        // For any valid Plum Blossom call, original/transformed/mutual are all Some
        let dt = DateTime::parse_from_rfc3339("2024-06-15T12:00:00+08:00")
            .unwrap()
            .with_timezone(&Utc);
        let signal = plum_blossom_signal(dt).unwrap();
        assert!(signal.transformed.is_some());
        assert!(signal.moving_line.is_some());
        assert!(signal.moving_line.unwrap() >= 1);
        assert!(signal.moving_line.unwrap() <= 6);
    }

    #[test]
    fn channel_energy_matches_hexagram_field() {
        // Each channel: energy == hexagram as f64 - 31.5
        let dt = Utc.with_ymd_and_hms(2023, 8, 8, 14, 30, 0).unwrap();
        let signal = plum_blossom_signal(dt).unwrap();
        assert!((signal.original.energy - signal.original.hexagram as f64 + 31.5).abs() < f64::EPSILON);
        assert!((signal.mutual.energy - signal.mutual.hexagram as f64 + 31.5).abs() < f64::EPSILON);
        let transformed = signal.transformed.unwrap();
        assert!((transformed.energy - transformed.hexagram as f64 + 31.5).abs() < f64::EPSILON);
    }

    #[test]
    fn leap_reject_propagates_as_validation() {
        // Find a known leap-month datetime and verify Reject fails
    }

    #[test]
    fn leap_allow_succeeds() {
        // Same leap-month datetime with Allow policy succeeds
    }

    #[test]
    fn determinism_same_input_same_output() {
        let dt = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let a = plum_blossom_signal(dt).unwrap();
        let b = plum_blossom_signal(dt).unwrap();
        assert_eq!(a, b);
    }
}
```

### Acceptance criteria
- [ ] `plum_signal_end_to_end_fixed_instant`: fixed UTC instant yields channels matching manual S1/S2 computation
- [ ] `midpoint_energy_endpoints`: `hexagram=0 → -31.5`, `hexagram=63 → +31.5`
- [ ] `midpoint_energy_center_adjacent`: `hexagram=31 → -0.5`, `hexagram=32 → +0.5`; no integer hexagram equals 31.5
- [ ] `plum_channels_always_populated`: transformed and moving_line are `Some` for Plum Blossom
- [ ] `channel_energy_matches_hexagram_field`: energy == hexagram - 31.5 for all three channels
- [ ] `leap_reject_propagates_as_validation`: `Reject` policy → `TaErrorKind::Validation`
- [ ] `leap_allow_succeeds`: `Allow` policy → `Ok(IchingSignal)`
- [ ] `determinism_same_input_same_output`: two calls with same datetime → `PartialEq` equality
- [ ] All tests pass under `cargo test -p algotrap --lib ta::iching -- --nocapture`
- [ ] At least one test per submodule: `types`, `calendar`, `plum_blossom`, `signal`

### Stop condition
Full integration test suite passes. All L1 §8.2 verification gates satisfied.

### Dependencies
- `p3-u2-plum-blossom-signal-facade` (needs `plum_blossom_signal` and `plum_blossom_signal_with_policy`)

---

## p3-u4-final-gate-and-facade-re-export-wiring

### Goal
Add the **facade-function** re-exports in `src/ta/iching/mod.rs`, `src/ta/mod.rs`, and `src/ta/prelude.rs`, then run the full L1 §8 quality-gate suite across the entire `ta::iching` tree. The submodule declaration for `signal` was added by `p3-u1` and the type re-exports were added by `p1-u7`; this unit only appends function-level re-exports (Build-Progression Rule).

### Writable surface
| File | Action |
|------|--------|
| `src/ta/iching/mod.rs` | `[MODIFY]` — add `pub use signal::{plum_blossom_signal, plum_blossom_signal_with_policy};` (only) |
| `src/ta/mod.rs` | `[MODIFY]` — append `plum_blossom_signal`, `plum_blossom_signal_with_policy` to the existing `pub use iching::{…};` (do not re-declare `pub mod iching;` or duplicate type re-exports) |
| `src/ta/prelude.rs` | `[MODIFY]` — append `plum_blossom_signal`, `plum_blossom_signal_with_policy` to the existing `pub use super::iching::{…};` |

### Rationale for MODIFY
Each file already imports the iching types (p1-u7) and declares the iching submodules (p1-u1 through p3-u1). This unit adds only the two public facade functions to the existing re-export lines after they exist in `signal.rs` (p3-u2). Duplicate `pub mod` or `pub use` declarations between units are forbidden — this unit must not re-issue prior lines.

### Locked contracts
```rust
// src/ta/iching/mod.rs — final addition after prior `pub mod` + type `pub use`:
pub use signal::{plum_blossom_signal, plum_blossom_signal_with_policy};

// src/ta/mod.rs — final `pub use` line (supersedes the p1-u7 shape):
pub use iching::{
    plum_blossom_signal,
    plum_blossom_signal_with_policy,
    HexagramEnergy,
    IchingSignal,
    LeapMonthPolicy,
};

// src/ta/prelude.rs — final `pub use` line (supersedes the p1-u7 shape):
pub use super::iching::{
    plum_blossom_signal,
    plum_blossom_signal_with_policy,
    HexagramEnergy,
    IchingSignal,
    LeapMonthPolicy,
};
```

Implementer note: the p1-u7 declarations must be **replaced in-place** by these expanded forms, not appended as duplicates. If the implementer sees two `pub use iching::{...}` lines after the change, they must consolidate.

### Acceptance criteria
- [ ] `cargo check -p algotrap` succeeds (single `pub use` line per file, no duplicate re-exports)
- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy -p algotrap -- -D warnings` reports no warnings in any `ta::iching` file
- [ ] `cargo test -p algotrap --lib ta::iching -- --nocapture` exits 0 with tests from all 4 submodules
- [ ] `use algotrap::ta::plum_blossom_signal;` resolves from external code
- [ ] `use algotrap::ta::iching::plum_blossom_signal_with_policy;` resolves from external code
- [ ] `use algotrap::ta::{HexagramEnergy, IchingSignal, LeapMonthPolicy};` resolves
- [ ] `use algotrap::ta::prelude::*;` includes all five iching public items (types + facade functions)
- [ ] No `ext`, `engine`, or `query` imports anywhere in `ta::iching` (grep-verified)
- [ ] No `unsafe`, no `unwrap`/`expect` in library code (tests may use them)
- [ ] No `std::fs`, `std::env`, `OnceLock`, or file I/O in any `ta::iching` file

### Stop condition
All L1 §8 quality gates pass. The Plum Blossom implementation is complete and ready for downstream consumption by `engine` or application layers.

### Dependencies
- `p3-u2-plum-blossom-signal-facade` (adds the functions being re-exported)
- `p3-u3-signal-facade-integration-tests` (all tests must already pass)
- `p1-u7-module-re-exports-and-gate` (type re-export lines exist and are expanded, not duplicated)
