# Phase 1 — Calendar Adapter + Core Types (Foundation)

**Parent L1:** [`l1.md`](../l1.md) §7.1
**Goal:** Establish the time-policy wrapper, lunar-lite integration, canonical type invariants, and the validated DTO factory layer so all downstream phases have a stable, tested vocabulary.

---

## Build-Progression Rule (applies to every unit)

The source tree **must compile after each unit**. Therefore a unit that creates a submodule file is the unit that adds its `pub mod`/`pub(crate) mod` declaration to `src/ta/iching/mod.rs`, and adds the `pub use` re-export only once the referenced items exist. No unit may forward-declare a submodule or re-export an item it has not yet introduced. This rule replaces the earlier "scaffold declaring all children" shape, which would not pass `cargo check`.

---

## p1-u1-crate-and-module-scaffold

### Goal
Add the `lunar-lite` dependency, register the `iching` module in `ta`, and create a **doc-only** `src/ta/iching/mod.rs` root — no submodule declarations, no re-exports, no stub files. Submodules and re-exports are wired incrementally by the units that create them (Build-Progression Rule).

### Writable surface
| File | Action |
|------|--------|
| `Cargo.toml` | `[MODIFY]` — add `lunar-lite = "1.3.0"` under `[dependencies]` |
| `src/ta/mod.rs` | `[MODIFY]` — add `pub mod iching;` after existing modules |
| `src/ta/iching/mod.rs` | `[CREATE]` — crate-style `//!` module doc only; no `pub mod`/`pub use` yet |

### Locked contracts
```rust
// src/ta/mod.rs — addition
pub mod iching;
```
```rust
// src/ta/iching/mod.rs — doc comment only (a module with no items compiles)
//! I-Ching datetime energy-chart feature (Plum Blossom).
//! Submodules (`types`, `calendar`, `plum_blossom`, `signal`) and public
//! re-exports are wired by the units that first introduce them.
```

### Acceptance criteria
- [ ] `cargo check -p algotrap` succeeds with the new dependency resolved
- [ ] `src/ta/iching/mod.rs` contains only a module doc comment (no child modules declared)
- [ ] No `ext`, `engine`, or `query` imports in any `iching` file (grep-verified)

### Stop condition
`cargo check -p algotrap` exits 0 with an empty `iching` module. Subsequent units add their own submodule wiring.

### Dependencies
None (first unit in the plan).

---

## p1-u2-dto-field-definitions

### Goal
Define the `HexagramEnergy`, `IchingSignal`, `LeapMonthPolicy`, and `TIME_POLICY_ID` types in `types.rs` with their field structures, derives, and rustdoc — but **no factory functions yet** (those are a separate unit).

### Writable surface
| File | Action |
|------|--------|
| `src/ta/iching/types.rs` | `[CREATE]` — field definitions only |
| `src/ta/iching/mod.rs` | `[MODIFY]` — add `pub mod types;` (Build-Progression Rule: this unit wires the submodule it creates) |

### Locked contracts
```rust
// src/ta/iching/mod.rs — addition (types submodule now exists)
pub mod types;
```
```rust
// src/ta/iching/types.rs

/// Time-policy identifier baked into provenance (§2.4).
pub const TIME_POLICY_ID: &str = "cst-utc8-fixed-v1";

/// Leap-month handling policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LeapMonthPolicy {
    /// Reject leap months; return `TaError::validation` if the date falls in one.
    #[default]
    Reject,
    /// Permit leap months; `lunar_month` is used as-is.
    Allow,
}


/// One plottable I-Ching hexagram-energy channel.
///
/// **Must be created through validated factory functions** (`HexagramEnergy::new`),
/// never via raw struct-literal construction by callers. The invariant
/// `energy == hexagram as f64 - 31.5` is mechanically enforced at construction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HexagramEnergy {
    /// Canonical six-bit hexagram value B = Σ line[i]·2ⁱ. Range 0..=63.
    pub hexagram: u8,
    /// Hexagram value centered around the six-bit range midpoint, in [-31.5, +31.5].
    /// Equal to `hexagram as f64 - 31.5`.
    pub energy: f64,
}

/// Complete method-scoped I-Ching reading at one timestamp.
///
/// DTOs are method-neutral; `transformed` and `moving_line` are always `Some`
/// for Plum Blossom but may be `None` for future methods.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IchingSignal {
    /// Current/root state (本卦). Always present.
    pub original: HexagramEnergy,
    /// Post-trigger state (变卦). `Some` for Plum Blossom; `None` for methods without a moving line.
    pub transformed: Option<HexagramEnergy>,
    /// Internal/mutual characteristic (互卦), structurally derived from `original`.
    /// Always present because every six-line hexagram has a nuclear derivation.
    pub nuclear: HexagramEnergy,
    /// Moving line position 1..=6 (bottom-to-top) for Plum Blossom; `None` otherwise.
    pub moving_line: Option<u8>,
}
```

### Acceptance criteria
- [ ] `types.rs` compiles with all types, derives, and the constant
- [ ] `LeapMonthPolicy` derives `Default`, returning `Reject`
- [ ] Each struct has a rustdoc referencing its I-Ching role
- [ ] The "must use factory" invariant is documented on `HexagramEnergy` struct-level doc

### Stop condition
Types are defined and compile; no factory logic exists yet. The unit is purely structural.

### Dependencies
- `p1-u1-crate-and-module-scaffold` (needs `types.rs` to exist in the module tree)

---

## p1-u3-hexagram-energy-factory

### Goal
Implement the validated `HexagramEnergy::new` factory function that mechanically enforces the `energy == hexagram as f64 - 31.5` invariant and rejects out-of-range inputs.

### Writable surface
| File | Action |
|------|--------|
| `src/ta/iching/types.rs` | `[MODIFY]` — add `impl HexagramEnergy` block with `new()` and tests |

### Rationale for MODIFY
This unit adds method implementations to the struct defined in `p1-u2`. The file already exists from the prior unit; this unit adds behavior to it. No other unit in Phase 1 modifies `HexagramEnergy` in `types.rs`.

### Locked contracts
```rust
impl HexagramEnergy {
    /// Creates a validated hexagram-energy channel.
    ///
    /// The `energy` field is always computed as `hexagram as f64 - 31.5`.
    /// Callers provide only the canonical hexagram index; energy is derived.
    /// Returns `TaError::validation` if `hexagram > 63`.
    pub fn new(hexagram: u8) -> crate::ta::TaResult<Self> {
        if hexagram > 63 {
            return Err(crate::ta::TaError::validation(
                "hexagram must be 0..=63",
            ));
        }
        Ok(Self {
            hexagram,
            energy: hexagram as f64 - 31.5,
        })
    }
}
```

### Acceptance criteria
- [ ] `HexagramEnergy::new(0)` returns `Ok(HexagramEnergy { hexagram: 0, energy: -31.5 })`
- [ ] `HexagramEnergy::new(63)` returns `Ok(HexagramEnergy { hexagram: 63, energy: 31.5 })`
- [ ] `HexagramEnergy::new(31)` has `energy == -0.5` (below zero)
- [ ] `HexagramEnergy::new(32)` has `energy == 0.5` (above zero)
- [ ] `HexagramEnergy::new(64)` returns `Err` with `kind == TaErrorKind::Validation`
- [ ] No raw struct-literal construction of `HexagramEnergy` exists outside `HexagramEnergy::new` in non-test code

### Stop condition
Factory function passes all midpoint fixture assertions; clippy clean.

### Dependencies
- `p1-u2-dto-field-definitions` (needs the struct and `TaError` in scope)

---

## p1-u4-trigram-invariants

### Goal
Implement `Trigram` with the Xiantian table, line accessors, display, and validated constructors — the foundational bit-pattern vocabulary for all hexagram operations.

### Writable surface
| File | Action |
|------|--------|
| `src/ta/iching/types.rs` | `[MODIFY]` — add `Trigram` struct, Xiantian table constants, and methods |

### Rationale for MODIFY
`Trigram` lives alongside `HexagramEnergy`/`IchingSignal` in `types.rs` per the L1 architecture (§3.2). This is the final modification to `types.rs` in Phase 1.

### Locked contracts
```rust
/// A three-line I-Ching trigram using the Xiantian (先天) numbering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trigram {
    lines: [u8; 3], // lines[0]=bottom, 1=Yang 0=Yin
}

impl Trigram {
    /// Xiantian trigram number (1..=8). Returns `TaError::validation` for `n ∉ 1..=8`.
    pub fn from_num(n: u8) -> crate::ta::TaResult<Self>;

    /// Xiantian trigram from a sum modulo 8. Maps `s % 8 == 0` to 8 (Kun).
    pub fn from_num_mod8(s: u32) -> crate::ta::TaResult<Self>;

    /// Bottom-to-top line array `[line[0], line[1], line[2]]`.
    pub fn lines(self) -> [u8; 3];

    /// Top-to-bottom display string, e.g. `"010"` for Kan.
    pub fn display(self) -> String;

    /// Inverse: reconstruct a Trigram from bottom-to-top lines.
    /// Returns `TaError::validation` if any element is not 0 or 1.
    pub fn from_lines(lines: [u8; 3]) -> crate::ta::TaResult<Self>;
}
```

Xiantian mapping (single source of truth, §4.1):

| num | lines (bottom→top) | display (top→bottom) |
|-----|-------------------|----------------------|
| 1 (Qian) | `[1,1,1]` | `"111"` |
| 2 (Dui)  | `[1,1,0]` | `"011"` |
| 3 (Li)   | `[1,0,1]` | `"101"` |
| 4 (Zhen) | `[1,0,0]` | `"001"` |
| 5 (Xun)  | `[0,1,1]` | `"110"` |
| 6 (Kan)  | `[0,1,0]` | `"010"` |
| 7 (Gen)  | `[0,0,1]` | `"100"` |
| 8 (Kun)  | `[0,0,0]` | `"000"` |

### Acceptance criteria
- [ ] `Trigram::from_num(1..=8)` round-trips through `lines()` and `display()` for all 8 values
- [ ] `Trigram::from_num(0)` returns `Validation` error
- [ ] `Trigram::from_num(9)` returns `Validation` error
- [ ] `Trigram::from_num_mod8(0)` maps to Kun (num 8, lines `[0,0,0]`)
- [ ] `Trigram::from_num_mod8(8)` maps to Kun (num 8)
- [ ] `Trigram::from_num_mod8(1)` maps to Qian (num 1, lines `[1,1,1]`)
- [ ] `Trigram::from_lines([1,0,1])` round-trips with `lines() == [1,0,1]`
- [ ] `Trigram::from_lines([2,0,1])` returns `Validation` (non-binary value)
- [ ] Display strings are exactly 3 chars of `'0'`/`'1'`, top-to-bottom

### Stop condition
All trigram tests pass; round-trip and edge-case assertions verified.

### Dependencies
- `p1-u2-dto-field-definitions` (needs `types.rs` and `TaError` in scope)

---

## p1-u5-hexagram-core-ops

### Goal
Implement `Hexagram` with trigram composition, binary index, display, flip-line, and nuclear derivation — the core hexagram operations that Plum Blossom casting and the facade consume.

### Writable surface
| File | Action |
|------|--------|
| `src/ta/iching/types.rs` | `[MODIFY]` — add `Hexagram` struct and all methods |

### Rationale for MODIFY
`Hexagram` is the second half of the `types.rs` vocabulary, added after `Trigram`. This is the last `types.rs` modification in Phase 1.

### Locked contracts
```rust
/// A six-line I-Ching hexagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hexagram {
    lines: [u8; 6], // lines[0]=bottom, 1=Yang 0=Yin
}

impl Hexagram {
    /// Compose from upper and lower trigrams.
    pub fn from_trigrams(upper: Trigram, lower: Trigram) -> Self;

    /// Canonical six-bit value B = Σ line[i]·2ⁱ. Range 0..=63.
    pub fn binary_index(self) -> u8;

    /// Top-to-bottom 6-char display string (line[5]..line[0]).
    pub fn bits_top_to_bottom(self) -> String;

    /// Reconstruct from a canonical index 0..=63.
    /// Returns `TaError::validation` if `b > 63`.
    pub fn from_binary_index(b: u8) -> crate::ta::TaResult<Self>;

    /// Flip exactly one line (1=bottom..6=top).
    /// Returns `TaError::validation` if `line ∉ 1..=6`.
    pub fn flip_line(self, line: u8) -> crate::ta::TaResult<Self>;

    /// Nuclear hexagram: lower trigram from lines 2,3,4; upper from lines 3,4,5
    /// (1-indexed bottom; 0-indexed: lower=[1,2,3], upper=[2,3,4]).
    pub fn nuclear(self) -> Self;
}
```

### Acceptance criteria
- [ ] `Hexagram::from_trigrams` composes upper/lower correctly into the 6-line array
- [ ] `Hexagram::from_binary_index(b)` for all `b ∈ 0..=63` yields `binary_index() == b`
- [ ] `bits_top_to_bottom().len() == 6` for all valid hexagrams
- [ ] Display convention matches research §2.1: `Qian(1)` top→bottom = `"111111"`, index 63
- [ ] `flip_line(1)` flips bottom line; `flip_line(6)` flips top line
- [ ] `flip_line(0)` returns `Validation`; `flip_line(7)` returns `Validation`
- [ ] `nuclear()` matches research §3.1: original lines 2,3,4 → lower trigram; lines 3,4,5 → upper trigram
- [ ] `Hexagram` is `Copy + Clone + Eq` (verified by compile test or `assert!(<Hexagram as Copy>::is_copy())`)
- [ ] `from_binary_index(64)` returns `Validation`

### Stop condition
All hexagram unit tests pass, including exhaustive `0..=63` round-trip, nuclear derivation, and flip-line edge cases.

### Dependencies
- `p1-u4-trigram-invariants` (needs `Trigram` to compose hexagrams)

---

## p1-u6-calendar-cst08-wrapper

### Goal
Implement the CST+08 time conversion and `lunar-lite` wrapper in `calendar.rs`, converting `DateTime<Utc>` → local `NaiveDate` + `NaiveTime` → `LunarFields`, with Zi-hour branch derivation and leap-month policy enforcement.

### Writable surface
| File | Action |
|------|--------|
| `src/ta/iching/calendar.rs` | `[CREATE]` — all calendar adapter logic |
| `src/ta/iching/mod.rs` | `[MODIFY]` — add `pub(crate) mod calendar;` (Build-Progression Rule; calendar is internal, not publicly re-exported) |

### Locked contracts
```rust
// src/ta/iching/mod.rs — addition (calendar submodule now exists; internal only)
pub(crate) mod calendar;
```
```rust
use chrono::{DateTime, NaiveDate, NaiveTime, TimeZone, Utc, FixedOffset};
use crate::ta::{TaError, TaResult};
use super::types::LeapMonthPolicy;

/// Fixed UTC+08:00 offset for the `cst-utc8-fixed-v1` time policy.
const CST_OFFSET: FixedOffset = FixedOffset::east_opt(8 * 3600)
    .expect("UTC+08:00 is always valid");

/// Lunar calendar fields extracted from `lunar-lite`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LunarFields {
    /// Lunar month 1..=12 (never the leap instance number; leap is tracked separately).
    pub lunar_month: u8,
    /// Lunar day 1..=30.
    pub lunar_day: u8,
    /// Whether this date falls in a leap month.
    pub is_leap_month: bool,
    /// Lunar year (for year-branch derivation).
    pub lunar_year: i32,
}

/// Resolves a `DateTime<Utc>` to local CST+08 `NaiveDate` + `NaiveTime`.
///
/// Time policy: `cst-utc8-fixed-v1`. No date rollback at 23:00.
pub fn to_local_datetime(dt: DateTime<Utc>) -> (NaiveDate, NaiveTime);

/// Derives the Earthly Branch number (1..=12) for a local time.
///
/// Zi-hour rule (`cst-utc8-fixed-v1`):
/// - `time ∈ [23:00, 01:00)` → hour_branch = 1 (Zi), using the **same local date**.
/// - `01:00..03:00` → 2 (Chou), etc.
/// The midnight boundary is `00:00`; 23:00 does NOT roll the date backward.
pub fn hour_branch(time: NaiveTime) -> u8;

/// Derives the year branch (1..=12) from a Gregorian year.
///
/// Formula: `((year - 4) % 12) + 1`. Branch 1 = Zi ... 12 = Hai.
/// Returns `TaError::validation` for years outside a reasonable range.
pub fn year_branch(year: i32) -> TaResult<u8>;

/// Converts `NaiveDate` to lunar fields via `lunar-lite`.
///
/// Uses `lunar_lite::solar_to_lunar(SolarDate { year, month, day })`.
/// Maps `LunarError` → `TaError::validation`.
/// Applies the `LeapMonthPolicy`: `Reject` returns error when `is_leap_month == true`.
pub fn resolve_lunar(date: NaiveDate, policy: LeapMonthPolicy) -> TaResult<LunarFields>;
```

**lunar-lite API note (verified against docs.rs v1.3.0):**
- `lunar_lite::solar_to_lunar(SolarDate { year: i32, month: u8, day: u8 }) -> Result<LunarDate, LunarError>`
- `LunarDate { year: i32, month: u8, day: u8, is_leap_month: bool }`
- `LunarError` is `#[non_exhaustive]` — all `match` arms need a wildcard
- `lunar_lite::lunar_year_branch(lunar_year: i32) -> EarthlyBranch` with `.index() -> usize`

### Acceptance criteria
- [ ] `to_local_datetime(2024-02-10T00:00:00Z)` yields CST+08 date `2024-02-10` (08:00 local)
- [ ] `hour_branch(NaiveTime::from_hms_opt(23, 30, 0).unwrap()) == 1` (Zi)
- [ ] `hour_branch(NaiveTime::from_hms_opt(0, 0, 0).unwrap()) == 1` (Zi, midnight)
- [ ] `hour_branch(NaiveTime::from_hms_opt(1, 0, 0).unwrap()) == 2` (Chou)
- [ ] `hour_branch(NaiveTime::from_hms_opt(22, 59, 59).unwrap()) == 12` (Hai)
- [ ] `year_branch(2024)` returns a valid 1..=12 value
- [ ] `resolve_lunar` with a known leap-month date + `LeapMonthPolicy::Reject` returns `Validation`
- [ ] `resolve_lunar` with the same date + `LeapMonthPolicy::Allow` succeeds
- [ ] `resolve_lunar` for `2024-02-10` returns fields consistent with lunar-lite reference (Lunar New Year 2024: month=1, day=1)
- [ ] No file I/O, no `OnceLock`, no `std::fs` usage (clippy + grep-verified)
- [ ] `lunar-lite` is only imported in `calendar.rs` (no other iching file touches it)

### Stop condition
All calendar unit tests pass under `cargo test -p algotrap --lib ta::iching::calendar -- --nocapture`. Clippy clean.

### Dependencies
- `p1-u2-dto-field-definitions` (needs `LeapMonthPolicy`)
- `p1-u1-crate-and-module-scaffold` (needs `Cargo.toml` with `lunar-lite`)

---

## p1-u7-module-re-exports-and-gate

### Goal
Wire the public type re-exports at the end of Phase 1 by (a) adding `pub use types::{HexagramEnergy, IchingSignal, LeapMonthPolicy};` to `iching/mod.rs`, and (b) re-exporting from `src/ta/mod.rs` and `src/ta/prelude.rs`, then verify the entire Phase 1 compiles and passes all gates.

Note: signal-function re-exports (`plum_blossom_signal`) come later in Phase 3 unit `p3-u4`.

### Writable surface
| File | Action |
|------|--------|
| `src/ta/iching/mod.rs` | `[MODIFY]` — add `pub use types::{HexagramEnergy, IchingSignal, LeapMonthPolicy};` |
| `src/ta/mod.rs` | `[MODIFY]` — add `pub use iching::{HexagramEnergy, IchingSignal, LeapMonthPolicy};` |
| `src/ta/prelude.rs` | `[MODIFY]` — add `pub use super::iching::{HexagramEnergy, IchingSignal, LeapMonthPolicy};` |

### Rationale for MODIFY
Re-exports are the final wiring step that makes the types available at the crate's `ta` layer. All three files may already have been minimally modified in prior units; this unit adds only re-export lines.

### Locked contracts
```rust
// src/ta/iching/mod.rs — addition after existing `mod` declarations:
pub use types::{HexagramEnergy, IchingSignal, LeapMonthPolicy};

// src/ta/mod.rs — addition after existing pub use:
pub use iching::{HexagramEnergy, IchingSignal, LeapMonthPolicy};

// src/ta/prelude.rs — addition after existing pub use block:
pub use super::iching::{HexagramEnergy, IchingSignal, LeapMonthPolicy};
```

### Acceptance criteria
- [ ] `cargo check -p algotrap` succeeds
- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy -p algotrap -- -D warnings` reports no warnings in new files
- [ ] `cargo test -p algotrap --lib ta::iching -- --nocapture` exits 0 and runs tests from `types`, `calendar` submodules
- [ ] `crate::ta::HexagramEnergy`, `crate::ta::IchingSignal`, `crate::ta::LeapMonthPolicy` are importable from external crate code
- [ ] No `ext`, `engine`, or `query` imports anywhere in `ta::iching` (grep-verified)

### Stop condition
Full Phase 1 gate passes. All 7 units are complete. Phase 2 may begin.

### Dependencies
- All prior Phase 1 units (`p1-u1` through `p1-u6`)
