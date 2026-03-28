# Feature: Prompt Optimization — Reasoning Effort + Template Compression

> **Status**: draft
> **Owner**: InNoobWeTrust
> **Created**: 2026-03-28
> **Research**: `docs/prds/research/prompt-engineering-v2.md` (Approaches 2 + 3)

## Parent Spec

`docs/specs/llm-prompt-engineering.md` — Template variable rendering, output schema

## Description

Two quick-win optimizations to improve LLM output quality while reducing token consumption:
1. Enable `reasoning_effort: high` for gpt-5-mini via litellm config to trade latency for deeper analysis
2. Compress the 38-line JSON output template to ~10 lines, removing redundant instructions

Both changes are independent and can be deployed separately. Combined, they improve quality (reasoning) while reducing noise (compression).

## User Stories

- As a **bot operator**, I want the LLM to reason more deeply before producing analysis, so that predictions are better calibrated.
- As a **bot operator**, I want the system prompt to be compact, so that more context window is available for data and tool results.
- As a **bot operator**, I want reasoning strategy to adapt to the model, so that I can swap models without manual prompt editing.

## Scenarios

### Scenario 1: Reasoning effort enabled via litellm config

- **Given** the litellm config has `reasoning_effort: high` for the gpt-5-mini model
- **When** the LLM receives a chat completion request (with or without tools)
- **Then** litellm passes `reasoning_effort: high` to the upstream provider
- **And** the model uses internal chain-of-thought before producing visible output
- **And** reasoning tokens are consumed from the `max_completion_tokens` budget

### Scenario 2: max_completion_tokens caps reasoning cost

- **Given** `reasoning_effort: high` is enabled
- **And** `max_completion_tokens` is set (e.g., 4096)
- **When** the model's internal reasoning + visible output exceeds the cap
- **Then** the response is truncated at `max_completion_tokens`
- **And** the system handles truncated responses gracefully (parse failure → NONE direction, 0 confidence)

### Scenario 3: Tool-calling requests pass reasoning_effort

- **Given** the LLM request includes tool definitions
- **When** litellm forwards the request with `reasoning_effort: high`
- **Then** the model still applies reasoning effort to tool selection and argument generation
- **And** if litellm does **not** pass reasoning_effort on tool-calling requests, a warning is logged and the config is updated to use per-request injection instead

### Scenario 4: Compressed JSON template — correct key production

- **Given** the system prompt uses the compressed output template (10 lines)
- **And** the template includes one concrete example per field (e.g., `"rssi": 0.20`)
- **When** gpt-5-mini produces its final JSON output
- **Then** the output contains all required keys: `confidence`, `direction`, `summary`, `weights`, `trade_plans`, `significance_threshold`
- **And** `weights` uses actual indicator names (not literal `"<indicator>"`)
- **And** `trade_plans` contains at least 2 entries with `label`, `direction`, `entry`, `target`, `stop`, `rationale`

### Scenario 5: Compressed template — optional fields

- **Given** the compressed template
- **When** the LLM omits `indicator_params` entirely
- **Then** the system treats it as no-op (no indicator param changes)
- **And** existing indicator config is retained unchanged

### Scenario 6: CoT prompt — non-reasoning models only

- **Given** `EnvConf` has `supports_reasoning: false` (or the field is absent, defaulting to false)
- **When** the system renders the prompt
- **Then** a 3-line chain-of-thought trigger is appended to the system prompt
- **And** if `supports_reasoning: true`, the CoT trigger is **not** included (prevents double-reasoning waste)
- **And** the `supports_reasoning` flag is set per model in `EnvConf` or derived from litellm model metadata if available

### Scenario 7: Merged instruction sections

- **Given** the current prompt has separate "Weight rules" and "Indicator Tuning" sections
- **When** the compressed prompt is deployed
- **Then** these are merged into a single "Indicator Weights & Tuning" section
- **And** the "Analysis Workflow" steps are removed (tool definitions guide behavior)
- **And** the KB section is reduced to 1-2 lines
- **And** confidence calibration retains tier numbers but drops the "avoid rounding" prose

## Validation Rules

- `reasoning_effort` must be one of: `low`, `medium`, `high` (model-dependent)
- `max_completion_tokens` must be set when `reasoning_effort` is enabled (hard cap)
- CoT prompt and `reasoning_effort` are mutually exclusive — never combine
- Compressed template must retain at least one concrete field example to prevent literal placeholder reproduction

## Changes Required

### litellm config (`k8s/litellm-config.yaml` or `k8s/litellm.yaml`)

- Add `reasoning_effort: high` under `litellm_params` for gpt-5-mini model
- Add `max_completion_tokens: 4096` (or appropriate cap)

### Prompt template (`config/prompts/system_adaptive.txt`)

- Compress JSON template from 38 to ~10 lines
- Merge Weight rules + Indicator Tuning sections
- Remove Analysis Workflow numbered steps
- Compact KB section
- Add conditional CoT trigger (only for non-reasoning models)

### Bot code (optional, for CoT conditionality)

- If CoT must be conditional on model capability: add `supports_reasoning` config flag to `EnvConf`
- If litellm handles everything: no code change needed

## Verification

- Deploy to staging with gpt-5-mini
- Run 10 analysis cycles and verify:
  - JSON output parses correctly (all required fields present, correct types)
  - Weight keys are real indicator names
  - Trade plans have ≥2 entries
  - Response latency is acceptable (< 60s per cycle with reasoning_effort: high)
- Compare confidence distribution before/after (should use full 0-100 range, not cluster at multiples of 5)
