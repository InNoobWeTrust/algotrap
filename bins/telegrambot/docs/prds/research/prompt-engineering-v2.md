# Brainstorm: Prompt Engineering & Reasoning Strategy

> **Context**: gpt-5-mini via litellm proxy, 15-min scan cycle, current 116-line system prompt with a 38-line JSON template

## Problem

The current system prompt is **verbose + monolithic**:
- 38-line JSON template forces all decisions in a single pass
- 8 distinct concerns crammed into one output: confidence, direction, summary, weights, trade plans (×2-3), significance threshold, indicator params
- Small models struggle with "do everything at once" — they satisfice on the first few fields and degrade on later ones
- The template itself consumes significant context window

## Approach 1: Progressive Tool-Calling (Structured Decomposition)

**Idea**: Instead of one massive JSON output, the LLM uses tool calls to progressively submit parts of its analysis. Each part is validated independently.

### Architecture

```
Turn 1: LLM calls data-gathering tools (existing behavior)
Turn 2: LLM calls `submit_assessment(confidence, direction, summary)`
Turn 3: LLM calls `submit_trade_plans([{plan A}, {plan B}])`
Turn 4: LLM calls `submit_weights({rssi: 0.3, ...})`
Turn 5 (optional): LLM calls `submit_indicator_params({rssi: {period: 10}})`
Final: LLM produces empty content (end turn) → pipeline assembles result
```

### Pros
- Each tool call has a focused schema — LLM sees one small JSON per step
- Validation per-step: reject bad trade plans immediately, LLM can retry
- Natural ordering: analysis → plans → weights → tuning (each depends on prior reasoning)
- Smaller models perform better on focused tasks

### Cons
- **More turns = more latency + tokens** — 4-5 extra round trips per scan
- **Completeness guardrails**: What if the LLM stops after `submit_assessment` and never submits plans? Need timeout + minimum-steps enforcement
- **Error recovery**: A failed turn 3 means partial state — need rollback or retry logic
- **History compression**: More messages in chat history to compress
- **Tool schema maintenance**: More tool definitions = more tokens in each request

### Verdict
Worth exploring only if output quality is measurably degraded on the monolithic approach. The extra latency and complexity are significant. **A simpler first step: reduce the JSON template size** (see Approach 3).

---

## Approach 2: Reasoning Effort via API Parameter

**Idea**: Use `reasoning_effort` parameter (supported by gpt-5-mini up to `high`) to trade latency for quality.

### How It Works
- gpt-5-mini builds an internal chain-of-thought (reasoning tokens) before the visible output
- Higher effort = more reasoning tokens = better multi-step analysis
- Reasoning tokens count as output tokens (billed, consume context window)
- Latency increase is acceptable: 15-min scan cycle gives ample budget

### Implementation

#### Option A: API-Level (litellm passthrough)

LiteLLM supports `reasoning_effort` passthrough. Add to `EnvConf`:

```rust
#[serde(default = "default_reasoning_effort")]
pub reasoning_effort: Option<String>, // "low" | "medium" | "high"
```

Pass via `extra_body` or model-specific params in the chat completion request. The `async-openai` crate v0.33 may not expose `reasoning_effort` natively — would need raw JSON injection via litellm's extra params.

LiteLLM config approach (simpler — no code change):
```yaml
model_list:
  - model_name: gpt-5-mini
    litellm_params:
      model: openai/gpt-5-mini
      reasoning_effort: high
```

#### Option B: Prompt-Level CoT Fallback

For models that don't support `reasoning_effort`, inject a chain-of-thought trigger in the system prompt:

```
Before producing output, think step-by-step:
1. What is the dominant trend across timeframes?
2. Do momentum indicators (RSSI, structure_power) agree?
3. Are there conflicting signals? Which indicators disagree?
4. Based on my past predictions' outcomes, what adjustments are needed?
Then produce the JSON.
```

This is less effective than true reasoning tokens but still improves output quality for non-reasoning models.

#### Option C: Hybrid (Recommended)

- Set `reasoning_effort: high` in litellm config for gpt-5-mini (zero code change)
- Add `max_completion_tokens` to bound worst-case reasoning token consumption
- **Do NOT add prompt-level CoT alongside reasoning_effort** — combining them causes double-reasoning (model reasons internally, then outputs CoT text, wasting tokens). CoT prompt is for non-reasoning models only.

### Risks

- **Tool-calling + reasoning_effort**: Needs verification that litellm passes `reasoning_effort` through on tool-calling requests. Some providers treat tool selection and reasoning as separate paths.
- **Invisible token consumption**: Reasoning tokens are billed but not visible in the response. Without `max_completion_tokens`, the model could burn 50k+ reasoning tokens before producing output. Hard cap is mandatory.

### Verdict
**High-value, low-effort change.** LiteLLM config is a one-liner. Must verify tool-calling path and set `max_completion_tokens`.

---

## Approach 3: Reduce Prompt Verbosity (Prerequisite for Both)

**Idea**: Compress the JSON template and remove redundant instructions regardless of other changes.

### Current waste

1. **Full JSON template with comments** (38 lines): The model already knows JSON. A compact schema reference is enough.
2. **Hardcoded weight keys in template**: These should come from `indicator_config_context`, not be duplicated.
3. **Repetitive trade plan entries**: Show structure once, say "2-3 plans required".
4. **Inline explanations in template**: `"significance_threshold": <0.10-0.50, how much indicator change...>` — this instruction belongs in the section header, not in the JSON.

### Proposed compressed template

```
Output: JSON only, no markdown fences.
{
  "confidence": 0-100,
  "direction": "LONG|SHORT|NONE",
  "summary": "2-4 sentences: evidence, conflicts, comparison to past",
  "weights": { "<indicator>": <{{weight_min}}-{{weight_max}}> },
  "trade_plans": [{ "label": "A", "direction": "LONG|SHORT|WAIT",
    "entry": price|null, "target": price|null, "stop": price|null,
    "rationale": "1 sentence" }],
  "significance_threshold": 0.10-0.50,
  "indicator_params": { "<name>": {"period": int, "active": bool} }
}
```

This is 10 lines vs. 38 — saving ~400 tokens per request.

### Risk: Over-Compression

Small models need explicit examples. A compact schema like `"weights": { "<indicator>": <range> }` risks the LLM producing literal `"<indicator>"` as a key. Mitigation: keep one concrete example inline (e.g., `"rssi": 0.20`) and let the model extrapolate. Test empirically before deploying.

### Additional compression opportunities

- **Merge "Weight rules" + "Indicator Tuning" sections**: Both are about indicator params. One section, half the tokens.
- **Remove "Analysis Workflow" steps 1-5**: The tool definitions already guide tool usage. Small models don't follow 5-step workflows — they call tools based on their own heuristics.
- **Compact KB section**: One sentence instead of six lines.
- **Remove confidence calibration prose**: Keep the tier numbers, drop the "avoid rounding" paragraph. The model either follows it or doesn't — more prose doesn't help.

### Estimated savings
Current system prompt: ~4,674 bytes (~1,200 tokens)
Proposed: ~2,500 bytes (~650 tokens)
Savings: ~550 tokens per request × 96 scans/day = 52,800 tokens/day

## Approach 4: Context Engineering — Scratchpad + Handoff (Anthropic Pattern)

**Idea**: Replace the current lossy `compress_history` (LLM summarization that discards tool results) with structured in-session memory that the agent controls.

### Background

Anthropic's "context engineering" framework (Sep 2025) identifies four strategies:

| Strategy | Purpose | Our Equivalent |
|----------|---------|----------------|
| **Write** (scratchpad) | Persist state outside context window | `read_kb`/`write_kb` (cross-session), **missing for in-session** |
| **Compress** (compaction) | Reclaim space by summarizing | `compress_history` (current, lossy) |
| **Select** (just-in-time) | Load only what's needed, when needed | Tool-based data gathering (existing) |
| **Isolate** (multi-agent) | Separate concerns across agents | N/A for single-agent |

**Key insight**: Compaction is lossy — it discards information the model might need later. **Scratchpad is lossless** — the agent decides what's worth keeping and writes it explicitly. It's the difference between someone summarizing your meeting notes vs. you taking your own notes.

### How It Maps to the Bot

The current flow:
```
System prompt → LLM calls tools (data gathering) → compress_history(lossy) → LLM outputs JSON
```

Proposed flow with scratchpad:
```
System prompt → LLM calls tools → LLM calls write_notes(findings) → LLM calls tools → 
LLM calls write_notes(more) → ... → LLM reads notes + outputs JSON
```

#### In-Session Scratchpad Tool

Add a `write_notes` / `read_notes` tool that stores to an in-memory HashMap (not persisted — it's session-scoped):

```rust
// New tool: write_notes
fn write_notes(key: &str, content: &str) -> String {
    // Store in-memory, keyed by topic
    // Keys: "observations", "conflicts", "plan_thinking", etc.
    session_scratchpad.insert(key, content);
    "Noted."
}

// New tool: read_notes  
fn read_notes(key: Option<&str>) -> String {
    // Return all notes or specific key
    // Injected into final turn context
}
```

#### Token Overhead

Adding 2 tool definitions (~200 tokens) to every request is a cost. Current 5 tools → 7 tools = ~40% more tool schema tokens. Mitigation: the tools are tiny (simple key-value), and the savings from dropping `compress_history` LLM calls more than compensate.

#### Interaction with Reasoning Effort

`reasoning_effort: high` provides internal (invisible) CoT for the current turn. The scratchpad provides external (visible, persistent) state across turns. They are complementary, not redundant:
- Internal reasoning: helps the model think through a complex tool result within one turn
- Scratchpad: carries observations across turns when context resets happen

This replaces the current `compress_history` approach:
- **Before**: System compresses old messages → loses tool result details
- **After**: LLM writes key observations to scratchpad → old messages can be **dropped entirely** without loss because the LLM already saved what matters

#### Why This Is Better Than Compression

| Aspect | compress_history (current) | Scratchpad (proposed) |
|--------|---------------------------|----------------------|
| What's preserved | Whatever a separate LLM call decides | Whatever the analysis LLM explicitly chose to save |
| Information quality | Lossy — summarizer doesn't know what's important for the final decision | Lossless — the analyst saves what it considers relevant |
| Token cost | Extra LLM call for compression | Zero — just HashMap read/write |
| Latency | Synchronous LLM call mid-loop | Instant (in-memory) |
| Context bloat | Summary still takes space | Notes are compact by nature (agent writes sparingly) |

#### Interaction with KB System

The existing `read_kb`/`write_kb` is **cross-session** (persisted to disk, carries observations across 15-min cycles). The scratchpad is **in-session** (dies at end of scan, holds intermediate analysis for the current cycle). They're complementary:

- `write_notes`: "RSSI at 72 on 4h, structure_power aligning bullish across 1h+4h"
- `write_kb`: "BTC tends to fake out at RSSI 70+ during low-volume weekends" (persistent insight)

### Prompt Changes

Instead of "Your FINAL response must be ONLY this JSON", the prompt becomes:

```
As you analyze, use write_notes to record key observations.
When ready, produce the final JSON as your text response.
```

The analysis workflow section shrinks because the scratchpad naturally structures the agent's thinking — it doesn't need 5 explicit steps in the prompt.

### Forced Handoff: Guardrail for Lazy Note-Takers

**Problem**: The LLM may never call `write_notes`. If we naively drop old messages when context fills up, we lose everything.

**Solution**: A system-level **forced handoff** — the pipeline detects approaching token limits and injects a directive forcing the LLM to write its current state before context is cleared.

#### Mechanism

The agent loop (`run_agent`) already tracks message count. Replace the current `compress_history` trigger with a 3-phase protocol:

```
Phase 1: Normal operation
  LLM calls tools, optionally calls write_notes
  System monitors: message_count > threshold OR estimated tokens > budget

Phase 2: Forced handoff (system-injected)
  System injects a user message:
    "⚠️ Context limit approaching. Before continuing, call write_notes 
     with key 'handoff' to save your current analysis state: key observations,
     preliminary direction, any unresolved conflicts. Your previous messages 
     will be cleared after you save."
  LLM MUST respond with write_notes("handoff", "...") tool call
  Pipeline validates: if LLM doesn't call write_notes, retry once with 
    stronger directive; if still no notes, fall back to compress_history

Phase 3: Context reset
  Drop all messages except: system prompt + scratchpad injection
  Scratchpad contents are injected as a user message:
    "[Your analysis notes from earlier turns]:\n{all scratchpad entries}"
  LLM continues with fresh context, full state preserved in scratchpad
```

#### Implementation (in `run_agent`)

```rust
// Before each LLM call, check if handoff is needed
let estimated_tokens = estimate_message_tokens(&messages);
let needs_handoff = messages.len() > conf.keep_recent_messages 
    || estimated_tokens > TOKEN_BUDGET_THRESHOLD;

if needs_handoff && !scratchpad.is_empty() {
    // Scratchpad has notes — safe to clear
    context_reset(&mut messages, &scratchpad);
} else if needs_handoff && scratchpad.is_empty() {
    // No notes yet — force the LLM to write before clearing
    inject_handoff_directive(&mut messages);
    // Next turn: LLM will call write_notes, then we clear
    pending_handoff = true;
}
```

#### Fallback Chain

| Condition | Action |
|-----------|--------|
| Scratchpad has entries | Safe to clear old messages, inject notes |
| Scratchpad empty + LLM responds to forced handoff | Clear after note is written |
| Scratchpad empty + LLM ignores forced handoff (turn 1) | Retry with stronger directive |
| Scratchpad empty + LLM ignores forced handoff (turn 2) | Fall back to `compress_history` (legacy lossy path) |

This means `compress_history` isn't deleted — it becomes the **last-resort fallback** when the LLM refuses to cooperate with handoff directives. Covers the edge case without losing robustness.

#### Token Budget Estimation

Since we're using litellm → OpenAI, we can estimate tokens cheaply:
- System prompt: ~1,200 tokens (fixed, known at startup)
- Each tool call + result: ~200-500 tokens (vary by tool)
- Total budget: model's context window minus headroom for final output

The threshold should be calibrated per model:

```rust
// Heuristic: trigger handoff when messages accumulate beyond what's useful
// The real constraint is per-turn context, not total window
let handoff_threshold_messages = conf.keep_recent_messages + 2; // default: 12
// Optional: rough token estimate = messages × avg_tokens_per_message (~400)
```

Exact tokenization isn't needed. Message count is a sufficient proxy — the current `keep_recent_messages` (default 10) already acts as a rough threshold. The key insight: trigger handoff _before_ the context degrades, not at the absolute limit.

---

## Recommendation: Priority Order

1. **[Quick win] LiteLLM reasoning_effort: high** — One-line config change + `max_completion_tokens` cap. Verify tool-calling passthrough before deploying.
2. **[Quick win] Compress prompt template** — Cut JSON template from 38→10 lines, keep one concrete example per field. Test with gpt-5-mini to verify it still produces correct keys.
3. **[Medium effort] In-session scratchpad + forced handoff** — Replace lossy `compress_history` with agent-controlled `write_notes`/`read_notes` + 3-phase forced handoff guardrail. `compress_history` becomes last-resort fallback.
4. **[Conditional] CoT trigger** — Only for non-reasoning models. Do NOT combine with `reasoning_effort` (causes double-reasoning waste).
5. **[Evaluate later] Progressive tool calling** — Only if monolithic JSON quality is measurably bad after steps 1-4.

## Open Questions

1. **async-openai v0.33**: Does it support `reasoning_effort` natively, or do we need litellm-level passthrough only? If litellm config is sufficient, no crate update needed.
2. **Token budget**: With `reasoning_effort: high`, how many reasoning tokens does gpt-5-mini typically generate? Need to set `max_completion_tokens` to avoid runaway costs.
3. **Prompt compression vs. context richness**: How much instruction can we remove before the model starts making structural errors (e.g., wrong JSON keys, missing trade plans)?
4. **Scratchpad key design**: Should the keys be free-form (LLM chooses) or constrained (e.g., only "observations", "conflicts", "assessment")? Free-form is more flexible but constrained keys map better to the final JSON structure.
5. **Scratchpad injection**: Should notes be injected into the final turn's context automatically, or should the LLM call `read_notes` explicitly? Automatic injection is more reliable for small models.

