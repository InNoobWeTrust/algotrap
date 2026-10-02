# `ta` — synchronous technical analysis

## Purpose

This module provides the typed, synchronous analysis domain. It turns model values into
stateful indicator results through explicit kernels, operators, and pure gap-zone analysis,
without binding that computation to an async runtime, database, or application workflow.

## Responsibility contract

**Owns**

- The typed kernel transition vocabulary: prior state, successor state, and row output.
- In-memory processing state and the operator families that implement technical analysis.
- Analysis errors and numerical safety at operator boundaries.

**Does not own**

- Stream adaptation, scheduling, or runtime concerns.
- Application aggregate definitions, result-frame projection, or presentation policy.
- SQL, DuckDB access, persistence, or external market-data I/O.

## Stable boundaries

`ta` depends on `model` and is consumed by the adapter and engine layers; it must not depend on
either of those downstream concerns. A transition is explicit: a successor state is adopted only
after the transition succeeds, so a failed aggregate step cannot commit partial state.

Numeric inputs and derived outputs must remain finite, and invalid periods or missing required
values are reported through the analysis error contract. Non-finite results are errors, never
silent substitutions. Applications own the aggregate composition and later decide how typed rows
become result frames.

## I-Ching cast trajectories

`iching_bar_trajectory(bar_open_time_ms, bar_close_time_ms)` returns an `IchingBarTrajectory` for one market bar. The bar owns the right-half-open interval `[open, close)`; the exact close belongs to the next bar.

| Channel | Opening cast | Intra-bar range | Terminal cast |
|---|---|---|---|
| Original (本卦) | `energy_open`, `moving_line_open` | `energy_high`, `energy_low` | `energy_close`, `moving_line` |
| Transformed (变卦) | `transformed_open` | — | `transformed_close` |
| Mutual (互卦) | `mutual_open` | `mutual_high`, `mutual_low`, `mutual_mean` | `mutual_close` |

**Coordinate:** `energy = hexagram_bin − 31.5`, range `[−31.5, +31.5]`. `hexagram_bin` is the six-bit binary index 0..=63 where bit 0 is the bottom line and bit 5 is the top line; it is not a King Wen number and carries no empirically validated financial interpretation.

**Structural dependence:** Original is the root state. Mutual is structurally derived from Original's middle four lines (lines 2–5); its top-bit position (line 6, index 5) takes the value of Original's line 5 (index 4), so Mutual's coordinate sign depends on Original's line 5. Transformed is Original with its moving-line bit flipped; its sign differs from Original's only when moving line = 6 (the sole flip that touches bit 5). The three channels are dependent by construction, not independent market confirmations.

**Sign relationships (binary coordinate properties, not market indicators):** `energy` is always a non-zero half-integer; `hexagram_bin ≥ 32` (bit 5 = 1, line 6 Yang) gives positive energy (≥+0.5), `hexagram_bin < 32` gives negative (≤−0.5), and zero is impossible. Mutual is positive iff Original's line 5 (bit 4) = 1, because `mutual.lines[5] = original.lines[4]`. Transformed differs in sign from Original only when moving line = 6; for all other moving lines Original and Transformed share the same sign.

**Compatibility note (Oct 2026):** `moving_line_open: Option<u8>` is a new public field on `IchingBarTrajectory`. Existing code using exhaustive struct-literal patterns must add this field; `..` rest patterns are unaffected. The existing `moving_line` field (terminal cast) and all function signatures are unchanged. No blanket source-compatibility guarantee is made for this public struct.

## Navigation

- [`../../docs/architecture.md`](../../docs/architecture.md) — dependency direction and ownership rules
- [`../../docs/architecture/stream-and-duckdb-data-flow.md`](../../docs/architecture/stream-and-duckdb-data-flow.md) — analysis-to-frame boundary
- [`../model/README.md`](../model/README.md) — canonical analysis inputs
- [`../engine/README.md`](../engine/README.md) — downstream result-frame contract
- [`../README.md`](../README.md) — library-level navigation
