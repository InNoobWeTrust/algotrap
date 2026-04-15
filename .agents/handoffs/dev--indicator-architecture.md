---
branch: dev
topic: indicator architecture hardening + prompt optimization
status: implementation-complete
updated: 2026-03-28T15:50:00+07:00
agent: antigravity
---

# Indicator Architecture Hardening + Prompt Optimization

## Status

**All specs implemented.** 4 spec work items fully delivered across prompt optimization, scratchpad/handoff, ATR gap zones lazy migration, and indicator architecture cleanup.

### Already deployed (origin/dev)

7 commits implementing indicator architecture v2:
```
026fd16 (HEAD -> dev, origin/dev) docs: update ATR gap zones traceability
e099b25 docs: update handoff — all indicators wired, ready for review
7b852db feat(telegrambot): wire gap zones into LLM tool pipeline
584cb89 feat: add ATR Gap Zones indicator with full test coverage
2d3741f feat(telegrambot): add indicator config context to system prompt
6949a15 feat(telegrambot): add IndicatorConfig with LLM-tunable params + active/inactive toggle
e22f9ad refactor(telegrambot): remove climax_signal, add schema compatibility check
```

87 tests pass (17 lib + 70 telegrambot).

### Implementation deliverables

| Work Item | Status | Detail |
|-----------|--------|--------|
| LiteLLM config: `reasoning_effort` + `max_completion_tokens` | ✅ Done | Configured for gpt-5-mini |
| `system_adaptive.txt` compressed (115→42 lines) | ✅ Done | Merged instruction sections, JSON template |
| `supports_reasoning` flag + conditional CoT trigger | ✅ Done | CoT only for non-reasoning models via `EnvConf` |
| Scratchpad tools (`write_notes`/`read_notes`) + handoff protocol | ✅ Done | In-memory HashMap, 3-phase forced handoff |
| ATR gap zones lazy Polars migration | ✅ Done | `is_atr_gap` + `body_ratio` as lazy Exprs |
| `is_atr_gap` + `body_ratio` in `indicators()` pipeline | ✅ Done | Compute-all principle respected |
| `compute_gap_zone_context` simplified with fallback | ✅ Done | Post-collect extraction with composite trust |
| Scenario 13 regime change suggestion | ✅ Done | Accuracy < 40% for 5+ cycles → prompt nudge |
| Climax cleanup | ✅ Done | Residual `climax` refs removed from test fixtures |

## Specs Implemented

### 1. indicator-architecture-v2 — ✅ All scenarios implemented

| Item | Status |
|------|--------|
| Scenario 13 (regime change suggestion) | ✅ Implemented — accuracy < 40% for 5+ cycles triggers prompt nudge to re-enable dormant indicators |
| Residual `climax` refs in test fixtures | ✅ Cleaned up — removed from `scoring.rs`, `memory.rs` weight tests |

### 2. atr-gap-zones-trd — ✅ Lazy Polars migration complete

Migrated from eager `detect_gap_zones` with raw `&[f64]` slices to TRD-specified architecture:
- **Layer 1**: `is_atr_gap(ohlc, period) -> Expr` + `body_ratio(ohlc) -> Expr` (lazy) ✅
- **Layer 2**: `extract_gap_zones(&DataFrame, &GapZoneParams)` (post-collect, reads `rssi` for composite trust) ✅
- **Compute-all principle**: `is_atr_gap`/`body_ratio` always computed regardless of `is_active` ✅

### 3. prompt-optimization — ✅ Reasoning effort + template compression done

- **LiteLLM config**: `reasoning_effort: high` + `max_completion_tokens: 4096` for gpt-5-mini ✅
- **Prompt compression**: `system_adaptive.txt` reduced from 115→42 lines ✅
- **CoT**: Only for non-reasoning models, controlled by `supports_reasoning` in `EnvConf` ✅

### 4. scratchpad-handoff — ✅ Context engineering implemented

- **Tools**: `write_notes(key, content)` / `read_notes(key?)` — in-memory HashMap ✅
- **Forced handoff**: 3-phase protocol when scratchpad is empty at context limit ✅
- **Threshold**: `keep_recent_messages * 2` (default 20 messages, ~10 tool calls) ✅
- **Fallback**: `compress_history` retained as last-resort (2 handoff attempts first) ✅
- **Signature change**: `execute_tool_call` accepts `&mut HashMap<String, String>` ✅

## Key Decisions (carried forward)

1. Schema reset = indicator key set changes ONLY. Param tuning does NOT trigger reset.
2. OHLC is always-on base tier. Min-2-active guardrail applies to derived indicators only.
3. Compute-all, show-selectively. `active` toggle controls LLM visibility, not computation.
4. `min_trust` exempt from ±30% rate limiting.
5. `compress_history` not deleted — becomes last-resort fallback behind scratchpad.
6. CoT prompt and `reasoning_effort` are mutually exclusive — never combine.
7. Scratchpad is session-scoped (dies at scan end). KB is cross-session (persisted).

## Implementation Priority

1. ✅ **[quick win]** LiteLLM config: `reasoning_effort: high` + `max_completion_tokens`
2. ✅ **[quick win]** Compress `system_adaptive.txt` template
3. ✅ **[medium]** Scratchpad + forced handoff (tools.rs, mod.rs, tools.json)
4. ✅ **[small]** ATR gap zones lazy Polars migration (lib refactor)
5. ✅ **[small]** Scenario 13: regime change suggestion in prompt
6. ✅ **[cleanup]** Remove residual `climax` from test fixtures

## Implementation Summary

### Files modified

| Area | Files | Changes |
|------|-------|---------|
| Prompt optimization | `litellm_config.yaml`, `system_adaptive.txt`, `EnvConf` | reasoning_effort, max_completion_tokens, template compression (115→42 lines), `supports_reasoning` flag |
| Scratchpad & handoff | `tools.rs`, `mod.rs`, `tools.json` | `write_notes`/`read_notes` tool implementations, `execute_tool_call` signature update, 3-phase handoff protocol |
| ATR gap zones | `lib/src/indicators/atr_gap_zones.rs`, `indicators()` pipeline | Lazy `is_atr_gap` + `body_ratio` Exprs, `extract_gap_zones` refactor, `compute_gap_zone_context` with fallback |
| Indicator architecture | Prompt templates, scoring/regime logic | Scenario 13 regime change suggestion, climax cleanup in test fixtures |

### Key implementation details

- **Prompt compression**: Merged redundant instruction sections, converted verbose text to compact JSON template format
- **Scratchpad**: Session-scoped `HashMap<String, String>` passed via `&mut` through tool execution chain
- **Handoff protocol**: 3 phases — (1) prompt to write notes, (2) force handoff with scratchpad dump, (3) fall back to `compress_history`
- **Lazy Polars**: `is_atr_gap` and `body_ratio` are now `Expr`-returning functions wired into the main `indicators()` pipeline, always computed regardless of active toggle
- **Regime change**: Monitors rolling accuracy; when < 40% for 5+ consecutive cycles, injects suggestion to re-evaluate indicator configuration

## Blockers

None.
