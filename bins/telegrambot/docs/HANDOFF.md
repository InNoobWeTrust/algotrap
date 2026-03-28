# Indicator Architecture v2 — Handoff

> Last updated: 2026-03-28 12:30 UTC+7
> Branch: `dev`
> Status: ✅ IMPLEMENTATION COMPLETE — awaiting user review before deploy

## Commits (oldest → newest, all on `dev`)

1. `e22f9ad` — Remove `climax_signal` + add schema compatibility check
2. `6949a15` — Add `IndicatorConfig` with LLM-tunable params + toggle system  
3. `2d3741f` — System prompt: indicator tuning section + `indicator_params` JSON
4. `584cb89` — ATR Gap Zones indicator in `algotrap::ta::gap_zones`

## Test Status

- `cargo test -p algotrap -p telegrambot` → **87 passed, 0 failed**
- algotrap: 17 tests (9 new gap_zones)
- telegrambot: 70 tests (4 new IndicatorConfig, 4 new schema check)

## What's Done

| Feature | Files | Status |
|---------|-------|--------|
| Climax removal | `data.rs`, `tools.rs`, `main.rs`, `config.rs`, `telegram.rs`, `scoring.rs`, `system_adaptive.txt` | ✅ |
| Schema compatibility | `memory.rs`, `main.rs` | ✅ |
| IndicatorConfig + tunable pipeline | `memory.rs`, `data.rs`, `main.rs`, `commands.rs`, `test_analysis.rs` | ✅ |
| Active/inactive toggle + dormant | `memory.rs`, `main.rs` | ✅ |
| System prompt updates | `system_adaptive.txt`, `llm/mod.rs` | ✅ |
| ATR Gap Zones (lib) | `src/ta/gap_zones.rs`, `src/ta/mod.rs` | ✅ |
| Gap zones in IndicatorConfig | `memory.rs` (gap_zones entry + min_trust field) | ✅ |

## What's NOT Done (by design — awaiting review)

1. **Gap zones scan-time integration**: The `detect_gap_zones` function exists in the lib and `gap_zone_params()` exists in config, but the `scan_ticker` pipeline in `main.rs` doesn't call gap zone detection or pass the summary to the LLM context yet. This is intentional — the user wants to review all changes before wiring the live pipeline.

2. **Gap zones LLM context in tools.rs**: No formatting of gap zone data in `get_multi_tf_overview` or `get_indicator_summary` yet. Needs the scan integration above first.

## Architecture Notes for Next Agent

- `IndicatorConfig` is persisted in `TickerMemory` with `#[serde(default)]` for backward compat
- `min_trust` is rate-limit-exempt in `apply_proposed` (quality filter, not computational param)
- Schema check runs at startup; param tuning does NOT trigger schema resets (only key set changes do)
- Gap zones is a stateful indicator — recomputed from full OHLC series each cycle, not a Polars expression
- All existing indicator periods in `data.rs` are now configurable but default to their previous hardcoded values
