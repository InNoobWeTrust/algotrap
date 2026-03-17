# Feature: Persistent Memory & Knowledge Base

> **Status**: draft
> **Owner**: Product Owner
> **Created**: 2026-03-18

## Parent TRD

`docs/trds/adaptive-alert-v2.md` — ADR-2 (persistent state), ADR-5 (selective KB loading), ADR-6 (KB tools)

## Description

Enable the bot to learn from experience via persistent JSON memory (predictions,
weights, outcomes) and a 10-topic markdown knowledge base. The LLM reads memory
and KB before each analysis, updates them after, and validates past predictions
against actual price outcomes.

## User Stories

- As the **operator**, I want the bot to remember its past predictions and
  outcomes, so that it can self-correct its scoring weights over time.
- As the **operator**, I want to read the bot's knowledge base to understand
  market patterns it has observed, so that I can learn from its analysis.
- As the **operator**, I want the bot to validate its past predictions against
  what actually happened, so that outcome accuracy drives weight tuning.

## Scenarios

### Scenario: Memory read on scan start

- **Given** a scan cycle begins for BTC-USDT
- **And** the memory file `/data/memory/BTC-USDT.json` exists with 5 predictions
- **When** the system loads memory
- **Then** all 5 predictions are loaded (within max MAX_PREDICTIONS window)
- **And** `current_weights` is loaded as the baseline for this cycle
- **And** `last_notified_snapshot` is loaded for change detection

### Scenario: Memory write after scan

- **Given** a scan cycle completes for BTC-USDT with confidence 55, direction LONG
- **When** the prediction is stored
- **Then** a new entry is appended to `predictions[]` with timestamp, confidence,
  direction, weights, trade_plans, indicator_snapshot, significance_threshold
- **And** `current_weights` is updated with the (clamped, rate-limited) weights
- **And** the write is atomic (write to `.tmp` then rename)

### Scenario: Sliding window — max predictions exceeded

- **Given** the memory file already contains `MAX_PREDICTIONS` entries (configurable, default 8)
- **When** a new prediction is stored
- **Then** the oldest prediction is evicted (FIFO)
- **And** the file retains exactly `MAX_PREDICTIONS` entries

### Scenario: Sliding window — today + last day with escalation

- **Given** the memory file contains predictions from 3 days ago (none from
  today or yesterday)
- **When** the sliding window is applied
- **Then** the 3-day-old predictions are retained (escalate to nearest available)
- **And** max `MAX_PREDICTIONS` entries are kept regardless of age

### Scenario: Outcome validation

- **Given** the memory contains a prediction from 2 hours ago with trade plans:
  - Plan A: LONG entry at 82,100, SL at 81,200
  - Plan B: SHORT entry at 83,500, SL at 84,200
  - Plan C: Wait (no entry)
- **And** the current BTC-USDT price is 82,500 (above Plan A entry, in LONG
  direction)
- **When** the outcome validator runs
- **Then** Plan A is marked as matching (price moved through entry in correct
  direction)
- **And** Plans B and C are marked as not matching
- **And** `outcome_score` is set to 1/3 = 0.33
- **And** the outcome is included in the LLM's context for the next cycle

### Scenario: Outcome validation — no match

- **Given** the memory contains a prediction with trade plans A (LONG at 82,100),
  B (SHORT at 83,500), C (Wait)
- **And** the current price is 82,050 (below all entry levels, moved sideways)
- **When** the outcome validator runs
- **Then** no plans match
- **And** `outcome_score` is set to 0.0

### Scenario: Outcome validation runs at scan start

- **Given** a scan cycle begins for BTC-USDT
- **And** the memory contains 5 predictions, 3 without `outcome_score`
- **When** outcome validation runs (before LLM analysis)
- **Then** all 3 non-scored predictions are evaluated against current price
- **And** their `outcome_score` fields are populated
- **And** the updated outcomes are included in the LLM prompt context

### Scenario: Cold start — no memory file

- **Given** the memory file does not exist
- **When** the system loads memory
- **Then** default empty memory is used: empty predictions, equal weights, no
  last_notified_snapshot
- **And** the memory directory is created if missing
- **And** a log message indicates cold start

### Scenario: KB read via tool — valid topic

- **Given** the LLM calls `read_kb` with topic "market-regimes"
- **And** the file `/data/memory/kb/market-regimes.md` exists with content
- **When** the tool executes
- **Then** the markdown content is returned to the LLM

### Scenario: KB read via tool — empty topic

- **Given** the LLM calls `read_kb` with topic "timing-patterns"
- **And** the file does not exist or is empty
- **When** the tool executes
- **Then** an empty template is returned: `# Timing Patterns\n\nNo observations recorded yet.`

### Scenario: KB read via tool — invalid topic

- **Given** the LLM calls `read_kb` with topic "random-invalid-topic"
- **When** the tool executes
- **Then** an error is returned listing the 10 valid topic slugs

### Scenario: KB write via tool — append mode

- **Given** the LLM calls `write_kb` with topic "indicator-quirks",
  content "### 2026-03-18\nRSSI unreliable below ATR 0.5%", mode "append"
- **When** the tool executes
- **Then** the content is appended to `/data/memory/kb/indicator-quirks.md`
  with a blank line separator
- **And** the tool returns "Updated indicator-quirks.md (append)"

### Scenario: KB write via tool — replace mode

- **Given** the LLM calls `write_kb` with topic "weight-rationale",
  content "# Weight Rationale\n\n...", mode "replace"
- **When** the tool executes
- **Then** the file is fully replaced with the new content
- **And** the tool returns "Updated weight-rationale.md (replace)"

### Scenario: KB write — content too long

- **Given** the LLM calls `write_kb` with content exceeding 2000 characters
- **When** the tool executes
- **Then** the content is truncated to 2000 characters
- **And** a warning is included in the tool response

### Scenario: Selective KB loading in prompt

- **Given** a scan cycle begins for ETH-USDT
- **When** the system constructs the LLM prompt
- **Then** the following KB topics are loaded automatically:
  `weight-rationale.md` and `prediction-retrospective.md`
- **And** `ticker-personalities.md` is loaded (always relevant per-ticker)
- **And** other KB topics are available on-demand via `read_kb` tool

### Scenario: Memory file corruption recovery

- **Given** the memory file contains invalid JSON
- **When** the system attempts to load memory
- **Then** the corrupt file is renamed to `{symbol}.json.corrupt`
- **And** default empty memory is used instead
- **And** an error is logged with the parse failure details

## Validation Rules

- Memory JSON is written atomically (temp file + rename)
- Sliding window retains max `MAX_PREDICTIONS` entries (configurable, default 8);
  today + last day preferred, escalates to nearest available if empty
- Outcome validation runs at scan start, before LLM analysis, on all
  non-scored predictions still in the sliding window
- Outcome validation is LLM-assisted but the formula (matching plans / total
  plans) is deterministic once matches are identified
- KB topic names are validated against fixed whitelist of 10 slugs
- KB write content is capped at 2000 characters per call
- KB files are seeded with empty templates on first access
- Memory directory `/data/memory/` and KB directory `/data/memory/kb/` are
  created on startup if missing

## Out of Scope

- Database storage (files only)
- Memory encryption at application level
- Cross-ticker memory sharing (each ticker has independent memory)
- KB topic creation/deletion by the LLM (fixed 10 topics)

## Dependencies

- `docs/specs/adaptive-scoring.md` — weights come from the scoring engine
- `docs/specs/tiered-response.md` — last_notified_snapshot used for change detection

## ⚔ Challenge Gate

> **Status**: passed
> **Challenger**: Antigravity (self-review)
> **Date**: 2026-03-18

### Debate Record

| # | Vector | Challenge | Response | Verdict |
|---|--------|-----------|----------|---------|
| 1 | Evidence | Outcome validation checks if "price moved through entry in correct direction." But that's a simplistic measure — a trade plan could have the right direction but entry was never reached, or was reached then reversed past the SL. Is this accurate enough? | The validation is intentionally simple: did price cross the entry level in the correct direction at any point since the prediction? This is a "was the idea directionally correct" measure, not a PnL calculation. For weight-tuning feedback loops, directional accuracy is sufficient. Full PnL tracking would require tracking entries, exits, and SL hits — out of scope for v2. | author-won |
| 2 | Longevity | 10 fixed KB topics — what if the LLM discovers a pattern that doesn't fit any of the 10 categories? | `strategy-evolution.md` is the catch-all for meta-observations. The 10 categories cover the domain comprehensively (from indicator-level to cross-ticker to time patterns). If a genuinely new category emerges, it can be added in a future revision — the system is designed for 10 but the codebase can trivially extend the whitelist. | author-won |

### Challenge Summary

- **Challenges raised**: 2
- **Author victories**: 2
- **Challenger victories**: 0
- **Escalated**: 0
- **Overall verdict**: ACCEPTED
