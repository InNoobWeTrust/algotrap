# Feature: LLM Prompt Engineering (Adaptive Mode)

> **Status**: approved
> **Owner**: Product Owner
> **Created**: 2026-03-18

## Parent TRD

`docs/trds/adaptive-alert-v2.md` — System Components (`llm/mod.rs` context injection),
ADR-1 (hybrid scoring), ADR-5 (selective KB loading)

## Description

Define the prompt template contract for the adaptive alert system. The LLM receives
a system prompt with injected memory context, indicator weights, and past outcome
history. It uses tools (including KB read/write) to gather data, then outputs a
structured JSON response with confidence, direction, weights, trade plans, and
significance threshold. This spec defines the template variables, rendering rules,
analysis mode selection, output schema contract, and chat history compression.

## User Stories

- As the **operator**, I want the LLM to see its past predictions and outcomes in
  the prompt, so that it can self-calibrate confidence.
- As the **operator**, I want weight constraints communicated in the prompt, so that
  the LLM respects guardrails without post-hoc surprises.
- As the **operator**, I want prompts externalized in ConfigMap files, so that I can
  tune prompt wording without recompiling.

## Scenarios

### Scenario: Analysis mode selects prompt files

- **Given** the scan loop calls `run_agent` with `AnalysisMode::AlertScan`
- **When** the system loads prompt files
- **Then** `system_adaptive.txt` is loaded as the system prompt
- **And** `user_adaptive.txt` is loaded as the user message
- **And** `tools.json` is loaded (shared across all modes, includes `read_kb`/`write_kb`)
- **And** the full analysis mode (`FullAnalysis`) continues to use `system.txt` / `user.txt`

### Scenario: Template variable rendering — warm start

- **Given** BTC-USDT memory exists with 3 predictions (1 scored, 2 unscored)
- **And** current weights are `{ "rssi": 0.30, "structure_power": 0.25, ... }`
- **When** the system renders `system_adaptive.txt`
- **Then** `{{symbol}}` → `BTC-USDT`
- **And** `{{time}}` → current UTC timestamp
- **And** `{{tfs}}` → Debug-formatted timeframe list (e.g., `[M15, H1, H4]`)
- **And** `{{weight_min}}` → `0.05` (from `EnvConf`)
- **And** `{{weight_max}}` → `0.50` (from `EnvConf`)
- **And** `{{weight_rate_limit}}` → `0.05` (from `EnvConf`)
- **And** `{{memory_context}}` → compact text listing last N predictions with
  timestamps, confidence, direction, and outcome score if available
- **And** `{{weights_context}}` → formatted current weight table
- **And** `{{outcome_summary}}` → aggregate outcome stats (e.g., "3 predictions,
  1 validated at 0.67 accuracy")

### Scenario: Template variable rendering — cold start (no memory)

- **Given** BTC-USDT has no memory file (first run)
- **When** the system renders `system_adaptive.txt`
- **Then** `{{memory_context}}` → `No previous predictions. This is a cold start.`
- **And** `{{weights_context}}` → `No previous weights. Use equal attention across all indicators.`
- **And** `{{outcome_summary}}` → `No past predictions to evaluate yet.`

### Scenario: Memory context is compact (token budget)

- **Given** the memory contains 8 predictions (max window)
- **When** the system renders `{{memory_context}}`
- **Then** the output is ≤ 400 tokens (~1600 characters)
- **And** each prediction is summarized in one line:
  `[timestamp] confidence=X direction=DIR outcome=SCORE|pending`
- **And** trade plan details are NOT included (available via memory, not in prompt)

### Scenario: Weights context format

- **Given** current weights are `{ "rssi": 0.30, "structure_power": 0.25, "climax_signal": 0.15, "atr_reversion_percent": 0.10, "sharpe": 0.10, "ema200": 0.10 }`
- **When** the system renders `{{weights_context}}`
- **Then** output is:

  ```text
  Current weights (from previous cycle):
    rssi: 0.30
    structure_power: 0.25
    climax_signal: 0.15
    atr_reversion_percent: 0.10
    sharpe: 0.10
    ema200: 0.10
  Significance threshold: 0.25 (25% change in any key indicator triggers re-notification)
  ```

### Scenario: Outcome summary format

- **Given** 5 predictions exist, 3 with outcome scores (0.67, 0.33, 1.00)
- **When** the system renders `{{outcome_summary}}`
- **Then** output includes:
  - Number of validated predictions and average accuracy
  - Example: `Your past 5 predictions: 3 validated (avg accuracy: 0.67). High accuracy means your signals are reliable. Low accuracy suggests some indicators may need re-weighting — do not simply suppress confidence.`

### Scenario: LLM output schema — valid response

- **Given** the LLM produces its final response
- **When** the response is parsed
- **Then** the following fields are extracted:
  - `confidence` (f64, clamped 0-100)
  - `direction` (String: "LONG" | "SHORT" | "NONE")
  - `summary` (String, 2-4 sentences)
  - `weights` (HashMap<String, f64>, per-indicator)
  - `trade_plans` (Vec, ≥2 plans with label/direction/entry/target/stop/rationale)
  - `significance_threshold` (f64, 0.0-1.0)
- **And** weights are post-processed through guardrails (clamp + rate limit)
- **And** significance_threshold is stored in memory for next cycle

### Scenario: LLM output schema — missing optional fields

- **Given** the LLM returns JSON without `weights` or `significance_threshold`
- **When** the response is parsed
- **Then** previous cycle's weights are retained (no change)
- **And** previous cycle's significance_threshold is retained
- **And** `confidence`, `direction`, `summary`, `trade_plans` are still extracted
- **And** a warning is logged noting the missing fields

### Scenario: LLM output schema — unparseable response

- **Given** the LLM returns non-JSON or invalid JSON
- **When** the response is parsed
- **Then** confidence is set to 0 (Silent tier)
- **And** direction is "NONE"
- **And** previous weights and significance_threshold are retained
- **And** an error is logged with the raw response text

### Scenario: Chat history compression

- **Given** the LLM agent is in a multi-turn tool-calling loop
- **And** the conversation has 15 messages (system + user + tool calls + responses)
- **And** `KEEP_RECENT_MESSAGES` is configured to 10
- **When** a new turn is about to be sent
- **Then** the 5 oldest non-system messages are extracted
- **And** a separate LLM call (fresh context, no tools) summarizes them into
  a single human-role message preserving semantic meaning (indicator trends,
  cross-TF signals, key observations)
- **And** the summary replaces the old messages in the conversation
- **And** the system message is always preserved (never compressed)
- **And** the most recent 10 messages are preserved in full
- **And** the compression LLM call uses a short system prompt:
  `"Summarize the following analysis conversation into a concise paragraph.
  Preserve key indicator readings, timeframe observations, and any patterns
  noted. Do not add new analysis."`

### Scenario: Prompt files are externalized (ConfigMap compatible)

- **Given** the prompt directory contains `system_adaptive.txt` and `user_adaptive.txt`
- **When** the operator modifies prompt wording and applies via `kubectl apply`
- **Then** the next scan cycle uses the updated prompts
- **And** no recompilation or Docker rebuild is required

## Validation Rules

- Template rendering must replace ALL `{{var}}` placeholders — no literal `{{` in output
- `{{memory_context}}` must be ≤ 400 tokens (~1600 characters)
- LLM output schema parsing must be backwards-compatible with the existing
  `{ confidence, direction, summary }` format (graceful degradation)
- Weights from LLM output must always pass through `apply_weight_guardrails`
- `trade_plans` parsing must tolerate 0 plans (fallback: empty vec)
  but the prompt instructs ≥2
- All prompt files must use `{{placeholder}}` syntax consistent with
  `render_prompt` in `llm/mod.rs`
- Chat history compression must never drop the system message or the
  most recent tool call/response pair
- Compression uses a separate LLM call (fresh context, no tools) — the
  main analysis LLM never sees the raw old messages, only the summary

## Out of Scope

- Prompt A/B testing framework
- Automatic prompt optimization / tuning
- Multi-model prompt variants (single model only)
- Streaming LLM responses

## Dependencies

- `docs/specs/adaptive-scoring.md` — defines weight bounds communicated in prompt
- `docs/specs/persistent-memory.md` — memory provides past predictions and outcomes
- `docs/specs/architecture.md` — prompt loading mechanism (`load_and_render_prompt`)

## ⚔ Challenge Gate

> **Status**: passed
> **Challenger**: Antigravity (self-review)
> **Date**: 2026-03-18

### Debate Record

| # | Vector | Challenge | Response | Verdict |
|---|--------|-----------|----------|---------|
| 1 | Assumptions | `AlertScan` → `system_adaptive.txt` is a breaking change from `AlertScan` → `system_alert.txt`. What about the old prompt? | Old prompt stays in ConfigMap for rollback. This is a deliberate upgrade, not an accidental break. | author-won |
| 2 | Edge cases | Chat history compression says "compressed into a single summary message" — who writes the summary? The LLM can't summarize its own context. | A separate LLM call with fresh context summarizes the old messages. This preserves semantic meaning (indicator trends, cross-TF signals) that code-based truncation would lose. | author-won (fixed) |
| 3 | Evidence | "≤ 400 tokens (~1600 characters)" — is this mapping accurate? | Approximate guideline, not a hard assert. Code truncates by character limit. Token count is model-dependent. | author-won |
| 4 | Alternatives | Outcome summary "if accuracy is low, be more conservative" could create a death spiral (low accuracy → lower confidence → fewer alerts → less data → stays low). | Fixed: reworded to "High accuracy means reliable signals. Low accuracy suggests re-weighting indicators — do not simply suppress confidence." | author-won (fixed) |
| 5 | Longevity | 6 hardcoded indicator names — every doc needs updating when adding indicators. | Indicators change very rarely (requires Polars pipeline changes). Single source in config is over-engineering for v2. | author-won |

### Challenge Summary

- **Challenges raised**: 5
- **Author victories**: 5 (2 with fixes)
- **Challenger victories**: 0
- **Escalated**: 0
- **Overall verdict**: ACCEPTED

