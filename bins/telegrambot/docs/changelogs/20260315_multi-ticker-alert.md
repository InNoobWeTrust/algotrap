# Changelog: Multi-Ticker Alert System

> **Feature spec**: `docs/specs/alert-scanning.md`, `docs/specs/manual-analysis.md`, `docs/specs/multi-ticker-config.md`
> **Started**: 2026-03-15

## Session: 2026-03-15T20:22

### Phase R: Research
- [ADDED] `docs/prds/research/multi-ticker-alert.md` — Research brief

### Phase 0: Requirements Cascade
- [ADDED] `docs/prds/multi-ticker-alert-bot.md` — PRD
- [ADDED] `docs/trds/multi-ticker-alert.md` — TRD
- [ADDED] `docs/specs/alert-scanning.md` — BDD spec
- [ADDED] `docs/specs/manual-analysis.md` — BDD spec
- [ADDED] `docs/specs/multi-ticker-config.md` — BDD spec

### Challenge Gates
- [ADDED] `docs/changelogs/20260315_multi-ticker-alert.md` — this file

## Session: 2026-03-15T20:30 — Execution

### Source Code
- [MODIFIED] `src/config.rs` — Replaced single-ticker fields with `TickerConf` + `TICKERS` JSON
- [MODIFIED] `src/data.rs` — Parameterized by `&TickerConf`
- [MODIFIED] `src/chart.rs` — Parameterized by `&TickerConf`
- [MODIFIED] `src/llm/mod.rs` — Added `AnalysisMode` enum, `AnalysisResult`, alert JSON parsing
- [MODIFIED] `src/llm/tools.rs` — `capture_chart` filtered in alert mode (ADR-5), parameterized by `&TickerConf`
- [ADDED] `src/commands.rs` — Teloxide slash commands: `/help`, `/analyze`, `/list`
- [MODIFIED] `src/telegram.rs` — Added `send_alert`, `available_tickers_message`
- [MODIFIED] `src/main.rs` — Dual-mode: `tokio::spawn` scan loop + command dispatcher
- [MODIFIED] `src/lib.rs` — Registered `commands` module

### Config & Prompts
- [ADDED] `config/prompts/system_alert.txt` — Alert-mode system prompt
- [ADDED] `config/prompts/user_alert.txt` — Alert-mode user prompt
- [MODIFIED] `.env.example` — TICKERS JSON, SCAN_INTERVAL_SECS, CONFIDENCE_THRESHOLD

### K8s
- [MODIFIED] `k8s/prompts-configmap.yaml` — Added alert prompt files

### ⚔ Challenge Gate: Research Brief — ACCEPTED

Challenges raised: 2 | Author victories: 2 | Reviewer victories: 0

1. **BingX rate limiting at 4× scale** — Author's defense: BingX public API has
   generous limits; 4 tickers × 8 TFs = 32 requests per cycle is well within bounds.
   Proceed at author's decision. Re-evaluate if adding >10 tickers.
2. **LLM cost implications** — Author's defense: alert mode scans are lightweight
   (no chart capture). Cost scales linearly with ticker count, which is operator-controlled.

### ⚔ Challenge Gate: PRD — ACCEPTED (with revision)

Challenges raised: 2 | Author victories: 1 | Reviewer victories: 1

1. **"Reduce noise by ≥80%" is vaguely measurable** — Reviewer won. Revised to:
   "Alert messages sent only when confidence ≥ threshold."
2. **Unauthorized chat access** — Author's defense: single-chat deployment, bot only
   sends to configured TELEGRAM_CHAT_ID. Command access is inherently limited to
   that chat. Accepted.

### ⚔ Challenge Gate: TRD — ACCEPTED (with revisions)

Challenges raised: 3 | Author victories: 0 | Reviewer victories: 3

1. **ADR-3: tokio::select! cancels branches** — Reviewer won. Fixed to
   `tokio::spawn` with `tokio::select!` only for shutdown signaling.
2. **AnalysisResult contains chart_screenshots but alert mode doesn't capture** —
   Reviewer won. Decoupled `chart_screenshots` from `AnalysisResult`; charts are
   captured by the caller post-threshold.
3. **Alert mode tool usage undefined** — Reviewer won. Added ADR-5: alert mode
   uses indicator tools but excludes `capture_chart`.

### ⚔ Challenge Gate: BDD Specs — ACCEPTED (with revision)

Challenges raised: 1 | Author victories: 0 | Reviewer victories: 1

1. **Alert scanning scenario didn't specify which tools are used** — Reviewer won.
   Updated high-confidence scenario to list tools explicitly per ADR-5.

## Session: 2026-03-16T00:04 — Bugfix

### Source Code
- [MODIFIED] `src/commands.rs` — Added `filter_channel_post()` branch via
  `dptree::entry()` so slash commands work in Telegram channels (not just groups)

### Docs
- [MODIFIED] `docs/trds/multi-ticker-alert.md` — Noted channel post handling in
  `commands.rs` component description
- [MODIFIED] `docs/specs/manual-analysis.md` — Added channel post scenario and
  validation rule

### Root Cause
Telegram channels emit `ChannelPost` updates, not `Message` updates. The original
code used `Update::filter_message()` only, which silently ignored channel posts.
Fix: dual-branch handler via `dptree::entry()` listening to both event types.
