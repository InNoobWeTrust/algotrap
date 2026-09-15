# Phase 2 — Plum Blossom Casting (Pure Method)

**Parent L1:** [`l1.md`](../l1.md) §7.2
**Goal:** Implement the Plum Blossom casting algorithm as a pure function over discrete inputs, with synthetic fixture verification, and wire the calendar adapter's Plum Blossom input producer.

---

## p2-u1-trigram-roundtrip-and-hex-flip-tests

### Goal
Add comprehensive round-trip and edge-case tests for `Trigram` and `Hexagram` that serve as regression anchors before any method logic consumes them. These tests validate the Phase 1 types thoroughly from the consumer's perspective.

### Writable surface
| File | Action |
|------|--------|
| `src/ta/iching/types.rs` | `[MODIFY]` — add `#[cfg(test)] mod tests` block with round-trip and flip-line tests |

### Rationale for MODIFY
This unit adds a test module to `types.rs`. It does not change any production code — only test functions that exercise existing public API surface. No other Phase 2 unit adds tests to `types.rs`.

### Locked contracts
No new production types. Tests exercise:
- `Trigram::from_num` → `lines()` → `from_lines()` round-trip for all 8 numbers
- `Trigram::display()` string length and character content for all 8
- `Hexagram::from_binary_index(b)` → `binary_index()` exhaustive round-trip for `b ∈ 0..=63`
- `Hexagram::flip_line` identity: flipping the same line twice returns the original
- `Hexagram::nuclear` on a hand-computed fixture (e.g. Qian-Kun hexagram: all yang → nuclear is all yang via Qian)

### Acceptance criteria
- [ ] `#[test] fn trigram_roundtrip_all_xiantian()` — 8 assertions, one per Xiantian number
- [ ] `#[test] fn hexagram_exhaustive_roundtrip()` — 64 assertions, one per binary index
- [ ] `#[test] fn flip_line_double_flip_identity()` — flip line 3 twice, assert equality with original
- [ ] `#[test] fn nuclear_qian_is_qian()` — nuclear of pure-Qian hexagram is Qian (all yang lines in positions 2–5)
- [ ] `#[test] fn nuclear_kun_is_kun()` — nuclear of pure-Kun hexagram is Kun
- [ ] All tests pass under `cargo test -p algotrap --lib ta::iching::types`

### Stop condition
All round-trip tests pass. Types are validated from a consumer perspective.

### Dependencies
- `p1-u5-hexagram-core-ops` (needs complete `Trigram` and `Hexagram` implementations)

---

## p2-u2-plum-blossom-cast-pure

### Goal
Implement `PlumBlossomInput`, the `PlumBlossomRecord` internal struct, and the pure `cast()` function that computes original hexagram, moving line, transformed hexagram, and nuclear hexagram from discrete inputs — no I/O, no `chrono`, no `lunar-lite`.

### Writable surface
| File | Action |
|------|--------|
| `src/ta/iching/plum_blossom.rs` | `[CREATE]` — all Plum Blossom casting logic and synthetic tests |
| `src/ta/iching/mod.rs` | `[MODIFY]` — add `pub(crate) mod plum_blossom;` (Build-Progression Rule; Plum Blossom casting is internal to `iching`, consumed via `signal`'s facade which is public) |

### Locked contracts
```rust
// src/ta/iching/mod.rs — addition (plum_blossom submodule now exists)
pub(crate) mod plum_blossom;
```
```rust
// src/ta/iching/plum_blossom.rs

use crate::ta::TaResult;
use super::types::{Trigram, Hexagram};

/// Discrete Plum Blossom input — all validated discrete values.
/// Constructed only by the calendar adapter; callers must not fabricate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlumBlossomInput {
    /// Earthly branch of the lunar year, 1..=12. Branch 1 = Zi.
    pub year_branch: u8,
    /// Lunar month, 1..=12.
    pub lunar_month: u8,
    /// Lunar day, 1..=30.
    pub lunar_day: u8,
    /// Earthly branch of the hour, 1..=12. Branch 1 = Zi.
    pub hour_branch: u8,
}

/// Internal Plum Blossom casting record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlumBlossomRecord {
    pub hexagram: u8,          // canonical six-bit value 0..=63
    pub moving_line: u8,       // 1..=6
    pub transformed: u8,       // canonical six-bit value after flip
    pub nuclear: u8,           // canonical six-bit value
}

/// Pure Plum Blossom cast over discrete inputs.
///
/// Formulas (research §3.1):
/// ```text
/// S1 = year_branch + lunar_month + lunar_day
/// S2 = S1 + hour_branch
/// upper_num = if S1 % 8 == 0 { 8 } else { S1 % 8 }  // 1..=8
/// lower_num = if S2 % 8 == 0 { 8 } else { S2 % 8 }  // 1..=8
/// moving_line = if S2 % 6 == 0 { 6 } else { S2 % 6 } // 1..=6
/// ```
pub fn cast(input: PlumBlossomInput) -> TaResult<PlumBlossomRecord>;
```

**Key implementation details:**
- `S1` and `S2` are computed as `u32` (to avoid overflow for edge cases; year_branch + month + day can reach 12+12+30=54, plus hour 12 = 66, well within u32)
- `from_num_mod8` handles the modulo-zero → 8 mapping
- `Hexagram::from_trigrams(upper, lower)` composes the original
- `Hexagram::flip_line(moving_line)` produces transformed
- `Hexagram::nuclear()` produces nuclear
- `binary_index()` on each yields the canonical value

### Acceptance criteria
- [ ] Synthetic fixture: `PlumBlossomInput { year_branch: 1, lunar_month: 1, lunar_day: 1, hour_branch: 1 }` → S1=3, S2=4 → upper=3 (Li), lower=4 (Zhen), moving_line=4
- [ ] Synthetic fixture: `PlumBlossomInput { year_branch: 12, lunar_month: 12, lunar_day: 30, hour_branch: 12 }` → S1=54, S2=66 → upper_num = 54 % 8 = 6 (Kan), lower_num = 66 % 8 = 2 (Dui), moving_line = 66 % 6 = 0 → maps to 6
- [ ] Moving line is always 1..=6 on success
- [ ] `PlumBlossomInput` with `year_branch: 0` fails validation
- [ ] `PlumBlossomInput` with `hour_branch: 13` fails validation
- [ ] `PlumBlossomInput` with `lunar_day: 0` fails validation
- [ ] Transformed hexagram always differs from the original: `flip_line` inverts exactly one bit, so `record.transformed != record.hexagram` for every valid moving_line 1..=6. Assert this for at least one fixture per line position (1 through 6).
- [ ] Nuclear is always computed, regardless of moving-line position
- [ ] Double-check: flipping moving_line on the transformed record's hexagram with the same moving_line produces the original (round-trip)
- [ ] All tests pass under `cargo test -p algotrap --lib ta::iching::plum_blossom`

### Stop condition
Pure casting tests pass with hand-verified fixtures. No I/O or chrono dependency in this file.

### Dependencies
- `p1-u4-trigram-invariants` (needs `Trigram::from_num_mod8`)
- `p1-u5-hexagram-core-ops` (needs `Hexagram::from_trigrams`, `flip_line`, `nuclear`, `binary_index`)

---

## p2-u3-calendar-to-plum-input-adapter

### Goal
Add `to_plum_blossom_input()` to `calendar.rs`, wiring `DateTime<Utc>` → `PlumBlossomInput` through the CST+08 local conversion, lunar resolution, year-branch derivation, and hour-branch derivation.

### Writable surface
| File | Action |
|------|--------|
| `src/ta/iching/calendar.rs` | `[MODIFY]` — add `to_plum_blossom_input()` function and integration tests |

### Rationale for MODIFY
This function is the only addition to `calendar.rs` beyond Phase 1. It depends on the resolved `LunarFields` and branch derivation already in the file. No other Phase 2 unit touches `calendar.rs`.

### Locked contracts
```rust
use super::plum_blossom::PlumBlossomInput;

/// Converts a `DateTime<Utc>` to a validated `PlumBlossomInput`.
///
/// Pipeline:
/// 1. `to_local_datetime(dt)` → CST+08 `NaiveDate` + `NaiveTime`
/// 2. `resolve_lunar(date, policy)` → `LunarFields`
/// 3. `year_branch(lunar.lunar_year)` → year branch 1..=12
/// 4. `hour_branch(time)` → hour branch 1..=12
/// 5. Assemble `PlumBlossomInput` and validate all fields ∈ 1..=12
pub fn to_plum_blossom_input(
    dt: chrono::DateTime<chrono::Utc>,
    policy: super::types::LeapMonthPolicy,
) -> crate::ta::TaResult<PlumBlossomInput>;
```

### Acceptance criteria
- [ ] `to_plum_blossom_input(fixed_utc_datetime, LeapMonthPolicy::Reject)` returns a valid `PlumBlossomInput` with all fields ∈ 1..=12
- [ ] Leap-month rejection propagates through the adapter
- [ ] `to_plum_blossom_input` for a known UTC instant (e.g. `2024-02-10T00:00:00Z`) yields the same `PlumBlossomInput` as manual computation: lunar date = 2024-01-01 (Lunar New Year), year_branch for lunar year 2024, hour_branch for 08:00 local time = Chen (5)
- [ ] `year_branch(lunar_year)` result matches `lunar_lite::lunar_year_branch(lunar_year).index() + 1`
- [ ] No direct `lunar_lite` usage in `plum_blossom.rs` (the adapter is the sole consumer)

### Stop condition
End-to-end adapter test passes. All Phase 2 tests pass under `cargo test -p algotrap --lib ta::iching::plum_blossom -- --nocapture`.

### Dependencies
- `p1-u6-calendar-cst08-wrapper` (needs `to_local_datetime`, `resolve_lunar`, `hour_branch`, `year_branch`)
- `p2-u2-plum-blossom-cast-pure` (needs `PlumBlossomInput`)

---

## p2-u4-plum-blossom-energy-channel-construction

### Goal
Add a higher-level function in `plum_blossom.rs` that takes `PlumBlossomInput`, runs `cast()`, and constructs the three `HexagramEnergy` channels (original, transformed, nuclear) plus the moving line — the bridge between pure casting and the signal facade.

### Writable surface
| File | Action |
|------|--------|
| `src/ta/iching/plum_blossom.rs` | `[MODIFY]` — add `compute_channels()` function and channel-construction tests |

### Rationale for MODIFY
This function extends the Plum Blossom module with energy-channel construction, building directly on the `cast()` function from the same file. It is the final addition to `plum_blossom.rs` in Phase 2.

### Locked contracts
```rust
use super::types::HexagramEnergy;

/// Result of a Plum Blossom computation, materializing energy channels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PlumBlossomResult {
    pub original: HexagramEnergy,
    pub transformed: HexagramEnergy,
    pub nuclear: HexagramEnergy,
    pub moving_line: u8,
}

/// Computes Plum Blossom energy channels from discrete inputs.
///
/// 1. Calls `cast(input)` to get the `PlumBlossomRecord`
/// 2. Constructs `HexagramEnergy::new(record.hexagram)` for each channel
/// 3. Each channel's `energy == hexagram as f64 - 31.5` by factory invariant
/// 4. `moving_line` does not alter any channel's energy
pub fn compute_channels(input: PlumBlossomInput) -> crate::ta::TaResult<PlumBlossomResult>;
```

### Acceptance criteria
- [ ] For any valid `PlumBlossomInput`, `compute_channels` returns `Ok` with all three channels populated
- [ ] `result.original.energy == result.original.hexagram as f64 - 31.5` for the original channel
- [ ] `result.transformed.energy == result.transformed.hexagram as f64 - 31.5` for the transformed channel
- [ ] `result.nuclear.energy == result.nuclear.hexagram as f64 - 31.5` for the nuclear channel
- [ ] `result.moving_line ∈ 1..=6`
- [ ] Three-channel fixture: a fixed input returns populated original, transformed, and nuclear with energies matching manual computation
- [ ] Midpoint fixture: `HexagramEnergy::new(0)` has energy `-31.5`; `HexagramEnergy::new(63)` has energy `+31.5`

### Stop condition
All Plum Blossom unit tests pass. Phase 2 is complete. Phase 3 (facade) may begin.

### Dependencies
- `p2-u2-plum-blossom-cast-pure` (needs `cast()`)
- `p1-u3-hexagram-energy-factory` (needs `HexagramEnergy::new`)
