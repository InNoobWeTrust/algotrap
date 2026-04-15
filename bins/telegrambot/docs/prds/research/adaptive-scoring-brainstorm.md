# Brainstorming Session: Adaptive Scoring & Strategic Outlook

**Date**: 2026-03-17
**Technique(s) used**: SCAMPER, What If…, Problem Framing (3 Whys)

## Problem Statement

The telegrambot's alert system uses an undefined, LLM-subjective confidence
score to gate trade signals. After 2 days of operation, every scan returned
confidence 20-30% (below threshold 70%) producing zero output. This fails
the data-first principle and provides no strategic value between entries.

### Root Cause (3 Whys)

1. Why no signals? → Confidence consistently 20-30%, below 70% threshold
2. Why so low? → No rubric — LLM guesses conservatively with no measurement criteria
3. Why is silence the only response? → Binary design: alert or nothing. No middle ground.

### Success Criteria

- Output is **data-driven** — confidence derived from measurable indicator conditions
- Bot **self-improves** — reflects on past predictions, tunes weights dynamically
- User gets **strategic outlook** even when no entry exists
- **Never 2 days of silence** — significant changes trigger notifications
- First-time bot UX is smooth

## All Ideas (grouped by theme)

### Theme A: Adaptive Weighted Scoring

- A1. Weighted composite score from key indicators (RSSI, climax_signal, EMA200+momentum)
- A2. LLM dynamically tunes weights per scan cycle based on market conditions
- A3. Persistent memory via volume-mounted JSON per ticker
- A4. Outcome tracking — compare predictions against actual price movement
- A5. Freestyle memory — LLM can note exceptional findings worth remembering
- A6. Weight guardrails to prevent degenerate configurations

### Theme B: Strategic Outlook & Tiered Output

- B1. Three tiers: Alert (≥70) / Watch (40-69) / Silent (<40)
- B2. Multi-TF momentum trajectory (RSSI/structure_power direction over last N scans)
- B3. Scenario planning: trade plan options A/B/C for different outcomes
- B4. Significant-change detection (>25% delta or LLM-set threshold) for hourly notifications
- B5. Charts only when confidence ≥50% (better than random)
- B6. Watch message content: current price + market summary + trade plan options

### Theme C: UX Improvements

- C1. `/start` welcome with bot description and command list
- C2. Unknown message handler → "use /help"
- C3. `/status` — last scan results per ticker
- C4. `/weights` — show current LLM-tuned weights
- C5. `/digest` — on-demand summary of all tickers

## Top Ideas (prioritized)

### 1. Adaptive Weighted Scoring with Memory (A1–A6)

**Summary**: Replace subjective LLM confidence with weighted indicator composite.
LLM tunes weights dynamically, reflects on past prediction accuracy via
persistent JSON memory. Freestyle notes for exceptional findings.

**Why it matters**: Data-first confidence that self-improves over time.

**Key decisions**:
- Trusted indicators: RSSI (noisy at low ATR), climax_signal, EMA200 (high TF + momentum)
- Memory: sliding window — today's + last day's predictions, max 8. Escalate to
  nearest available if last day empty
- Outcome validation: LLM reads its trade plan options (A/B/C). No match = 0.
  Each correct entry+direction = +1, divided by total matching options over time range
- Weights tuned dynamically — online learning, no fixed thresholds

**Risks**: LLM might set degenerate weights → need bounded ranges (guardrails)

### 2. Tiered Response with Momentum Trajectory (B1–B6)

**Summary**: Three tiers of output. Watch tier shows: current price + market
summary + trade plan options. Charts only when confidence ≥50%.

**Why it matters**: Eliminates 2-day silence. User gets strategic awareness.

**Key decisions**:
- Significant change = >25% delta from last message OR dynamically set by LLM
- Prefer online learning over fixed thresholds for change detection
- Watch message verbosity: current price, market summary, trade plan options
- No charts below 50% confidence (worse than random = don't waste resources)

**Risks**: Could become noisy → LLM-tuned significance threshold mitigates

### 3. UX Polish (C1–C5)

**Summary**: Complete command set for first-time and ongoing UX.

**Key decisions**: AI suggests best UX patterns. Low effort, no risk.

## Action Plan

This document feeds into the **Research Brief** → **PRD** → **TRD** → **BDD specs**
cascade per the requirements-driven-dev lifecycle.
