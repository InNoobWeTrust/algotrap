# Feature: In-Session Scratchpad + Forced Handoff

> **Status**: draft
> **Owner**: InNoobWeTrust
> **Created**: 2026-03-28
> **Research**: `docs/prds/research/prompt-engineering-v2.md` (Approach 4)
> **Ref**: Anthropic "Context Engineering" framework (Sep 2025) — Write / Compress / Select / Isolate

## Parent Spec

`docs/specs/llm-prompt-engineering.md` — Chat history compression (Scenario group to replace)

## Description

Replace the current lossy `compress_history` (a separate LLM call that summarizes and discards old messages) with an agent-controlled in-session scratchpad. The LLM uses `write_notes` / `read_notes` tools to persist observations across turns. When context limits approach, a forced handoff mechanism ensures the LLM saves its state before messages are cleared.

`compress_history` is retained as a last-resort fallback when the LLM refuses to write handoff notes.

## User Stories

- As a **bot operator**, I want the LLM to manage its own working memory during a scan, so that important observations are never lost to lossy compression.
- As a **bot operator**, I want context handoffs to be instantaneous (no extra LLM call), so that scan cycles complete faster.
- As a **bot operator**, I want a safety net when the LLM doesn't cooperate with handoff, so that the system never enters a degraded state.

## Scenarios

### Scenario 1: LLM writes notes voluntarily

- **Given** the `write_notes` tool is available to the LLM
- **When** the LLM calls `write_notes` with key `"observations"` and content `"RSSI at 72 on 4h, structure_power bearish divergence on 1h"`
- **Then** the content is stored in an in-memory HashMap keyed by `"observations"`
- **And** the tool returns `"Noted."`
- **And** the HashMap is session-scoped (not persisted to disk)

### Scenario 2: LLM reads notes

- **Given** the scratchpad contains 2 entries: `observations` and `conflicts`
- **When** the LLM calls `read_notes` with no key (read all)
- **Then** the tool returns all entries formatted as:
  ```
  [observations]: RSSI at 72 on 4h, structure_power bearish divergence on 1h
  [conflicts]: Sharpe positive on 1d but negative on 4h
  ```
- **And** when called with key `"observations"`, it returns only that entry

### Scenario 3: Notes overwrite on same key

- **Given** the scratchpad has key `"observations"` with value `"old data"`
- **When** the LLM calls `write_notes` with key `"observations"` and content `"updated data"`
- **Then** the old value is replaced by `"updated data"`

### Scenario 4: Context reset — scratchpad has entries

- **Given** the scratchpad has entries (LLM has written notes)
- **And** message count exceeds `keep_recent_messages * 2` (handoff threshold — each tool call adds ~2 messages, so this gives room for 10+ tool calls before triggering)
- **When** the system detects the threshold is crossed
- **Then** all messages except the system prompt are dropped
- **And** a user message is injected: `"[Your analysis notes from earlier turns]:\n{all entries}"`
- **And** the LLM continues with fresh context + its own notes
- **And** no extra LLM call is made (unlike `compress_history`)

### Scenario 5: Forced handoff — scratchpad is empty

- **Given** the scratchpad is empty (LLM has not written any notes)
- **And** message count exceeds the handoff threshold
- **When** the system detects the threshold is crossed
- **Then** the system injects a user message: `"⚠️ Context limit approaching. Call write_notes with key 'handoff' to save your current analysis state before history is cleared."`
- **And** the LLM is given one turn to respond
- **And** if the LLM calls `write_notes`, proceed to context reset (Scenario 4)

### Scenario 6: Forced handoff — LLM ignores first directive

- **Given** the system injected a forced handoff directive (Scenario 5)
- **And** the LLM responded without calling `write_notes` (e.g., it produced a text response or called a different tool)
- **When** the system checks the response
- **Then** a stronger directive is injected: `"⚠️ You MUST call write_notes('handoff', '<your observations>') NOW. No other actions."`
- **And** the LLM is given one more turn

### Scenario 7: Forced handoff — LLM ignores both directives (fallback)

- **Given** the LLM ignored both handoff directives (Scenarios 5 and 6)
- **When** the system checks the second response
- **Then** `compress_history` is invoked as the last-resort fallback (legacy lossy path)
- **And** a warning is logged: `"LLM refused handoff; falling back to compress_history"`

### Scenario 8: Scratchpad does not interfere with KB

- **Given** the LLM calls both `write_notes("observations", "...")` and `write_kb("indicator-quirks", "...")`
- **When** the scan completes
- **Then** scratchpad entries are discarded (session-scoped)
- **And** KB entries are persisted to disk (cross-session)
- **And** the two storage mechanisms do not interfere

### Scenario 9: Scratchpad injection includes all keys

- **Given** the scratchpad has entries: `observations`, `conflicts`, `handoff`
- **When** context reset occurs
- **Then** the injected user message includes all 3 entries, each prefixed with its key
- **And** order is deterministic (alphabetical by key)

### Scenario 10: Empty scratchpad at scan start

- **Given** a new scan cycle begins
- **When** `run_agent` is called
- **Then** the scratchpad is initialized as an empty HashMap
- **And** no notes from previous scan cycles exist (session-scoped)

### Scenario 11: Parallel write_notes in one turn

- **Given** the LLM issues multiple tool calls in a single response: `write_notes("observations", "...")` and `write_notes("conflicts", "...")` simultaneously
- **When** the tool handler processes both calls
- **Then** both entries are stored in the scratchpad
- **And** if two calls target the same key, the last-processed call wins (HashMap insert order)

## Validation Rules

- Scratchpad keys are free-form strings (LLM chooses)
- Values are arbitrary text (no size limit, but LLM naturally writes concisely)
- `write_notes` always succeeds (no validation on content)
- `read_notes` with a nonexistent key returns `"No notes found for key '<key>'."`
- Handoff threshold = `keep_recent_messages * 2` (default: 20 messages). Each tool call generates ~2 messages (assistant tool_call + tool result), so this gives ~10 tool calls before triggering. The `* 2` multiplier prevents premature handoff that would interrupt data gathering.
- Forced handoff gets at most 2 attempts before fallback
- `compress_history` remains in the codebase as fallback, not deleted

## Changes Required

### Tool definitions (`config/prompts/tools.json`)

- Add `write_notes` tool: `{ key: string, content: string }` → returns `"Noted."`
- Add `read_notes` tool: `{ key?: string }` → returns formatted entries or specific entry

### LLM module (`src/llm/mod.rs`)

- Add `session_scratchpad: HashMap<String, String>` to `run_agent` scope
- Replace `compress_history` trigger with handoff check:
  - If scratchpad non-empty → context reset (drop + inject notes)
  - If scratchpad empty → inject forced handoff directive, set `pending_handoff` flag
  - If 2 handoff attempts fail → fall back to `compress_history`
- Add `context_reset()` helper: drops messages, injects scratchpad as user message
- Add `inject_handoff_directive()` helper: creates the system-injected message

### Tool execution (`src/llm/tools.rs`)

- Add `write_notes` handler: insert into scratchpad HashMap
- Add `read_notes` handler: return formatted entries
- **Signature change**: `execute_tool_call` must accept `&mut HashMap<String, String>` (scratchpad) as an additional parameter, threaded through from `run_agent`

### Prompt template (`config/prompts/system_adaptive.txt`)

- Add to Analysis Workflow or a dedicated section:
  ```
  Use write_notes to record key observations as you analyze.
  Your notes persist across context resets within this session.
  ```

## Traceability Matrix

| Scenario | Implementation | Test |
| -------- | -------------- | ---- |
| 1: write_notes stores entry | tools.rs handler | unit |
| 2: read_notes returns entries | tools.rs handler | unit |
| 3: overwrite on same key | tools.rs handler | unit |
| 4: context reset with notes | mod.rs handoff logic | unit |
| 5: forced handoff (empty pad) | mod.rs handoff logic | unit |
| 6: retry stronger directive | mod.rs handoff logic | unit |
| 7: fallback to compress_history | mod.rs handoff logic | unit |
| 8: scratchpad vs KB isolation | tools.rs | unit |
| 9: injection includes all keys | mod.rs context_reset | unit |
| 10: empty scratchpad at start | mod.rs run_agent | unit |
| 11: parallel write_notes | tools.rs handler | unit |

## Verification

- Unit tests for all 10 scenarios (scratchpad CRUD, handoff logic, fallback chain)
- Integration test: run a mock multi-turn agent loop where messages exceed threshold, verify:
  - With notes: context reset preserves notes, no compress_history call
  - Without notes: forced handoff injects directive, LLM writes, then reset
  - Stubborn LLM: fallback to compress_history after 2 attempts
- Staging: run 24h with scratchpad enabled, compare analysis quality vs. compress_history baseline
