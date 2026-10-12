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

**Coordinate:** `energy = Q − 31.5`, range `[−31.5, +31.5]`. `hexagram_bin` is the chosen six-bit Q encoding, `Q = 32l1 + 16l2 + 8l3 + 4l4 + 2l5 + l6`, with physical line 1 bottom, line 6 top, Yang=1 and Yin=0; it is not King Wen numbering, strength, or a validated market-direction measure.

**Structural dependence:** Original is the root state. Mutual is structurally derived from Original's middle four lines (lines 2–5), with `mutual.lines[0] = original.lines[1]`; Transformed toggles physical array position `moving_line − 1` within the same cast before Q encoding, not at a fresh future calendar instant. The bottom-first physical array, lower/upper trigram composition and top-to-bottom display are unchanged; the three channels are dependent by construction, not independent market confirmations.

**Sign relationships (per-cast coordinate properties, not market indicators):** Each cast's `energy` is a non-zero half-integer: Q≥32 (physical bottom line 1 Yang) gives positive energy (≥+0.5), otherwise negative (≤−0.5); an aggregate mean may still be zero. Mutual is positive iff Original's physical line 2 is Yang, because `mutual.lines[0] = original.lines[1]`. Transformed differs in sign from Original only when physical moving line = 1; all other moving lines preserve its sign.

**Numeric compatibility (local Q adoption):** User-approved Q replaces legacy bottom-LSB B; numeric values intentionally change while API shapes, names and nullability remain unchanged, with no dual codec. Bo=1, Fu=32, Jie=50, Tai=56 and Pi=7; Kun=0, Qian=63 and symmetric codes can remain equal. Physical casts, flips, mutual extraction and calendar behavior are unchanged.

**Compatibility note (Oct 2026):** `moving_line_open: Option<u8>` is a new public field on `IchingBarTrajectory`. Existing code using exhaustive struct-literal patterns must add this field; `..` rest patterns are unaffected. The existing `moving_line` field (terminal cast) and all function signatures are unchanged. No blanket source-compatibility guarantee is made for this public struct.

## Navigation

- [`../../docs/architecture.md`](../../docs/architecture.md) — dependency direction and ownership rules
- [`../../docs/architecture/stream-and-duckdb-data-flow.md`](../../docs/architecture/stream-and-duckdb-data-flow.md) — analysis-to-frame boundary
- [`../model/README.md`](../model/README.md) — canonical analysis inputs
- [`../engine/README.md`](../engine/README.md) — downstream result-frame contract
- [`../README.md`](../README.md) — library-level navigation
