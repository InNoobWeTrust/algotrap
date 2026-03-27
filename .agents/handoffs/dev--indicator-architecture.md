---
branch: dev
topic: indicator architecture hardening
status: in-progress
updated: 2026-03-28T03:29:00+07:00
agent: antigravity
---

# Indicator Architecture Hardening

## Status

**Scoring & feedback refactor: DONE** (committed, not pushed). **Indicator architecture: SPECS DONE, implementation not started.**

Two local commits ahead of `origin/dev`:
```
e72ad79 (HEAD -> dev) docs(telegrambot): add indicator architecture v2 and ATR gap zones specs
23235e9 refactor(telegrambot): direction-based scoring, LLM feedback, KB rules
8d07fa3 (origin/dev) ops(k8s): pin litellm to SHA256 digest and add persistent SQLite
```

### Completed (code + tests, committed)
- Direction-based composite scoring replaces plan-counting (`src/scoring.rs`)
- LLM feedback: direction markers, accuracy stats, conviction checking (`src/llm/mod.rs`)
- KB conditional write rules based on accuracy thresholds (`src/llm/mod.rs`)
- Weight floor lowered 0.05 → 0.01 (`src/config.rs`)
- System prompt updated with conviction rules + `{{kb_rules}}` placeholder
- One-time migration to reset legacy 0.333/0.667 scores
- 62 tests pass (14 new scoring scenarios)

### Completed (specs only, no code yet)
- `docs/specs/indicator-architecture-v2.md` — 15 scenarios
- `docs/specs/atr-gap-zones.md` — 13 scenarios

## Key Decisions

1. **Schema reset = indicator key set changes ONLY.** Adding/removing an indicator triggers prediction+weight reset. Param tuning (period, smooth, active toggle) does NOT trigger reset. This was debated and the user explicitly corrected a wrong initial design that tied param changes to resets.

2. **OHLC is always-on base tier.** Raw candle data is never part of the toggle system. Minimum-2-active guardrail applies to derived indicators only (rssi, atr, etc.).

3. **Stateful indicators recompute from full OHLC each cycle.** The bot already fetches complete candle history, so ATR Gap Zones are recalculated fresh each cycle with current params. No separate state invalidation needed on param changes.

4. **`min_trust` exempt from ±30% rate limiting.** It's a quality filter threshold, not a computational parameter. LLM should adjust freely within its range `[0.0, 0.9]`.

5. **`climax_signal` is redundant.** It's a derivative of RSSI + ATR reversion — adds zero new information. Remove from `data.rs`, `llm/tools.rs`, `main.rs`, system prompt, and weight tests.

6. **ATR Gap Zones design.** Gap = candle body (`min(open,close)` to `max(open,close)`) of candles closing outside `open ± ATR` (strict inequality). Trust = body/wick ratio. Overlap density weighted by trust = S/R strength. Tunable params: `atr_period` (default 42), `max_zones` (default 50), `min_trust` (default 0.3).

7. **Dormant roster for toggle.** Disabled indicators appear as a one-liner in the prompt with cycle-since-disabled count. LLM sets `"active": true` to re-enable. If accuracy < 40% for 5+ cycles, prompt suggests re-evaluating dormant indicators.

## Blockers

- **Rust-analyzer**: Proc-macro ABI mismatch between `rustc 1.93.0` and `1.94.1`. IDE-only, does not affect `cargo build`/`cargo test`.

## Next Steps

Implement in this order (each builds on the previous):

1. **Remove `climax_signal`** — Simplest change, cleans the pipeline.
   - `src/data.rs`: Remove `climax_signal`, `overbought`, `oversold` columns
   - `src/llm/tools.rs`: Remove from indicator summary
   - `src/main.rs`: Remove from change detection keys
   - `config/prompts/system_adaptive.txt`: Remove climax references
   - Tests: Update weight/snapshot tests

2. **Structural compatibility check** — Replace one-time migration hack.
   - `src/main.rs` startup: Compare stored indicator keys (from most recent prediction's snapshot) with current key set
   - Mismatch → clear `predictions` and `weights`, retain KB, log reason
   - No predictions → skip comparison (nothing to reset)

3. **`IndicatorConfig` struct** — Core of tunable params.
   - Add to `TickerMemory` (in `src/llm/mod.rs` or new file)
   - Define `ParamSpec { value, min, max, active }` per indicator
   - Parse `indicator_params` from LLM JSON response (absent = no-op)
   - Apply range clamping + ±30% rate limiting
   - Feed active params into `indicators()` pipeline in `src/data.rs`

4. **Active/inactive toggle + dormant roster**
   - Format dormant roster in prompt
   - Parse `active` toggle from LLM response
   - Min-2-active guardrail enforcement
   - Regime change suggestion (uses existing `compute_direction_accuracy`)

5. **ATR Gap Zones indicator**
   - Implement in `algotrap/src/ta/experimental.rs`
   - `GapZone { top, bottom, trust, age_bars }` struct
   - `detect_gap_zones(ohlc, atr_period, max_zones, min_trust)` function
   - Integrate into telegrambot `data.rs` indicator pipeline
   - LLM context formatting for gap zone summary

## Open Questions

None — all design decisions are settled and spec'd.

## Recent Changes

| File | Change |
|------|--------|
| `src/scoring.rs` | Complete rewrite: direction-based composite scoring |
| `src/llm/mod.rs` | conviction_aligned, direction markers, accuracy stats, KB rules |
| `src/main.rs` | Outcome validation, one-time migration, conviction logging |
| `src/config.rs` | weight_min 0.05 → 0.01 |
| `config/prompts/system_adaptive.txt` | Conviction rules, confidence calibration, `{{kb_rules}}` |
| `docs/specs/indicator-architecture-v2.md` | NEW: 15 scenarios, challenge-gated |
| `docs/specs/atr-gap-zones.md` | NEW: 13 scenarios, challenge-gated |
| `docs/specs/outcome-scoring-v2.md` | NEW: scoring spec |
| `docs/specs/llm-feedback-quality.md` | NEW: feedback spec |
| `k8s/pvc.yaml` | NEW: PVC manifest |
