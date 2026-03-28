---
branch: dev
topic: indicator architecture hardening + prompt optimization
status: spec-review-complete
updated: 2026-03-28T15:09:00+07:00
agent: antigravity
---

# Indicator Architecture Hardening + Prompt Optimization

## Status

**Specs reviewed and fixed.** Ready for big implementation push.

### Already deployed (origin/dev)

6 commits implementing indicator architecture v2:
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

### Uncommitted (staged for next deployment)

| File | Type | Status |
|------|------|--------|
| `specs/indicator-architecture-v2.md` | spec fix | Updated traceability (14/15 impl), fixed default config table |
| `specs/atr-gap-zones.md` | spec fix | Fixed `OhlcExperimental` reference, user-formatted tables |
| `specs/atr-gap-zones-trd.md` | new TRD | Lazy Polars design + composite trust formula |
| `specs/prompt-optimization.md` | new spec | Reasoning effort + template compression (7 scenarios) |
| `specs/scratchpad-handoff.md` | new spec | In-session scratchpad + forced handoff (11 scenarios) |
| `prds/research/prompt-engineering-v2.md` | research | Brainstorm doc, source for prompt specs |

## Specs Pending Implementation

### 1. indicator-architecture-v2 — 1 scenario remaining

| Gap | Detail |
|-----|--------|
| Scenario 13 | Regime change suggestion (accuracy < 40% for 5+ cycles → prompt nudge to re-enable dormant indicators) |
| Residual refs | `climax` in test fixtures (`scoring.rs`, `memory.rs` weight tests) — cosmetic |

### 2. atr-gap-zones-trd — lazy Polars migration

Current implementation uses eager `detect_gap_zones` with raw `&[f64]` slices. TRD specifies:
- **Layer 1**: `is_atr_gap(ohlc, period) -> Expr` + `body_ratio(ohlc) -> Expr` (lazy)
- **Layer 2**: `extract_gap_zones(&DataFrame, &GapZoneParams)` (post-collect, reads `rssi` for composite trust)
- **Key fix applied**: `is_atr_gap`/`body_ratio` always computed (compute-all principle), not conditional on `is_active`

### 3. prompt-optimization — reasoning effort + template compression

Two independent quick wins:
- **LiteLLM config**: `reasoning_effort: high` + `max_completion_tokens: 4096` for gpt-5-mini
- **Prompt compression**: 38-line → 10-line JSON template, merge instruction sections
- **CoT**: Only for non-reasoning models, controlled by `supports_reasoning` in `EnvConf`
- **Risk**: Verify litellm passes `reasoning_effort` on tool-calling requests

### 4. scratchpad-handoff — context engineering

Replace lossy `compress_history` with agent-controlled working memory:
- **Tools**: `write_notes(key, content)` / `read_notes(key?)` — in-memory HashMap
- **Forced handoff**: 3-phase protocol when scratchpad is empty at context limit
- **Threshold**: `keep_recent_messages * 2` (default 20 messages, ~10 tool calls)
- **Fallback**: `compress_history` retained as last-resort (2 handoff attempts first)
- **Signature change**: `execute_tool_call` must accept `&mut HashMap<String, String>`

## Key Decisions (carried forward)

1. Schema reset = indicator key set changes ONLY. Param tuning does NOT trigger reset.
2. OHLC is always-on base tier. Min-2-active guardrail applies to derived indicators only.
3. Compute-all, show-selectively. `active` toggle controls LLM visibility, not computation.
4. `min_trust` exempt from ±30% rate limiting.
5. `compress_history` not deleted — becomes last-resort fallback behind scratchpad.
6. CoT prompt and `reasoning_effort` are mutually exclusive — never combine.
7. Scratchpad is session-scoped (dies at scan end). KB is cross-session (persisted).

## Implementation Priority

1. **[quick win]** LiteLLM config: `reasoning_effort: high` + `max_completion_tokens`
2. **[quick win]** Compress `system_adaptive.txt` template
3. **[medium]** Scratchpad + forced handoff (tools.rs, mod.rs, tools.json)
4. **[small]** ATR gap zones lazy Polars migration (lib refactor)
5. **[small]** Scenario 13: regime change suggestion in prompt
6. **[cleanup]** Remove residual `climax` from test fixtures

## Blockers

None.
