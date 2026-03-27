# Feature: LLM Feedback Quality & Conviction

> **Status**: draft
> **Owner**: Product Owner
> **Created**: 2026-03-27

## Parent TRD

`docs/trds/adaptive-alert-v2.md` — ADR-1 (hybrid scoring), ADR-5 (selective KB loading)

## Description

Improve the quality of feedback the LLM receives about its own performance, and
enforce directional conviction in trade plans. Currently the LLM sees
"avg accuracy: 0.33" (broken score) and always hedges plans as [LONG, SHORT,
WAIT]. This spec covers: accurate direction feedback in the prompt, per-prediction
correctness markers, conviction alignment checks, confidence calibration,
and conditional KB writing rules.

## User Stories

- As the **operator**, I want the LLM to see its real direction accuracy, so
  that it can self-correct weights based on honest performance data.
- As the **operator**, I want trade plans to be directionally coherent with
  the declared direction, so that I can act on clear signals.
- As the **operator**, I want the KB auto-populated when the LLM detects patterns,
  so that accumulated knowledge improves future predictions.

## Scenarios

### Scenario: Outcome summary shows direction accuracy

- **Given** memory contains 8 predictions with outcome scores:
  [1.0, 0.4, 1.0, 0.0, 1.0, 1.0, 0.0, pending]
- **When** `format_outcome_summary` builds the prompt context
- **Then** the output includes: "Direction: 4/7 correct (57%)"
- **And** the output includes: "Composite avg: 0.63"
- **And** the output does NOT contain the old plan-match rate

### Scenario: Memory context shows per-prediction correctness

- **Given** memory contains a scored prediction:
  timestamp=2026-03-27T10:10, conf=55, dir=SHORT, outcome=0.4
- **When** `format_memory_context` builds the prompt context
- **Then** the line reads: `[03-27 10:10] conf=55 dir=SHORT outcome=0.40 dir=✗`
- **And** a prediction with score=1.0 shows `dir=✓`

### Scenario: Conviction check — plans aligned

- **Given** the LLM returns direction "LONG" with trade plans:
  [LONG, LONG, WAIT]
- **When** conviction is checked
- **Then** `conviction_aligned` = true (2 of 3 plans match LONG)

### Scenario: Conviction check — plans misaligned

- **Given** the LLM returns direction "LONG" with trade plans:
  [LONG, SHORT, WAIT]
- **When** conviction is checked
- **Then** `conviction_aligned` = false (only 1 of 3 plans match LONG)
- **And** a warning is logged: "Low conviction: direction=LONG but plans are
  [LONG, SHORT, WAIT]"
- **And** the plans are passed through **unchanged** (no auto-correction)

### Scenario: Conviction check — NONE direction

- **Given** the LLM returns direction "NONE" with any trade plans
- **When** conviction is checked
- **Then** `conviction_aligned` = true (NONE is inherently neutral, no alignment needed)

### Scenario: Notification includes conviction flag

- **Given** a notification is being sent for a WATCH-tier prediction
- **And** `conviction_aligned` = false
- **When** the message is formatted
- **Then** the message includes a "⚠️ Low conviction" marker

### Scenario: Confidence calibration prompt

- **Given** the system prompt is constructed
- **Then** it includes: "Use the full 0-100 range. Avoid rounding to multiples
  of 5. A confidence of 63 is better than 65 when justified."

### Scenario: KB conditional rule — low accuracy

- **Given** the LLM's direction accuracy is below 50%
- **When** the system prompt is constructed
- **Then** it includes a rule: "Your direction accuracy is below 50%. You MUST
  call write_kb('lessons-learned', ...) with your hypothesis about why."

### Scenario: KB conditional rule — high accuracy

- **Given** the LLM's direction accuracy is above 70%
- **When** the system prompt is constructed
- **Then** it includes a rule: "Your direction accuracy is above 70%. Call
  write_kb('successful-setups', ...) to record what's working."

### Scenario: KB conditional rule — repeated direction errors

- **Given** 3 or more of the last 5 scored predictions have the same direction
  and all scored < 0.5
- **When** the system prompt is constructed
- **Then** it includes: "You have multiple wrong calls in the same direction
  recently. Write to write_kb('false-signal-patterns', ...) your analysis."

### Scenario: Recent outcomes injected into context

- **Given** 2 predictions were just scored in the current scan cycle
- **When** the memory context is built
- **Then** the context includes: "Recently validated: [dir=LONG outcome=1.0 ✓],
  [dir=SHORT outcome=0.0 ✗]"

## Validation Rules

- Direction accuracy uses score ≥ 0.5 threshold for "correct" classification
- Conviction check: direction LONG/SHORT requires ≥2 matching plans
- NONE direction skips conviction check (always aligned)
- Plans are NEVER auto-corrected; only logged and flagged
- KB rules are conditional on accuracy stats; no KB rule if <3 scored predictions
- Confidence must be in [0, 100]; system prompt instructs against rounding

## Out of Scope

- Re-requesting from LLM when conviction is low (adds latency/cost)
- Programmatic KB writes (LLM decides what to write)
- Automated KB pruning or summarization
- Confidence post-processing (the LLM owns the number)

## Dependencies

- `docs/specs/outcome-scoring-v2.md` — direction accuracy requires v2 scores
- `docs/specs/persistent-memory.md` — KB read/write tools
- `docs/specs/adaptive-scoring.md` — weight tuning loop

---

## Traceability Matrix

| # | Scenario | Impl Status | Impl Artifact | Test Status | Test Artifact | Notes |
|---|----------|-------------|---------------|-------------|---------------|-------|
| 1 | Direction accuracy summary | ⬚ | — | ⬚ | — | |
| 2 | Per-prediction correctness | ⬚ | — | ⬚ | — | |
| 3 | Conviction — aligned | ⬚ | — | ⬚ | — | |
| 4 | Conviction — misaligned | ⬚ | — | ⬚ | — | |
| 5 | Conviction — NONE | ⬚ | — | ⬚ | — | |
| 6 | Notification conviction flag | ⬚ | — | ⬚ | — | |
| 7 | Confidence calibration prompt | ⬚ | — | ⬚ | — | |
| 8 | KB rule — low accuracy | ⬚ | — | ⬚ | — | |
| 9 | KB rule — high accuracy | ⬚ | — | ⬚ | — | |
| 10 | KB rule — repeated errors | ⬚ | — | ⬚ | — | |
| 11 | Recent outcomes in context | ⬚ | — | ⬚ | — | |

### Gap Summary

- **Scenarios total**: 11
- **Implemented**: 0 / 11
- **Tested**: 0 / 11
- **Blocking gaps**: all

---

## ⚔ Challenge Gate

> **Status**: passed
> **Challenger**: Antigravity (self-review)
> **Date**: 2026-03-27

### Debate Record

| # | Vector | Challenge | Response | Verdict |
|---|--------|-----------|----------|---------|
| 1 | Edge cases | "3+ consecutive same direction" requires tracking sequence order and handling NONE gaps. | Simplified to "3+ of last 5 scored" — window-based, no sequence tracking needed. | challenger-won |
| 2 | Evidence | Will stronger KB prompting actually make the LLM call write_kb? It hasn't in 10+ days. | Using conditional rules ("accuracy <50% → MUST call") instead of suggestions. This is a testable change. If the LLM still doesn't comply after 1 week, escalate to injecting a synthetic tool call. | author-won |

### Challenge Summary

- **Challenges raised**: 2
- **Author victories**: 1
- **Challenger victories**: 1 (simplified consecutive error detection)
- **Escalated**: 0
- **Overall verdict**: ACCEPTED (after revision)

### Revisions Made

- Replaced consecutive detection with window-based (3 of last 5)
