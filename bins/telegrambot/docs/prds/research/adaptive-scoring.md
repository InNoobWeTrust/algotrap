# Research Brief: Adaptive Scoring, Strategic Outlook & Bot UX

> **Status**: draft
> **Researcher**: Antigravity + Product Owner
> **Created**: 2026-03-17

## Context

After deploying the multi-ticker alert system (2026-03-15), 2 days of operation
produced **zero alerts** across all 4 tickers — every scan returned confidence
20-30%, well below the 70% threshold. Investigation reveals three structural
problems: undefined scoring methodology, missing strategic output below
threshold, and incomplete bot UX.

## Research Question

How should the alert system produce data-driven confidence scores, provide
strategic market outlook even when no entry is detected, and create a polished
user experience for Telegram bot interactions?

---

## Finding 1: Current Confidence Scoring Is Subjective

### Evidence

The current `system_alert.txt` prompt asks the LLM to rate 0-100 with loose
criteria:

- "Multi-timeframe trend agreement"
- "RSSI at an extreme (< 46 for LONG, > 54 for SHORT)"
- "ATR reversion percent supporting the direction"
- "Climax signal confirmation"

Problems:

| Issue | Impact |
| --- | --- |
| No defined weights for each indicator | LLM guesses conservatively, defaulting to 20-30% |
| Fixed RSSI thresholds (46/54) don't adapt to market conditions | Miss valid entries in low-volatility regimes |
| No feedback loop — LLM doesn't know if past predictions were right | Cannot self-correct over time |
| Single numeric score hides which factors are strong vs weak | Operator cannot diagnose why scores are low |

### Available Indicators (from `tools.rs` lines 155-166)

| Indicator | Source | Signal |
| --- | --- |--- |
| `rssi` | Structure-power derived | Momentum extremes (noisy at low ATR) |
| `rssi_ma` | Smoothed RSSI | Trend of momentum |
| `structure_power` | Multi-candle structure | Trend direction/strength |
| `structure_power_sma` | Smoothed | Trend consistency |
| `climax_signal` | ATR+volume derived | Exhaustion/reversal events |
| `atr_reversion_percent` | Mean reversion | Distance from ATR mean |
| `ema200` | 200-period EMA | Major trend reference |
| `sharpe` | Risk-adjusted return | Quality of trend |
| `atr_percent` | Volatility | Market regime |
| `leverage` | ATR-based | Position sizing signal |

### Product Owner's Trusted Indicators

1. **RSSI** — but noisy when ATR is low
2. **climax_signal** — ATR_reversion + climax but not mean momentum exhaustion
3. **EMA200** — only on high timeframes, and only when momentum not exhausted
   on 1-2 higher timeframes

### Solution Direction

**Weighted composite scoring** with LLM-tuned weights:

- Compute individual indicator scores from data (deterministic)
- LLM dynamically adjusts weights based on current market condition
- Weights stored in persistent memory for continuity
- LLM reflects on past prediction accuracy to self-correct

Key constraint from PO: **no fixed thresholds** — market conditions fluctuate,
the system must adapt. Confidence score remains the probability metric, but
its derivation changes from subjective LLM guess to data-driven computation
with LLM-tuned parameters.

---

## Finding 2: Binary Alert/Silence Fails the User

### Evidence

Current system has two states:

| Score | Behavior |
| --- | --- |
| ≥ threshold | Alert (charts + entry plan) |
| < threshold | **Complete silence** (only logged) |

This produced 2 days of zero output. The user receives no strategic value
from the system during this time — no market awareness, no developing setup
tracking, no scenario planning.

### User Requirements (from brainstorm)

- **Multi-TF momentum trajectory**: not just point-in-time snapshot, but
  direction of indicator change across 2-3 past scans
- **Trade plan options**: scenario-based (A/B/C plans for different outcomes)
- **Significant-change notification**: hourly if there's meaningful change
  from last notification (LLM-tuned threshold, seeded at 25% delta)
- **Charts only when confidence ≥ 50%** (better than random)

### Solution Direction

**Three-tier response system**:

| Tier | Score Range | Output |
| --- | --- | --- |
| 🎯 Alert | ≥ 70 | Full analysis + charts + entry plan |
| 👁️ Watch | 40-69 | Current price + market summary + trade plan options |
| 🔇 Silent | < 40 | Nothing sent (logged only, stored in memory) |

Plus **significant-change detection**: compare current indicator snapshot
against last-notified snapshot. If delta exceeds threshold (LLM-tuned,
seeded at 25%), send a Watch-tier update even if the score itself
hasn't crossed a tier boundary.

---

## Finding 3: Bot UX Gaps

### Evidence

- **`/start`**: User presses "Start" in Telegram → `/start` sent → no response
- **Unknown messages**: User types free text → no response → confusion
- **No status command**: User cannot check last scan results on demand
- **No digest command**: User cannot get an all-ticker summary on demand

### Solution Direction

| Command | Response |
| --- | --- |
| `/start` | Welcome message: bot description, capabilities, `/help` reminder |
| `/help` | Existing — list all commands |
| `/list` | Existing — configured tickers |
| `/analyze <SYMBOL>` | Existing — full manual analysis |
| `/status` | Last scan results per ticker (score, direction, key levels) |
| `/digest` | On-demand summary of all tickers (compact momentum map) |
| `/weights` | Show current LLM-tuned weights (transparency) |
| Unknown text | "I don't understand. Use /help to see available commands." |

---

## Finding 4: Self-Learning via Persistent Memory

### Evidence

Current system is stateless — no memory between scan cycles. The LLM cannot:

- Compare current indicators to past scans (no trajectory data)
- Evaluate whether its past predictions were correct
- Adapt its scoring weights based on experience

### Design (from brainstorm with PO)

**Persistent JSON memory** per ticker, volume-mounted in K8s:

```json
{
  "predictions": [
    {
      "timestamp": "2026-03-17T10:00Z",
      "symbol": "BTC-USDT",
      "confidence": 45,
      "direction": "LONG",
      "weights": {
        "rssi": 0.3,
        "climax": 0.25,
        "ema200": 0.2,
        "momentum_trajectory": 0.25
      },
      "trade_plans": [
        {"label": "A", "entry": 82100, "direction": "LONG", "sl": 81200},
        {"label": "B", "entry": 83500, "direction": "SHORT", "sl": 84200},
        {"label": "C", "label_desc": "Wait", "entry": null, "direction": "NONE"}
      ],
      "indicator_snapshot": {
        "rssi_1h": 42, "structure_power_4h": -1.2, "climax_1d": 0
      },
      "outcome": null
    }
  ],
  "current_weights": {
    "rssi": 0.3, "climax": 0.25, "ema200": 0.2, "momentum_trajectory": 0.25
  }
}
```

**Sliding window**: retain predictions from today + last day. If last day
has no predictions, escalate to nearest available. Max `MAX_PREDICTIONS`
(configurable, default 8) predictions.

**Outcome validation** (LLM-based):

1. Read past trade plan options (A/B/C)
2. Check if any matches reality (current price vs predicted entry/direction)
3. No match = 0, each correct entry+direction = +1
4. Score = matching options / total options over the time range

**Knowledge base** — markdown files, one per topic, stored alongside the memory
JSON in the volume mount (e.g., `memory/kb/`). LLM reads relevant topics before
each scan and can update them with new observations. Max **10 topics**:

| # | Topic File | Purpose |
| --- | --- | --- |
| 1 | `market-regimes.md` | Observed volatility regimes and how indicators behave in each (trending, ranging, low-ATR) |
| 2 | `indicator-quirks.md` | Known indicator edge cases (e.g., "RSSI is noisy when ATR < X", "climax_signal lags in low volume") |
| 3 | `ticker-personalities.md` | Per-ticker behavioral patterns (e.g., "XAUT trends slowly, BTC whipsaws at round numbers") |
| 4 | `timeframe-dynamics.md` | Cross-TF correlations (e.g., "15m structure_power flip usually precedes 1H flip by 2-3 candles") |
| 5 | `weight-rationale.md` | Why current weights are set as they are — what market condition drove each adjustment |
| 6 | `prediction-retrospective.md` | Lessons from past correct/incorrect predictions — what worked, what failed, recurring mistakes |
| 7 | `risk-patterns.md` | Observed risk events, false signals, and failure modes to avoid |
| 8 | `timing-patterns.md` | Time-of-day/week patterns (e.g., "BTC volume drops on weekends, avoid entries before Sunday close") |
| 9 | `cross-ticker-signals.md` | Correlations/divergences between tickers (e.g., "SOL leads ETH moves by ~30min in trending markets") |
| 10 | `strategy-evolution.md` | High-level strategic notes on how the system's approach has changed over time and why |

Each file is plain markdown, managed by the LLM. The LLM can:
- **Read** relevant topics before analysis (loaded into context)
- **Append** new observations after each scan cycle
- **Revise** existing entries when new evidence contradicts them
- **Prune** outdated observations (LLM judgment, with timestamp-based staleness)

The user can also read these files to learn from the bot's observations and
verify the bot's reasoning quality.

---

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
| --- | --- | --- | --- |
| LLM sets degenerate weights (all on one indicator) | Medium | High | Bounded weight ranges: min 0.05, max 0.5 per factor |
| Watch-tier messages become noisy | Medium | Medium | LLM-tunable significance threshold; start at 25% delta |
| Outcome validation is unreliable with few predictions | High (initially) | Low | Degrade gracefully — no weight updates until ≥ 3 outcomes available |
| Memory file corruption | Low | Medium | Atomic write (write to temp then rename); backup on each scan |
| LLM overfits weights to recent noise | Medium | Medium | Weight change rate limit (max ±0.05 per cycle); exponential decay on older outcomes |

---

## Recommendation

Proceed with all three themes as a single PRD ("Adaptive Alert System v2"):

1. **Adaptive weighted scoring** with LLM-tuned weights and persistent memory
2. **Three-tier response system** with momentum trajectory and scenario planning
3. **UX improvements** for commands and unknown input handling

These are tightly coupled — the memory system enables both adaptive scoring
and trajectory tracking, and the tiered response uses the adaptive score.

## ⚔ Challenge Gate

> **Status**: passed
> **Challenger**: Antigravity (self-review)
> **Date**: 2026-03-17

### Debate Record

| # | Vector | Challenge | Response | Verdict |
|---|--------|-----------|----------|---------|
| 1 | Assumptions | "LLM can meaningfully tune weights" — is there evidence that LLMs perform well at online parameter tuning vs specialized algorithms (e.g., Bayesian optimization, bandit algorithms)? | The goal isn't optimal weight tuning — it's *adaptive* tuning grounded in market context. LLMs excel at contextual reasoning ("ATR is low so RSSI is noisy, reduce its weight"). Simple bounded weight adjustment with rate limits prevents instability. If LLM tuning proves poor, the fallback is static equal weights + LLM interpretation only — a strict improvement over current subjective scoring. | author-won |
| 2 | Alternatives | Why not compute a fixed algorithmic score without LLM involvement (e.g., z-score composite)? This would be deterministic, backtestable, and faster. | PO explicitly rejected fixed thresholds — "market conditions fluctuate, the entry can fluctuate." A fixed z-score composite would replicate the problem of the current fixed RSSI thresholds (46/54). The LLM's value is interpreting *which* indicators matter *right now* given the current regime. Hybrid approach (data computes features, LLM interprets + tunes weights) captures both. | author-won |
| 3 | Evidence | Outcome validation using LLM comparison of trade plans vs reality — how reliable is this? The LLM might be biased toward confirming its own past predictions. | Valid concern. Mitigation: outcome validation uses a separate prompt (not the same context as the prediction). Additionally, the scoring formula (matching options / total options) is deterministic once matches are identified — the LLM only classifies "did price move to plan A's entry at 82100?" which is a factual price comparison, not a subjective judgment. But we should monitor for self-confirmation bias. | author-won (with note to monitor) |

### Challenge Summary

- **Challenges raised**: 3
- **Author victories**: 3 (1 with monitoring note)
- **Challenger victories**: 0
- **Escalated**: 0
- **Overall verdict**: ACCEPTED
