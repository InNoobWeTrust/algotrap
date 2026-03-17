# Changelog: Adaptive Alert System v2

> **Feature spec**: `docs/specs/adaptive-scoring.md`, `docs/specs/tiered-response.md`, `docs/specs/persistent-memory.md`, `docs/specs/bot-ux-v2.md`, `docs/specs/llm-prompt-engineering.md`
> **Started**: 2026-03-17

## Session: 2026-03-17T15:00 — Research & Requirements

### Phase R: Research
- [ADDED] `docs/prds/research/adaptive-scoring-brainstorm.md` — Brainstorm session
- [ADDED] `docs/prds/research/adaptive-scoring.md` — Research brief

### Phase 0: Requirements Cascade
- [ADDED] `docs/prds/adaptive-alert-v2.md` — PRD (3 challenge gates passed)
- [ADDED] `docs/trds/adaptive-alert-v2.md` — TRD (architecture, ADRs, components)
- [ADDED] `docs/specs/adaptive-scoring.md` — BDD spec: weighted scoring + guardrails
- [ADDED] `docs/specs/tiered-response.md` — BDD spec: Alert/Watch/Silent tiers
- [ADDED] `docs/specs/persistent-memory.md` — BDD spec: per-ticker JSON memory + KB
- [ADDED] `docs/specs/bot-ux-v2.md` — BDD spec: /start, /status, /digest, /weights
- [ADDED] `docs/specs/llm-prompt-engineering.md` — BDD spec: template vars, context injection, compression

## Session: 2026-03-17T17:00 — Execution (Phases 1-3)

### Source Code — New Modules
- [ADDED] `src/config.rs` — 14 new env vars (MEMORY_DIR, WEIGHT_*, TIER_*, MAX_PREDICTIONS, KEEP_RECENT_MESSAGES)
- [ADDED] `src/memory.rs` — Per-ticker JSON memory: read/write/atomic save, sliding window, TickerMemory types
- [ADDED] `src/kb.rs` — Knowledge base: 10-topic markdown I/O, seed on first run, read_kb/write_kb handlers
- [ADDED] `src/scoring.rs` — Tier engine, significant-change detection, weight guardrails, outcome scoring

### Source Code — Modified Modules
- [MODIFIED] `src/llm/mod.rs` — AlertScan→adaptive prompts, expanded response parsing (weights, threshold, trade_plans), render_prompt with 13 template vars, compress_history (LLM-based), format_memory/weights/outcome_context
- [MODIFIED] `src/llm/tools.rs` — Added read_kb, write_kb tool definitions + execution handlers
- [MODIFIED] `src/commands.rs` — Added /start, /status, /digest, /weights commands + unknown message handler
- [MODIFIED] `src/main.rs` — Tiered scan loop (outcome validation → LLM → scoring → tier → notify), memory load/save, weight guardrail application
- [MODIFIED] `src/telegram.rs` — Watch-tier formatter, digest formatter, weights formatter, media group sender
- [MODIFIED] `src/lib.rs` — Registered memory, kb, scoring modules
- [MODIFIED] `src/bin/test_analysis.rs` — Load memory for AlertScan test mode

### Config & Prompts
- [ADDED] `config/prompts/system_adaptive.txt` — Adaptive mode system prompt (13 template vars)
- [ADDED] `config/prompts/user_adaptive.txt` — Adaptive mode user prompt
- [MODIFIED] `config/prompts/tools.json` — Added read_kb, write_kb tool schemas

### Docs
- [MODIFIED] `docs/specs/architecture.md` — Updated agent loop, added AlertScan mode docs

## Session: 2026-03-18T01:00 — Adversarial Review & Fixes

### Cross-Consistency Fixes (8 issues found, 7 fixed)
- [MODIFIED] `docs/specs/adaptive-scoring.md` — significance_threshold range 0.0-1.0, removed normalization claims, canonical indicator names
- [MODIFIED] `docs/specs/llm-prompt-engineering.md` — Status draft→approved, stale function ref, tfs format
- [MODIFIED] `src/llm/mod.rs` — Added warn! logs for missing weights/threshold in LLM response
- [MODIFIED] `config/prompts/system_adaptive.txt` — Replaced hardcoded tier thresholds with template vars

### Verification
- `cargo check -p telegrambot` ✅
- `cargo test -p telegrambot` — 50/50 tests pass ✅
- Adversarial review: 8 challenges, all resolved
- Goal alignment check: all 7 PRD scope items implemented, no drift
