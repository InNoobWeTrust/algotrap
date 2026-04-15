# PRD: Adaptive Alert System v2

> **Status**: draft
> **Owner**: Product Owner
> **Created**: 2026-03-17

## Problem Statement

The multi-ticker alert system (deployed 2026-03-15) produced zero alerts in
2 days of operation. Every 15-minute scan returned LLM confidence 20-30%,
well below the 70% threshold. Root cause: the confidence score is an undefined
LLM guess with no data-driven methodology, no feedback loop, and no output
for below-threshold situations. The operator receives no strategic value
from the system between rare high-confidence entries.

## Goals & Non-Goals

### Goals

- Replace subjective LLM confidence with a **weighted composite score** where
  indicator weights are dynamically tuned by the LLM based on market context
- Produce **three tiers of output** (Alert / Watch / Silent) so the operator
  receives strategic market outlook even when no entry is detected
- Enable **self-learning** via persistent memory — the bot reflects on past
  prediction accuracy and tunes scoring weights over time
- Build a **knowledge base** (10-topic markdown) where the LLM records market
  observations that both the LLM and operator can reference
- Improve **Telegram bot UX** — `/start`, `/status`, `/digest`, `/weights`,
  unknown message handling

### Non-Goals

- Backtesting framework — adaptive weights are forward-looking, not backtested
- Multi-user support — single-operator deployment
- Custom indicator development — uses existing algotrap indicator set
- Automated trade execution — advisory only

## User Personas

- **Operator (sole user)**: Crypto trader managing 4 tickers (BTC, ETH, SOL,
  XAUT). Checks Telegram periodically. Needs strategic market awareness between
  entries, not just binary alerts. Trusts RSSI, climax_signal, and EMA200 as
  primary indicators but knows they have context-dependent reliability.

## User Stories (High-Level)

- As the **operator**, I want the bot to compute confidence from measurable
  indicator data with dynamically tuned weights, so that scores reflect actual
  market conditions rather than LLM guesswork.
- As the **operator**, I want to receive a strategic market outlook (price,
  momentum trajectory, trade plan options) when confidence is moderate (40-69),
  so that I have situational awareness even without a clear entry.
- As the **operator**, I want the bot to learn from its past predictions and
  adjust scoring weights over time, so that accuracy improves with experience.
- As the **operator**, I want to read the bot's knowledge base to understand
  what patterns it has observed, so that I can learn from its analysis and verify
  its reasoning quality.
- As the **operator**, I want proper bot UX (`/start`, unknown message handling,
  `/status`, `/digest`, `/weights`), so that interaction is smooth and intuitive.

## Success Metrics

- Bot produces non-silent output (Alert or Watch tier) for at least 1 ticker
  per scan cycle when any significant market activity exists
- Watch-tier messages are sent when indicator delta exceeds LLM-tuned
  significance threshold (seeded at 25%) since last notification
- LLM scoring weights change measurably over 7 days of operation (evidence of
  self-tuning, not static)
- Knowledge base files accumulate meaningful observations within the first
  week of operation (non-empty, contextually relevant)
- Zero user confusion on first interaction (`/start` produces welcome message)

## Scope

1. **Adaptive weighted scoring**: LLM tunes indicator weights dynamically per
   scan cycle. Weights stored in persistent JSON. Bounded ranges (0.05-0.5)
   with rate limits (±0.05/cycle) prevent instability.

2. **Three-tier response system**:
   - 🎯 **Alert** (≥ 70): full analysis + charts + entry plan
   - 👁️ **Watch** (40-69): current price + market summary + trade plan options
     (A/B/C scenarios). Charts only if confidence ≥ 50.
   - 🔇 **Silent** (< 40): logged only, stored in memory

3. **Significant-change detection**: compare indicator snapshot vs last-notified
   snapshot. If delta exceeds LLM-tuned threshold (seeded 25%), send Watch-tier
   update even within the same tier.

4. **Persistent memory**: JSON file per ticker with prediction history (sliding
   window: today + last day, max 8), weights, indicator snapshots.

5. **Outcome validation**: LLM evaluates past trade plan options (A/B/C) against
   what actually happened. Score = correct matches / total options.

6. **Knowledge base**: 10 markdown files (market-regimes, indicator-quirks,
   ticker-personalities, timeframe-dynamics, weight-rationale,
   prediction-retrospective, risk-patterns, timing-patterns,
   cross-ticker-signals, strategy-evolution). LLM reads before analysis,
   appends/revises after.

7. **Bot UX**: `/start`, `/status`, `/digest`, `/weights` commands + unknown
   message handler + channel post support.

## Out of Scope

- Historical backtesting of weight configurations
- Per-user configuration or multi-chat support
- Custom indicator creation or modification
- Direct integration with exchange APIs for trade execution
- Scheduled digest messages (only on-demand via `/digest`)

## Dependencies

- Existing multi-ticker infrastructure (TICKERS JSON, scan loop, command dispatcher)
- K8s PersistentVolume for memory + knowledge base files
- LLM API (LiteLLM proxy) — increased prompt complexity for memory + KB context

## Child TRDs

- `docs/trds/adaptive-alert-v2.md` — Architecture for adaptive scoring, tiered
  response, persistent memory, knowledge base, and UX commands

## ⚔ Challenge Gate

> **Status**: passed
> **Challenger**: Antigravity (self-review)
> **Date**: 2026-03-17

### Debate Record

| # | Vector | Challenge | Response | Verdict |
|---|--------|-----------|----------|---------|
| 1 | Evidence | "At least 1 ticker per scan cycle produces non-silent output" — this metric could be gamed by making the Watch tier threshold trivially low. Is it measuring real value or just output volume? | Valid — the metric measures output frequency, not quality. But it's paired with the "indicator delta exceeds significance threshold" condition and "knowledge base accumulates meaningful observations" metric. Together these ensure output is triggered by real data changes, not lowered bars. The significance threshold is also LLM-tuned with a reasonable seed (25%), not artificially low. | author-won |
| 2 | Longevity | LLM context window will grow significantly as memory (8 predictions × detailed JSON) + KB (10 markdown files) are loaded. Could this exceed token limits or degrade quality? | Context budget needs management. Predictions are compact (8 × ~200 tokens ≈ 1.6K). KB files should be summarized/pruned to stay under ~500 tokens each (5K total). Combined ≈ 7K tokens for memory+KB — well within GPT-4o's 128K context. Quality degradation from context size is mitigatable by loading only relevant KB topics per scan (not all 10). | author-won (with note: implement selective KB loading) |
| 3 | Edge cases | What happens when the memory PV is lost? Cold start with no history, no weights, no KB — does the system degrade gracefully? | Yes — all memory fields have defaults. Weights default to equal (0.25 each). Predictions array starts empty. KB files are seeded with empty templates. The system operates identically to current behavior on cold start, then builds knowledge over time. No hard dependency on prior state. | author-won |

### Challenge Summary

- **Challenges raised**: 3
- **Author victories**: 3 (1 with implementation note on selective KB loading)
- **Challenger victories**: 0
- **Escalated**: 0
- **Overall verdict**: ACCEPTED

### Revisions Made (if any)

- Added note on selective KB loading (only relevant topics per scan, not all 10) to manage context window budget.

## Notes

- Research brief: `docs/prds/research/adaptive-scoring.md`
- Brainstorming session: `docs/prds/research/adaptive-scoring-brainstorm.md`
