# TRD: Adaptive Alert System v2

> **Status**: draft
> **Owner**: Product Owner + Antigravity
> **Created**: 2026-03-17

## Parent PRD

`docs/prds/adaptive-alert-v2.md` — Addresses goals: adaptive weighted scoring,
three-tier response, self-learning via memory, knowledge base, UX improvements.

## Technical Overview

The current stateless alert scanner is replaced with a **stateful, self-tuning**
system. Each scan cycle reads persistent memory (past predictions, weights,
outcomes) and a markdown knowledge base, then feeds this context alongside raw
indicator data to the LLM. The LLM produces a structured response containing
confidence scores, updated weights, trade plan scenarios, and knowledge base
updates. A deterministic tier engine maps the confidence to Alert/Watch/Silent
and applies significant-change detection before deciding whether to notify.

The key architectural principle is **data computes, LLM interprets**: raw
indicator features are extracted deterministically from market data, while the
LLM's role is to reason about which indicators matter in the current context,
tune weights, produce scenario-based trade plans, and maintain the knowledge base.

## Architecture Decisions

### ADR-1: Hybrid scoring — data features + LLM-interpreted weights

- **Context**: Current system asks the LLM for a subjective 0-100 score.
  PO requires data-driven scoring with no fixed thresholds.
- **Decision**: Extract raw indicator features deterministically in Rust and
  present them to the LLM alongside explicit per-indicator weights and memory
  context. The **LLM computes the final confidence score**, using the weights
  as semantic reasoning constraints ("pay 30% attention to RSSI, 25% to
  climax"). Weights guide the LLM's reasoning and are auditable, but are NOT
  used as mathematical multipliers against normalized features.
- **Rationale**: Separates data extraction (Rust, deterministic) from
  interpretation (LLM, contextual). Weights make the LLM's attention
  allocation explicit and adjustable, rather than opaque. Avoids the need
  for a normalization layer (raw indicator ranges differ wildly: RSSI ~30-70,
  structure_power -5 to +5, EMA200 ~82000). The LLM understands indicator
  semantics and scales natively. If the LLM ignores its declared weights,
  the output is still auditable (weights vs. confidence + summary = traceable
  reasoning chain).
- **Alternatives Considered**:
  - Pure algorithmic scoring (z-score) — rejected: PO requires adaptive
    thresholds, fixed formulas repeat the current problem
  - LLM produces score directly without weights (current approach) — rejected:
    proved unreliable, consistently returns 20-30%, no auditability
  - Rust computes weighted composite from normalized features — rejected:
    requires defining normalization for each indicator (fragile, maintenance
    burden), and the LLM may output confidence that disagrees with the
    Rust-computed value, creating a source-of-truth conflict
  - Bayesian optimization for weights — rejected: requires large sample
    sizes, not suited for low-frequency trading decisions

### ADR-2: Persistent state via volume-mounted files

- **Context**: The system needs to retain predictions, weights, and knowledge
  across restarts and scan cycles.
- **Decision**: Use a K8s PersistentVolumeClaim mounted at `/data/memory/`.
  Per-ticker prediction state in JSON (`/data/memory/{symbol}.json`).
  Knowledge base as 10 markdown files in `/data/memory/kb/`.
- **Rationale**: File-based storage is simple, debuggable (operator can read
  files directly), requires no database, and survives pod restarts.
  JSON for structured data, markdown for free-form knowledge.
- **Alternatives Considered**:
  - SQLite — rejected: overkill for 4 tickers, adds dependency
  - ConfigMap/Secret — rejected: not designed for mutable runtime state
  - Redis — rejected: adds infrastructure, stateful set complexity

### ADR-3: Three-tier response engine

- **Context**: Binary alert/silence produces zero output for extended periods.
  PO wants strategic outlook even when no entry exists.
- **Decision**: Deterministic tier engine maps confidence to three tiers:
  - Alert (≥ 70): full analysis + charts + entry plan
  - Watch (40-69): current price + market summary + trade plans. Charts
    only if confidence ≥ 50.
  - Silent (< 40): log only, store prediction in memory
- **Rationale**: Tier boundaries are simple to implement, easy to understand,
  and provide clear UX expectations. Watch tier gives strategic value without
  overwhelming the operator.
- **Alternatives Considered**:
  - Continuous scoring with always-send — rejected: noisy
  - Two tiers (alert/digest) — rejected: misses the "developing setup" case

### ADR-4: Significant-change detection via indicator delta

- **Context**: Watch-tier messages should only fire when something meaningful
  has changed since the last notification, not on every scan cycle.
- **Decision**: Store the indicator snapshot from the last notification per
  ticker. On each scan, compute the max **symmetric percentage delta** across
  a configurable set of key indicators (`CHANGE_DETECTION_INDICATORS`, default:
  `rssi,structure_power,climax_signal`). Formula:
  `delta = |new - old| / max(|old|, |new|, 1.0)`. The floor of 1.0 prevents
  division-by-zero and handles zero-crossing gracefully. If the max delta
  exceeds an LLM-tuned threshold (seeded at 25%), allow a notification.
- **Rationale**: Symmetric percentage avoids directional bias (A→B same as
  B→A). The 1.0 floor handles indicators that cross zero (structure_power
  from +1 to -1 = delta of 1.0/1.0 = 100%, correctly flagged). Configurable
  indicator set prevents volatile minor indicators from triggering noise.
- **Post-deployment revision**: Time-based cooldown (`NOTIFICATION_COOLDOWN_SECS`,
  default 3600) was added alongside delta-based detection. Production observation
  showed that high-delta indicators (>50% swings) triggered Watch-tier
  notifications every 15-min cycle even when the overall signal hadn't changed
  meaningfully. The cooldown acts as a frequency cap: both significant change AND
  cooldown expiry are required. Tier changes still bypass cooldown (important
  state transitions). Additionally, Watch/Silent with direction=NONE are now
  suppressed — only actionable entries (LONG/SHORT) trigger Watch-tier messages.
- **Alternatives Considered**:
  - No cooldown, delta-only (original design) — rejected post-deployment:
    volatile indicators caused excessive notifications
  - No cooldown (send every cycle) — rejected: too noisy at 15-min intervals
  - Percentage relative to old value only — rejected: breaks on zero-crossing

### ADR-5: Selective knowledge base loading

- **Context**: 10 KB files could consume significant context window tokens.
  Not all topics are relevant for every scan.
- **Decision**: Load KB files selectively based on relevance. On each scan,
  load the 3-4 most relevant KB topics:
  - Always: `weight-rationale.md`, `prediction-retrospective.md`
  - Per-ticker: `ticker-personalities.md` (filtered to current symbol)
  - Conditional: `market-regimes.md` if ATR regime has shifted, others as
    LLM requests via a `read_kb`/`write_kb` tool
- **Rationale**: Context budget management. 3-4 files × ~500 tokens = ~2K,
  well within limits. LLM can request additional topics via tool calls.
- **Alternatives Considered**:
  - Load all 10 always — rejected: wastes ~5K tokens, degrades focus
  - No selective loading, just size limits — rejected: doesn't address
    relevance, just volume

### ADR-6: LLM tool for KB read/write

- **Context**: The LLM needs to both read and update the knowledge base
  during a scan cycle.
- **Decision**: Add two new LLM tools: `read_kb(topic)` returns a KB file's
  content; `write_kb(topic, content, mode)` appends or replaces content
  in a KB file. The LLM calls these as needed during analysis.
- **Rationale**: Tool-based access keeps the LLM in control of what it
  reads and writes, rather than dumping everything into the prompt. Enables
  the LLM to decide which topics are relevant for the current analysis.
- **Alternatives Considered**:
  - Inject all KB into system prompt — rejected: too large, not selective
  - Post-analysis KB update only — rejected: LLM can't reference KB
    observations during analysis

## System Components

- **`memory.rs`** [NEW]: Memory management — read/write per-ticker JSON,
  atomic writes (temp+rename), sliding window enforcement (max 8 predictions,
  today + last day), indicator snapshot storage.
- **`kb.rs`** [NEW]: Knowledge base file I/O — read/write markdown files in
  `/data/memory/kb/`, seed empty files on first run, enforce 10-topic limit,
  expose as LLM tool handlers.
- **`scoring.rs`** [NEW]: Tier engine — determine tier (Alert/Watch/Silent)
  from LLM-produced confidence score. Significant-change detection via
  symmetric indicator delta. Weight guardrails (bounds, rate limiting).
  Tier thresholds configurable via env vars. Notification gating via
  `should_notify`: time-based cooldown + direction filter (NONE suppressed
  for non-Alert tiers) + tier-change bypass.
- **`llm/mod.rs`** [MODIFY]: Add memory + KB context injection to prompts.
  Parse expanded LLM response (weights, trade plans, KB updates,
  significance threshold). New `AnalysisMode::AdaptiveScan` or refactor
  existing `AlertScan`. **Chat history compression**: during multi-step tool
  calling, compress older conversation messages beyond `KEEP_RECENT_MESSAGES`
  (configurable, default 10) into a single summary message to manage token
  budget.
- **`llm/tools.rs`** [MODIFY]: Add `read_kb` and `write_kb` tool definitions
  and execution handlers.
- **`commands.rs`** [MODIFY]: Add `/start`, `/status`, `/digest`, `/weights`
  handlers. Add unknown-message handler and default fallback to teloxide
  dispatcher.
- **`main.rs`** [MODIFY]: Tiered response logic in scan loop — Alert sends
  charts+analysis, Watch sends summary+plans (charts if ≥50), Silent logs
  only. Significant-change gating. **Outcome validation at scan start**
  (before LLM analysis): validate all non-scored predictions still in the
  sliding window.
- **`telegram.rs`** [MODIFY]: Watch-tier message formatting (price + summary +
  trade plan options). Format for `/status`, `/digest`, `/weights` responses.
- **`config.rs`** [MODIFY]: Add `MEMORY_DIR` env var (default `/data/memory`).
  Add `WEIGHT_RATE_LIMIT` (default 0.05), `WEIGHT_MIN` (default 0.05),
  `WEIGHT_MAX` (default 0.50) for configurable weight guardrails.
  Add `MAX_PREDICTIONS` (default 8) for memory sliding window size.
  Add `KEEP_RECENT_MESSAGES` (default 10) for chat history compression
  threshold. Add `TIER_ALERT_THRESHOLD` (default 70) and
  `TIER_WATCH_THRESHOLD` (default 40) for configurable tier boundaries.
  Add `CHANGE_DETECTION_INDICATORS` (default `rssi,structure_power,climax_signal`).
  Add `NOTIFICATION_COOLDOWN_SECS` (default 3600) for per-ticker cooldown.
  Remove deprecated `CONFIDENCE_THRESHOLD` (now LLM-tuned).

## API Contracts / Interfaces

### LLM Structured Response (Alert/Watch scan mode)

```json
Input (injected into prompt context):
  - indicator_features: normalized per-TF feature vector
  - memory: last N predictions with outcomes
  - current_weights: weight map from last cycle
  - kb_context: pre-loaded relevant KB topics

Output (LLM final response, parsed by Rust):
  - confidence: f64 (0-100, weighted composite)
  - direction: "LONG" | "SHORT" | "NONE"
  - weights: { rssi: f64, climax: f64, ema200: f64, momentum: f64, ... }
  - trade_plans: [{ label: "A"|"B"|"C", entry: f64|null, direction: str,
                    sl: f64|null, description: str }]
  - summary: str (market analysis prose)
  - significance_threshold: f64 (0-100, for next cycle's change detection)

Validation:
  - Each weight must be in [0.05, 0.50]
  - Weights must sum to ≤ 1.0 (excess normalized)
  - Weight change from previous cycle capped at ±0.05 per weight
  - Confidence clamped to [0, 100]
  - At least 2 trade_plans required (may include "Wait/NONE" plan)
```

### read_kb Tool

```
Function: read_kb

Input:
  - topic: string — one of the 10 topic slugs (e.g., "market-regimes")

Output:
  - content: string — markdown content of the KB file, or empty template
    if not yet written

Errors:
  - Unknown topic → list valid topics
```

### write_kb Tool

```
Function: write_kb

Input:
  - topic: string — one of the 10 topic slugs
  - content: string — markdown content to write
  - mode: "append" | "replace" — append adds a timestamped section,
    replace overwrites the file

Output:
  - confirmation: string — "Updated {topic}.md ({mode})"

Errors:
  - Unknown topic → list valid topics
  - Content exceeds 2000 chars → truncation warning
```

### Memory JSON Schema

```json
{
  "predictions": [
    {
      "timestamp": "ISO-8601",
      "confidence": 45.0,
      "direction": "LONG",
      "weights": { "rssi": 0.3, ... },
      "trade_plans": [
        { "label": "A", "entry": 82100.0, "direction": "LONG",
          "sl": 81200.0, "description": "..." }
      ],
      "indicator_snapshot": { "rssi_1h": 42.0, ... },
      "significance_threshold": 25.0,
      "outcome_score": null
    }
  ],
  "current_weights": { "rssi": 0.25, "climax": 0.25, "ema200": 0.25,
                        "momentum": 0.25 },
  "last_notified_snapshot": { "rssi_1h": 40.0, ... },
  "last_notified_at": "ISO-8601"
}
```

## Data Models

### TickerMemory

| Field | Type | Constraints | Description |
| --- | --- | --- | --- |
| predictions | Vec\<Prediction> | max 8, sliding window | Past predictions with outcomes |
| current_weights | HashMap\<String, f64> | each ∈ [0.05, 0.5] | LLM-tuned weights |
| last_notified_snapshot | HashMap\<String, f64> | — | Indicator values at last notification |
| last_notified_at | Option\<DateTime> | — | Timestamp of last notification sent |

### Prediction

| Field | Type | Constraints | Description |
| --- | --- | --- | --- |
| timestamp | DateTime | — | When prediction was made |
| confidence | f64 | [0, 100] | Weighted confidence score |
| direction | String | LONG/SHORT/NONE | Predicted direction |
| weights | HashMap\<String, f64> | — | Weights used for this prediction |
| trade_plans | Vec\<TradePlan> | ≥ 2 | Scenario A/B/C plans |
| indicator_snapshot | HashMap\<String, f64> | — | Key indicator values at prediction time |
| significance_threshold | f64 | [0, 100] | LLM-set threshold for next change detection |
| outcome_score | Option\<f64> | [0, 1] | Validated accuracy (filled on next scan) |

### TradePlan

| Field | Type | Constraints | Description |
| --- | --- | --- | --- |
| label | String | A/B/C/... | Plan identifier |
| entry | Option\<f64> | — | Entry price (null for "Wait" plans) |
| direction | String | LONG/SHORT/NONE | Trade direction |
| sl | Option\<f64> | — | Stop-loss price |
| description | String | — | Scenario description |

## Security Assessment

### Authentication & Authorization

- **Auth model**: Single-operator deployment. Bot token via K8s Secret.
  No user authentication beyond Telegram chat ID.
- **Access control**: Commands restricted to configured `TELEGRAM_CHAT_ID`.
  No multi-user, no role differentiation.

### Data Protection

- **Data classification**: Memory files contain price predictions and market
  analysis — not PII, but commercially sensitive to the operator.
- **Encryption at rest**: Relies on K8s PV encryption (cluster-level).
  No application-level encryption for memory files.
- **Secrets management**: All API keys/tokens remain in K8s Secrets. Memory
  files contain no credentials.

### Input Validation & Injection Prevention

- **LLM output validation**: All parsed JSON from LLM responses is validated
  against schema. Weights bounded [0.05, 0.5], rate-limited (±0.05/cycle).
  Confidence clamped [0, 100]. Unknown fields ignored.
- **KB write validation**: Content length capped at 2000 chars per write.
  Topic names validated against fixed whitelist of 10 slugs. Markdown only,
  no executable content.
- **Tool argument validation**: `read_kb`/`write_kb` topic arguments validated
  against enum. Invalid topics return an error listing valid options.

### Infrastructure & Configuration

- **PersistentVolume**: ReadWriteOnce, mounted at `/data/memory/`.
  Pod uses `Recreate` strategy (only 1 replica), so no concurrent access.
- **No new network exposure**: Memory is local filesystem, no new services.

### Supply Chain & Dependencies

- **No new crate dependencies**: Memory/KB use `std::fs`, `serde_json`,
  existing `tokio::fs`. No new external dependencies.

### Failure Modes

- **Memory file corruption**: Atomic write (write to `.tmp`, then rename).
  If rename fails, previous state is preserved.
- **PV unavailable**: Fail-open — log warning, operate stateless (current
  behavior). Memory features degrade gracefully.
- **LLM returns invalid weights**: Validation rejects, fall back to previous
  cycle's weights from memory. Log warning.

## Non-Functional Requirements

- **Performance**: Watch-tier scan cycle < 30s per ticker (no chart capture).
  Alert-tier ≤ 60s (includes chart capture). Memory I/O < 10ms.
- **Storage**: Memory JSON ~5KB per ticker. KB files ≤ 2KB each. Total
  ~70KB — trivial PV size.
- **Observability**: Log tier decisions (Alert/Watch/Silent) with confidence
  and delta values. Log weight changes. Log outcome validation scores.
- **Reliability**: Graceful degradation on memory loss. Pod restart resumes
  from last persisted state.

## Child BDD Specs

- `docs/specs/adaptive-scoring.md` — Weighted scoring, LLM weight tuning,
  guardrails (bounds, rate limits), cold start defaults
- `docs/specs/tiered-response.md` — Alert/Watch/Silent tier engine,
  significant-change detection, chart gating at 50%
- `docs/specs/persistent-memory.md` — Memory read/write, sliding window,
  outcome validation, KB CRUD, selective loading
- `docs/specs/bot-ux-v2.md` — /start, /status, /digest, /weights, unknown
  message handler, channel post support

## ⚔ Challenge Gate

> **Status**: passed
> **Challenger**: Antigravity (self-review)
> **Date**: 2026-03-17

### Debate Record

| # | Vector | Challenge | Response | Verdict |
|---|--------|-----------|----------|---------|
| 1 | Assumptions | ADR-1 assumes the LLM can meaningfully set weights within bounded ranges. But with a ±0.05 rate limit, it would take 5 cycles to move a weight from 0.25 to 0.50 — is this responsive enough to regime changes? | The rate limit prevents oscillation, not responsiveness. Most regime changes unfold over hours/days, not minutes. At 15-min cycles, 5 cycles = 75 minutes to fully shift a weight — reasonable for a daily trading timeframe. If too slow, the rate limit is a config constant (easily tunable). | author-won |
| 2 | Edge cases | What if the LLM consistently requests weights that violate bounds? The system clamps/normalizes, but does the LLM know its requested weights were adjusted? | Currently no — the LLM sees its requested weights in memory next cycle (post-clamping), so it indirectly sees the adjustment. Should add a note in the prompt: "your requested weights may be clamped to [0.05, 0.5] bounds." This makes the constraint explicit. | challenger-won |
| 3 | Security | `write_kb` tool allows the LLM to write arbitrary markdown to disk. Could a jailbroken LLM write malicious content or overwrite system files? | Topic names are validated against a fixed whitelist (10 slugs). Paths are constructed as `{MEMORY_DIR}/kb/{topic}.md` — no path traversal possible. Content is treated as opaque markdown (never executed). 2000-char limit prevents disk fill. The risk is the LLM writing nonsensical KB content, which degrades analysis quality but causes no security harm. | author-won |

### Challenge Summary

- **Challenges raised**: 3
- **Author victories**: 2
- **Challenger victories**: 1 (must revise: explicitly communicate weight bounds to LLM in prompt)
- **Escalated**: 0
- **Overall verdict**: ACCEPTED (after revision)

### Revisions Made (if any)

- Added requirement to include weight bounds [0.05, 0.5] and rate limit ±0.05 in the LLM system prompt, so the LLM knows its constraints before generating weights.
