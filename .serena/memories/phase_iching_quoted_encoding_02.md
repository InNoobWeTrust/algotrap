---
kind: phase
topic: iching-quoted-encoding
unit: U2
depth: L2
status: implemented-verified
created: 2026-10-04
updated: 2026-10-04
parent: plan_iching_quoted_encoding.md
target: "4.3.a Downstream Q contract alignment"
depends_on: U1
authority: main-session
memory_backend: serena
---

# U2 — Numerical consumer fixtures and minimal header alignment

## 1. Delivery contract and bounded files

**Outcome:** both bots' physical-cast expectations and actual-terminal LLM fixture match the implemented Q core, with only minimal encoding/time explanation added to the forecast header.

| Contract | Requirement |
|---|---|
| Approved objective | Chosen Q bottom-MSB encoding, unchanged physical casts and C=Q−31.5. Preserve U1 implementation; no new policy, score, interpretation or directional meaning. |
| Must ship | Three actual-fixture RED failures recorded separately; same-physical Q expectations in both presentation tests; real terminal forecast text; minimal two-line header plus factual regressions; focused/package/workspace GREEN evidence. |
| Never defer | Incorrect opening/terminal tuple, physical moving line, numeric reconstruction, misleading header, failed in-scope regression, ignored/deleted/weakened test. |
| May defer | Source prompts/ConfigMap and source/operator/reference/UX documentation to later U3/goal 4.4. U2 is not overall completion until those contracts are consistent and final gates pass. |
| Authority | Main session authorizes implementation, accepts evidence and owns any scope exception. |
| Review budget | One bounded independent plan/acceptance review before code, at most one targeted correction; one final targeted diff/evidence check after implementation, at most one targeted correction. Unresolved findings return to main, not an unbounded loop. |

Exactly one functional unit; steps below are internal TDD sequencing, not separate units.

| Operation | Future writable path | Allowed sections only |
|---|---|---|
| [MODIFY] | `bins/cryptobot/src/presentation.rs` | Test sections: same-physical numeric expected cases in `crossing_bar_exports_opening_moving_line`; preserve existing all-13 mapping coverage. |
| [MODIFY] | `bins/telegrambot/src/presentation.rs` | Test sections: corresponding numeric expected cases in `crossing_bar_exports_opening_moving_line`; preserve existing all-13 mapping coverage. |
| [MODIFY] | `bins/telegrambot/src/llm/mod.rs` | `FORECAST_HEADER`, matching encoding/time-header regression assertions and `forecast_prompt_renders_actual_terminal_cast_and_offsets` expectations only. |

Production presentation, projection and formatter bodies/signatures remain untouched. No new helper, field, type, dependency, DTO, configuration/policy function, tools or registry change. No TA changes, prompts/docs writes, async/clock change, build-infrastructure change, Git mutation/commit/push, PR/deploy/cluster/browser/network action or external scratch script. Planning writes only this phase and the parent link.

## 2. Locked actual interfaces and semantics

Verified against current selected source; declaration excerpts below are preservation locks, not new APIs.

```rust
// cryptobot presentation
fn crypto_output_frame(rows: Vec<CryptoIndicatorRow>, klines: &[Kline],
    tf: Timeframe) -> Result<SourceFrame, MarketError>;
// telegrambot presentation
fn telegram_output_frame(rows: Vec<TelegramIndicatorRow>, klines: &[Kline],
    outputs: &crate::memory::TelegramOutputConfig,
    tf: Timeframe) -> Result<SourceFrame, MarketError>;
// telegrambot llm
fn forecast_prompt_from_frames(ticker: &TickerConf,
    frames: &HashMap<Timeframe, Box<dyn ComputedFrame>>,
    as_of: chrono::DateTime<Utc>) -> String;
fn render_iching_forecast(forecasts: &[(Timeframe, Vec<IchingForecastBar>)],
    default_tf: Timeframe) -> String;
const FORECAST_HEADER: &str = /* only allowed production text change */;
const FORECAST_BUDGET: usize = 320;
```

- Preserve all existing owning types, derives, public fields/nullability and signatures. Presentation returns actual `SourceFrame`; do not infer an obsolete frame API or replace projection.
- Preserve these 13 ordered I-Ching numeric column names and mappings:

| Column | Trajectory field |
|---|---|
| `iching_original_energy` | `energy_open` |
| `iching_transformed_energy` | `transformed_open` |
| `iching_mutual_energy` | `mutual_open` |
| `iching_open` | `energy_open` |
| `iching_high` | `energy_high` |
| `iching_low` | `energy_low` |
| `iching_close` | `energy_close` |
| `iching_moving_line` | `moving_line_open.map(f64::from)` |
| `iching_transformed_close` | `transformed_close` |
| `iching_mutual_close` | `mutual_close` |
| `iching_mutual_high` | `mutual_high` |
| `iching_mutual_low` | `mutual_low` |
| `iching_mutual_mean` | `mutual_mean` |

- `latest_row_exports_full_scheduled_trajectory_for_short_and_h4_bars` in both bots already compares all 13 exported values with real `iching_bar_trajectory` for M1/H4. Retain these dynamic assertions and 13-column count; this mapping oracle must still pass with Q. Preserve ordered names, timestamp units/values/order, nullable numeric shape, pane formulas/styles and forecast sidecar separation through unchanged production code and existing coverage.
- Historical aliases remain opening; future LLM `bin`/`orig`/`ml` are terminal. Formatter state remains `((energy_close + 31.5) as u8, energy_close, moving_line)`; optional `ml` renders `–`, never `null`.
- Keep consecutive state **runs**, not global distinct states: identical neighboring tuples compress; repeated nonadjacent tuples remain separate runs; counts remain `bars / state runs`. Default TF priority, chronological T+ offsets, omission/empty messages, guardrails/placeholders and seven-tool registry are unchanged.
- Header excluded from body budget; preserve `FORECAST_BUDGET * 4` UTF-8-byte heuristic and complete disclaimer when truncated. No tokenizer/budget redesign. Single injected `as_of` and 10/W5/M2 horizon remain unchanged.

### Minimal two-line header text contract

First line stays exactly `[I-CHING CALENDAR — DETERMINISTIC, NOT A MARKET SIGNAL]`. Second line must retain current factual claims while minimally defining:

- `bin=0..63` is the chosen six-bit **binary Q**, not King Wen numbering; physical bottom line 1 has weight 32 and top line 6 weight 1 (full bottom→top weights 32,16,8,4,2,1), Yin0/Yang1.
- `orig=bin-31.5` is C, a plain centered encoding coordinate, not strength or a validated financial score. Keep serialized field names.
- `ml=terminal moving line`, physical 1=bottom and 6=top. No implied bit k−1 relationship; no need to add flip pseudocode to the header. If a test discusses Q masks, physical line k uses `2^(6−k)`.
- Values are the last/terminal cast **inside** `[open,close)`, not a cast at the close instant. A transformed gua is a within-cast line flip, not a fresh future calendar cast.
- T+0 is the first scheduled future bar after the observed anchor open; later offsets count scheduled bars. Preserve the existing `T+0=first scheduled future bar after the observed bar` definition while clarifying the anchor, not the forming bar.
- Retain `Wall-clock calendrical arithmetic. Zero market input. Not a price forecast.`, `terminal cast within each bar`, and subsequent-offset meaning. Exactly one newline; no psychic, direction, strength-score or added decision-policy rules.

`forecast_prompt_header_defines_calendar_tokens` must retain its old valid assertions and additionally require Q weights, polarity, physical line order, plain coordinate/non-strength, half-open ownership and within-cast-versus-fresh-cast explanation. No weakening by deleting the old claims merely to accommodate text.

## 3. TDD sequence and specific changes

### RED — unchanged downstream expectations against correct U1

1. Run each of the three focused commands below separately before numeric edits; each must execute its intended test and record its assertion failure. Fail-fast workspace output alone cannot certify Telegram tests ran. No dependency/compile failure counts as RED.
2. Add/strengthen matching header assertions before header text change and capture the header regression failure. Existing synthetic run/budget fixtures remain untouched.

### Align only the same physical expectations

For both `crossing_bar_exports_opening_moving_line` tests at 2024-02-10 00:00 UTC/H4:

| Value | Old obsolete expectation | Required Q expectation |
|---|---|---|
| Opening Original / `iching_open` alias | B33, +1.5 | Q33, +1.5 unchanged |
| Opening Transformed | B1, −30.5 | Q32, +0.5 |
| Terminal Original | B34, +2.5 | Q17, −14.5 |
| Terminal Transformed | B32, +0.5 | Q1, −30.5 |
| Exported opening moving line | 6 | 6 unchanged |
| Actual terminal moving line | 2 | 2 unchanged |

- Update the three energy expectations in each presentation test; preserve test names, actual candle times and physical meaning. Opening Q33 XOR physical-line-6 mask1 yields Q32; terminal Q17 XOR physical-line-2 mask16 yields Q1. Current searches found **no XOR/mask assertions in either presentation file**; do not invent a production fix or globally reverse integers. Any subsequently discovered pinned mask assertion must preserve its same physical gua/coverage and use Q's physical weight, within allowed test sections; otherwise stop for main.
- Keep `crypto_output_frame(direct_aggregate_rows(&candles), ...)` and `telegram_output_frame(vec![sample_row()], &[candle], ...)` test setup. The latter uses a sampled unrelated indicator row but computes real I-Ching trajectories from the candle; do not fake its I-Ching values.
- In `forecast_prompt_renders_actual_terminal_cast_and_offsets`, keep the two real 4h intervals beginning 2024-02-10 00:00 UTC and calls to `materialize_iching_forecast`/`render_iching_forecast`. Change terminal expected code 34→17 and first rendered row to `T+0 bin=17 orig=-14.5 ml=2`; preserve opening Q33/ml6 checks, rejection of opening tuple in the rendered body, and actual terminal/offset loop for subsequent bars.
- Acceptance data must originate from real computed frames/calendar casts. Do not overwrite trajectory fields or use `forecast_bar` to make the physical fixture pass. Existing synthetic `forecast_bar(34, ...)` / `forecast_bar(19, ...)` are abstract supplied-bin formatter fixtures and **must not be recoded**.
- Align header and matching assertions only after their RED evidence. Do not alter formatter body, run format, helper/signature, policy tests, tool registry or source prompts.

### GREEN and bounded review

3. Rerun all focused tests, then both packages, then complete workspace/all-target tests; never drop/ignore/weaken coverage. Preserve numeric bounds, all-13 mapping, original ordering, actual calendar, optional ml, consecutive/nonadjacent runs, body cap/default priority, placeholder/guardrail and registry regressions.
4. Complete compile/fmt/diff gates and one final targeted scope/evidence review. At most one targeted correction; unresolved issues return to main.

## 4. Acceptance commands and evidence

Commands are for later authorized execution; no command ran during this planning task.

```sh
# Separate RED runs, then repeat for GREEN (confirm intended tests executed):
cargo test --locked --offline -p cryptobot crossing_bar_exports_opening_moving_line
cargo test --locked --offline -p telegrambot crossing_bar_exports_opening_moving_line
cargo test --locked --offline -p telegrambot forecast_prompt_renders_actual_terminal_cast_and_offsets
cargo test --locked --offline -p telegrambot forecast_prompt_header_defines_calendar_tokens

# Package GREEN, then complete workspace coverage:
cargo test --locked --offline -p cryptobot
cargo test --locked --offline -p telegrambot
cargo test --workspace --all-targets --locked --offline
cargo check --workspace --locked --offline
cargo fmt --all --check
git --no-optional-locks diff --check
```

- Record actual pass/fail/ignored counts per target and command exit status, not a ritual fixed total. >=609 tests is an expected reconciliation signal (historical 603 plus six U1 additions), **not** sufficient certification or permission to pad/drop tests. If counts differ, explain actual targets/additions without suppressing failures.
- Required factual GREEN: both Q presentation tuples, all 13 direct-Q trajectory exports in existing tests, actual `T+0 bin=17 orig=-14.5 ml=2`, chronological terminal offsets, complete two-line factual header and all preserved formatter/policy/registry coverage.
- Main-supplied prior evidence: U1 exact-five-file implementation; 84 I-Ching PASS, algotrap 215 PASS, chartlib 29 PASS; prior full workspace compiled but stopped at cryptobot 33 PASS/1 obsolete-fixture failure. Subsequent tests were not executed. These are context, not this phase's test certification.

## 5. Stop conditions and handoff

- Only these three future source files/sections may change. Preserve U1 and unrelated dirty edits. Only two planning files change now; no source edits, tests, Git commands or external actions performed while planning.
- Stop on native permission denial/unavailable approval, missing offline dependency, material contradiction, need to alter formatter/projection/core, out-of-scope test/gate failure or unapproved write. Return INCOMPLETE with exact tool/path or gate, completed evidence and next safe main action; no alternate permission route, downloads or scope expansion.
- Handoff: exact files/sections changed, three focused actual RED failures plus header RED, focused/package/all-target GREEN and actual counts, compile/fmt/diff results, final bounded review disposition, deferred U3 prompt/operator/docs consistency. A full-suite failure is not waived by known obsolete fixtures.
- U2 implementation acceptance does not mean overall migration completion. Next main target is U3/goal 4.4 for minimal source-prompt and source/operator/reference wording alignment; preserve bibliography/authorized repairs and existing context-only policy when that later scope is selected.

## Acceptance checkpoint — 2026-10-04

- **U2 implemented-verified:** actual terminal `bin=17 orig=-14.5 ml=2`, same-physical consumer fixtures and minimal two-line Q header aligned; shapes, historical-opening/future-terminal semantics preserved. Original locked scope and historical instructions above unchanged.
- Main-supplied final tester `ses_efc9c3b7fffeq7kOdJ8AA6PRKA`: fmt/check/build, locked-offline workspace/all-target strict tests (609, zero failed/ignored/filtered), strict Clippy `-D warnings`, `CARGO_NET_OFFLINE=true make duckdb-test` (609 again), diff check all exit0. Implementation reviewer `ses_efca88215ffePyLKH2s2OKBxR5`: **APPROVE** U1/U2/U3 source/test semantics and protected source/YAML policy. U3/final validation complete; see parent completion/handoff.
- Recorded supplied evidence only; no verification rerun or source write. Local uncommitted migration; no stage/commit/push/PR update/deploy.
