---
branch: dev
topic: indicator architecture hardening
status: review-ready
updated: 2026-03-28T13:15:00+07:00
agent: antigravity
---

# Indicator Architecture Hardening

## Status

**ALL IMPLEMENTATION COMPLETE** — awaiting user review before deploy.

6 commits ahead of `origin/dev`:
```
7b852db (HEAD -> dev) feat(telegrambot): wire gap zones into LLM tool pipeline
584cb89 feat: add ATR Gap Zones indicator with full test coverage
2d3741f feat(telegrambot): add indicator config context to system prompt
6949a15 feat(telegrambot): add IndicatorConfig with LLM-tunable params + active/inactive toggle
e22f9ad refactor(telegrambot): remove climax_signal, add schema compatibility check
55bbcd8 (origin/dev) docs(telegrambot): add indicator architecture v2 and ATR gap zones specs
```

**87 tests pass** (17 algotrap lib + 70 telegrambot).

## What's Implemented

| Feature | Key Files |
|---------|-----------|
| Climax signal removal | `data.rs`, `tools.rs`, `main.rs`, `system_adaptive.txt` |
| Schema compatibility check | `memory.rs` (`check_schema_compatibility`) |
| IndicatorConfig + tunable pipeline | `memory.rs`, `data.rs` (all periods configurable) |
| Active/inactive toggle + dormant roster | `memory.rs` (`apply_proposed`, `tick_dormant`) |
| LLM indicator_params parsing | `llm/mod.rs` (`parse_alert_json`) |
| System prompt — indicator tuning section | `system_adaptive.txt`, `llm/mod.rs` |
| ATR Gap Zones (algotrap lib) | `src/ta/gap_zones.rs` (9 tests) |
| Gap zones in LLM tool pipeline | `llm/tools.rs` (`compute_gap_zone_context`) |

## Key Decisions

1. **Schema reset = indicator key set changes ONLY.** Param tuning does NOT trigger reset.
2. **OHLC is always-on base tier.** Min-2-active guardrail applies to derived indicators only.
3. **Stateful indicators recompute from full OHLC each cycle.** No separate state invalidation.
4. **`min_trust` exempt from ±30% rate limiting.** Quality filter, not computational param.
5. **`climax_signal` removed.** Redundant derivative of RSSI + ATR reversion.

## Blockers

None.
