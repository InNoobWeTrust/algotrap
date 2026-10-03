---
kind: plan
topic: iching-quoted-encoding
depth: L2
refined_goal: "4.1.b coordinated core-TA migration plus 4.1.a TDD prerequisite"
phase: phase_iching_quoted_encoding_01.md
current_refinement: "L2 only: 4.4.a Minimal Q explanation alignment"
explanation_phase: phase_iching_quoted_encoding_03.md
consumer_phase: phase_iching_quoted_encoding_02.md
status: implemented-verified
created: 2026-10-04
updated: 2026-10-04
approval: "User approved coordinate-only migration; main session selects next target"
memory_backend: serena
---

# I-Ching quoted encoding — strategic outline

## 1. Approved outcome

- Adopt `Q = Σ physical_line[i] * 2^(5-i)` over the unchanged bottom-first array: bottom weight 32, top weight 1; Yang=1, Yin=0. All existing energy fields remain centered coordinates `E = Q - 31.5`, range `[-31.5,+31.5]`.
- External numeric codes intentionally change: Bác/Bo Q1, Phục/Fu Q32, Tiết/Jie Q50. Preserve field names, functions, types and nullability; no dual-encoding DTOs or compatibility aliases.
- Coordinate migration only: no new scores, psychological weights, directional meaning, tools or dependencies. Existing context-only LLM policy remains; add only minimal encoding/time explanation, not interpretive rules.

## 2. Locked invariants

- Preserve physical representation, top-to-bottom display, `flip_line(k)` changing array `[k-1]`, Xiantian trigram numbering, and mutual extraction. Unresolved Qian/Kun mutual variant remains unchanged.
- Actual inverse API is `Hexagram::from_binary_index`, not `from_u8`; Q decode must recover the same physical arrays.
- Preserve CST/UTC calendar policy, hourly cast boundaries, `[open,close)` ownership, ascending time, and 10/W5/M2 future horizon using one injected `as_of`.
- Preserve all 13 frame columns and opening aliases: exported `iching_moving_line` uses `moving_line_open`; terminal `moving_line` remains separate. Preserve five plotted pane-3 series, their existing formulas/styles, and forecast sidecar separation.
- LLM forecast continues terminal Original reconstruction via `energy + 31.5`; it must report Q, terminal energy and terminal moving line, not an opening alias.
- Encode each physical cast before OHLC extrema and mutual-mean aggregation. Reversing an already aggregated old coordinate is invalid.

## 3. Grounded boundaries

| Boundary | Existing paths / anchors |
|---|---|
| Codec → cast → signal | `src/ta/iching/{types,plum_blossom,signal}.rs`: `Hexagram::binary_index`, `Hexagram::from_binary_index`, `cast`, `compute_channels`, `HexagramEnergy::new` |
| Cast aggregation → forecast | `src/ta/iching/{trajectory,forecast}.rs`: `trajectory_at`, `fold_cast`, `iching_bar_trajectory`, `materialize_iching_forecast`, `rendered_pane3_values` |
| Computed-frame consumers | `bins/{cryptobot,telegrambot}/src/presentation.rs`, including `crossing_bar_exports_opening_moving_line` tests; preserve schema/order/null semantics |
| LLM text and tests | `bins/telegrambot/src/llm/mod.rs`: `FORECAST_HEADER`, `render_iching_forecast`, `iching_context_only_policy_in_all_prompt_sources`; existing column coverage in `bins/telegrambot/src/llm/tools.rs` |
| Minimal prompt synchronization | `bins/telegrambot/config/prompts/{system,system_alert}.txt` and `bins/telegrambot/k8s/prompts-configmap.yaml`; no deployment |
| Source/operator/reference wording | `src/ta/README.md`, `bins/{cryptobot,telegrambot}/README.md`, `bins/cryptobot/UX-SPEC.md`, `docs/prds/research/iching-datetime-market-cycles.md` |

- Current ownership: pure synchronous TA → application-owned projection → `ComputedFrame`; preserve dependency boundaries from `docs/architecture.md`, `docs/architecture/stream-and-duckdb-data-flow.md` and `docs/specs/computed-frame-contract.md`.
- Reference PRD already has authorized +34/-5 repair edits (main-session context, not independently diff-verified here). Preserve bibliography and new clarifications while amending implemented encoding/status; do not adopt its speculative metrics. Historical old-convention documents need at most a supersession note, not archive rewrites. No `.serena/core` update required.

## 4. Numbered strategic goals

### 4.1 Physical codec correctness — selected L1 goal

**Outcome:** one functional numeric-encoding migration inside pure TA, with compiled codec/cast/trajectory/forecast evidence; no consumer implementation or boundary-contract changes.

#### 4.1.a Physical fixture and inverse-codec lock — ATOMIC

- Writable boundary for later execution: `[MODIFY] src/ta/iching/types.rs` tests only; precedes 4.1.b. Establish independently defined physical arrays/trigram composition, named Q fixtures, all 64 inverse round trips and all 384 one-line flips before production migration.
- Preserve bottom-first arrays, top-to-bottom displays, Xiantian numbers, mutual extraction and validation. `Hexagram::from_binary_index` must decode Q into the original physical gua; round-trip equality alone is insufficient because paired wrong codecs can agree.
- Anchor existing tests: `hexagram_from_trigrams_composes_upper_lower`, `hexagram_binary_index_roundtrip_exhaustive_and_display_len`, `hexagram_flip_line_bottom_and_top`, `hexagram_mutual_mapping`. Fixed Jiji physical `[1,0,1,0,1,0]` becomes input Q42, with mutual Weiji Q21; keep its physical display unchanged. Generic index/energy validation cases are not named old-B gua fixtures and must not be blindly reversed.
- Acceptance: independent weighted physical sums match named Q fixtures; decoding recovers their arrays and display; every flip changes only physical position k−1 and twice restores the original; invalid indexes/lines retain validation behavior. Later TDD records expected red evidence against the old codec; this planning call runs nothing.

#### 4.1.b Core migration with compiled cast/trajectory proof — L2 PLANNED, U1

- Exactly one bounded functional unit: [U1 — Domain Q codec and core-fixture proof](phase_iching_quoted_encoding_01.md), incorporating 4.1.a as its internal RED prerequisite, not a separate delivery unit.
- Writable boundary for later execution: `[MODIFY] src/ta/iching/{types,plum_blossom,signal,trajectory,forecast}.rs` only, including co-located tests and encoding comments. Follow U1's preserved interface locks, independent physical/cast oracle and focused acceptance gates before later consumer work.
- Change the existing encode/decode semantics together; preserve names, types, nullability, physical display/flip/mutual algorithms, calendar policy, cast ownership and forecast/render formulas. Keep `HexagramEnergy::new` and signal validation at code minus 31.5, now receiving Q.
- Grounded flow already encodes physical Original, flipped Transformed and Mutual via `binary_index` in `cast`; `compute_channels` centers them; `trajectory_at` obtains the signal before `fold_cast` computes extrema/sums. Expect no production aggregation rewrite; prove this path rather than adding a second conversion layer.
- Update every fixed old-B fixture to the same physical gua, not a mechanically reversed collection of unrelated constants. `cast_min_fixture_matches_hand_computed` / `compute_channels_fixed_input_matches_manual_channels`: Li-over-Zhen (Shihe), B41→Q37, unchanged moving line 4. `cast_max_fixture_matches_hand_computed`: Kan-over-Dui (Jie), B19→Q50, unchanged moving line 6. Classical Ge Q46→Xian Q14 retains the physical bottom-line flip.
- `plum_signal_end_to_end_fixed_instant`: Yi Original B33→Q33 stays +1.5; physical line 6 produces Fu B1→Q32, energy +0.5; Mutual Kun stays Q0/−31.5. `crossing_bar_preserves_opening_and_terminal_cast_tuples`: opening Original Q33/Transformed Q32, terminal Original B34→Q17/Transformed B32→Q1; opening line 6 and terminal line 2 unchanged. Numeric flip assertions must reflect Q's physical line weights.
- Acceptance: compiled focused TA tests pass for all five files. Independently enumerate owned opening/hour/calendar casts, reconstruct their physical gua and sum Q without using the codec-under-test as the expected oracle; compare Original OHLC, Transformed opening/terminal, Mutual opening/terminal/extrema/mean and both moving lines. Include a real crossing interval whose old and Q ordering differs, plus off-grid open, exact-close exclusion and zero-length behavior.
- Forecast evidence: strengthen `materializer_matches_direct_trajectory` / `rendered_values_match_template_formula` with independently expected Q-derived values on real materialized bars; preserve input order, interval bounds, errors and pure timeline-free behavior. Direct equality to the same trajectory implementation alone does not prove correct encoding.
- Exit boundary: focused core-TA gate first. External bot/LLM hardcoded fixture updates and full-workspace gate belong to later selected goals; do not broaden this unit to make those pass.

**Remaining strategic goals — outcomes only:**

2. **Per-cast propagation:** establish Q-based Original/Transformed/Mutual throughout casting and real crossing-bar trajectories; independently sum physical-cast Q values at owned hour/calendar boundaries to prove extrema and mutual mean.
### 4.3 Consumer continuity — selected L1 goal

**Outcome:** downstream Q contract alignment in both bots and actual future LLM output, without changing presentation/formatter logic, historical opening versus future terminal semantics, schema or existing forecast behavior.

#### 4.3.a Downstream Q contract alignment — L2 PLANNED, U2

- Exactly one functional unit, dependent on implemented U1: [U2 — Numerical consumer fixtures and minimal header alignment](phase_iching_quoted_encoding_02.md). Follow its three-file/section scope, preserved actual signatures, separate focused RED evidence and complete offline package/workspace acceptance before later U3 work.
- Exact future writable boundary: `[MODIFY] bins/cryptobot/src/presentation.rs` and `[MODIFY] bins/telegrambot/src/presentation.rs` numeric expected test cases only; `[MODIFY] bins/telegrambot/src/llm/mod.rs` only `FORECAST_HEADER`, matching factual/header tests and actual-cast fixture expectations. No production presentation/formatter algorithm changes.
- Both existing `crossing_bar_exports_opening_moving_line` tests use 2024-02-10 00:00 UTC/H4. Keep opening Original Q33/E+1.5 and exported opening moving line 6. Align opening Transformed old B1/E−30.5 to Q32/E+0.5; terminal Original old B34/E+2.5 to Q17/E−14.5; terminal Transformed old B32/E+0.5 to Q1/E−30.5. These are unchanged physical gua/flips, not a core defect.
- `forecast_prompt_renders_actual_terminal_cast_and_offsets`: preserve real `materialize_iching_forecast` inputs and opening/terminal distinction; terminal expected code becomes Q17 and rendered first offset must contain `T+0 bin=17 orig=-14.5 ml=2`, not opening `bin=33 orig=+1.5 ml=6`. Keep subsequent chronological offsets and terminal reconstruction via `energy + 31.5`.
- Current header defines `bin=0..63`, `orig=bin-31.5`, terminal `ml` and scheduled T+ offsets but does not define Q weights or physical moving-line order. Add only minimal factual explanation: chosen Q bottom→top weights 32,16,8,4,2,1; Yang1/Yin0; physical moving line 1=bottom, 6=top; centered coordinate C=Q−31.5 (existing serialized `orig`), not strength or a validated financial score. Distinguish a within-cast physical transformation from a fresh future calendar cast. Keep `bin`/`orig`/`ml` names, two-line header, placeholder behavior, terminal ownership, T+0 meaning and all existing disclaimers.
- Preserve `FORECAST_BUDGET=320` and its existing UTF-8 byte/token heuristic for the body, excluding the fixed header; preserve default-timeframe priority, run compression, empty/omission behavior and rendering logic. Update `forecast_prompt_header_defines_calendar_tokens` to prove the added definitions without weakening existing factual assertions. No new psych scores, interpretation or context-only policy rules.
- Synthetic `forecast_bar(bin, ...)` fixtures are abstract numeric codec states, not old named physical casts: do not automatically reverse their constants (including synthetic bin34/+2.5 and bin19/−12.5). Keep bounds, budget, compression and placeholder tests intact.
- Acceptance for later execution: both crossing fixtures pass with the explicit same-physical Q tuples; actual-calendar LLM fixture emits the terminal Q text; header remains two lines and body/placeholder tests retain behavior. Preserve existing all-channel/schema/opening-alias, pane/sidecar and single-`as_of` horizon coverage. Then rerun relevant consumer suites and full workspace tests to discover remaining targets; no guessed overall PASS or broadened writable scope.
- Evidence supplied by main, not rerun here: U1 implemented exactly five TA files; independent tester reports 84 I-Ching tests PASS, algotrap 215 PASS and chartlib 29 PASS. Full workspace compiled, then fail-fast stopped at cryptobot 33 PASS/1 obsolete-fixture failure; subsequent targets were not executed. Telegram obsolete assertions are source-confirmed pending execution, not certified test failures.
- Boundary/obstacles: no new physical contradiction identified. Known old-B expected literals explain the current blocker. Source prompts, ConfigMap, operator/module READMEs and reference/UX docs remain goal 4.4/U3. L2 planning verified current fixtures/header/formatter contracts; presentation searches found no XOR/mask assertions. No commands or source edits ran. Next: one bounded independent U2 plan/acceptance review before main-authorized implementation, with at most one targeted correction and a final targeted evidence check; overall migration remains incomplete.

### 4.4 Minimal explanation alignment — selected L1 goal

**Outcome:** current operator/reference wording and both system Read-Guides describe implemented Q consistently, with minimal factual explanation only. User requests 390/391 approve quantizable quoted Q, not psychological interpretation or new prompt rules.

#### 4.4.a Current Q wording and prompt-source parity — L2 PLANNED, U3

- Exactly one docs-editor unit: [U3 — Documentation and prompt factual-coordinate alignment](phase_iching_quoted_encoding_03.md), dependent on completed U1/U2. Follow its eight-path/scoped-text locks, byte-preserved guards/user prompts, existing all-four parity tests and final tester handoff. Main reports actual workspace 609 tests PASS, zero failures/ignored; not rerun here. Current two-line `FORECAST_HEADER` was read and already explains Q, physical moving lines, coordinate-not-strength, half-open terminal ownership and future anchor offsets. It is implemented, not future work; no header/formatter/source-code write belongs to U3.
- Exact proposed future `[MODIFY]` paths/scopes:

| Path | Minimal purpose / writable scope |
|---|---|
| `src/ta/README.md` | I-Ching Coordinate/Structural dependence/Sign relationships plus numeric-compatibility explanation; replace stale bottom-LSB/top-line sign statements. Preserve channel table and API/calendar ownership. |
| `bins/cryptobot/README.md` | I-Ching pane section: explicitly name chosen Q and E=Q−31.5; preserve 13-column mappings, opening moving-line alias, plot formulas and horizon. |
| `bins/telegrambot/README.md` | I-Ching coordinate/forecast meanings: name Q; correct actual 2024-02-10 H4 terminal bin17/orig−14.5/ml2, opening Q33/ml6. Preserve context-only and citation-policy paragraphs verbatim, source-parity/no-deployment caveat. |
| `bins/cryptobot/UX-SPEC.md` | Existing pane coordinate paragraph only: identify centered Q encoding, not strength; unchanged range/time/pane styles/series/states. No UI redesign. |
| `docs/prds/research/iching-datetime-market-cycles.md` | Current-versus-proposed status notices, §2.1 Q adoption/formula/examples/table, current-production statements in §4.1 and precise implemented-minimal-header clarification. Preserve authorized repair, citations/ISBN-10 and proposed research. |
| `bins/telegrambot/config/prompts/system.txt` | Root I-Ching Read-Guide factual encoding/physical-line explanation only; all policy, tools, placeholders and text outside this guide byte-unchanged. |
| `bins/telegrambot/config/prompts/system_alert.txt` | Same scoped factual Read-Guide alignment; preserve mode-specific exclusions and all unrelated adaptive rules byte-unchanged. |
| `bins/telegrambot/k8s/prompts-configmap.yaml` | Mirror only corresponding Read-Guide edits in `system.txt`/`system_alert.txt` literal blocks. `user.txt`/`user_alert.txt` values, YAML metadata and every other block byte-unchanged; no apply/restart. |

- Factual lock: bottom-first storage and lower-trigram bottom-three composition remain unchanged; display remains top→bottom. `Q=32l1+16l2+8l3+4l4+2l5+l6`, Yang1/Yin0, E=Q−31.5. Original sign positive iff physical line1 Yang; Mutual positive iff Original line2 Yang (`mutual.lines[0]=original.lines[1]`); Transformed sign flips only for physical moving line1. These are per-cast encoding properties, not strength/market statements; aggregate mean may be zero even though individual cast energies are nonzero half-integers.
- Named evidence: Bo1/Fu32/Jie50/Tai56/Pi7; Ge46→Xian14 is the physical moving-line1 transformation, not recoding B46. Legacy B is a historical counterpart, never the current contract or a second DTO. No new universal-book-authority or validated-energy/price claim.
- PRD currently has stale blanket “no implementation” notices and “Q not adopted”/E=B claims despite its 3-Oct clarification. Amend only current implementation/adoption descriptors; retain historical proposals as proposed. Distinguish implemented Plum Blossom/Q/minimal future-header explanation from unadopted record/method/weighted-Yang/psychological hypotheses and future interpretation research. Do not characterize the already updated Q header as unimplemented, or adopt experimental formulas as positional bit identification. Preserve Liu ISBN-10 `0060616687`, corrected catalog evidence, primary passages, physical flip/mutual variant clarification and research caveats; no bibliography re-research or speculative metric rewrite.
- Both Read-Guides need only a concise chosen-Q weights/polarity/physical-line explanation, retaining `binary index minus 31.5` (existing policy-test clause). Keep within-cast Transformed versus fresh future cast and opening alias versus terminal ml. No new confidence/risk framework, scores, tuning or interpretation; existing context-only exclusions and SAME-sentence LIVE guardrails remain verbatim, including policy bullets within the guide.
- Existing tests are read-only acceptance anchors: `iching_context_only_policy_in_all_prompt_sources`, `test_active_adaptive_prompt_sources_match_configmap_literal_blocks_byte_for_byte`, and updated `forecast_prompt_header_defines_calendar_tokens`. No test/source write required if factual additions preserve existing clauses. Later proof compares all four decoded ConfigMap values with source prompts and proves only allowed factual spans changed.
- Targeted active-doc search found old B conventions in `docs/plans/iching-energy-chart/{l0,l1}.md` and old phase specs. `docs/README.md` explicitly routes this plan as Planned/proposed, not a live contract; l0's seven-bit proposal and l1's historical normative wording are not current source authority. Exclude these execution/history files and `docs/archive/` from U3 rewrites; record the stale proposal finding. If main requires navigation disambiguation later, request a separately scoped named supersession note rather than rewriting archived plans or adopting their APIs. No new doc shards or `.serena/core` edit.
- Acceptance for later execution: current operator/PRD production text consistently uses Q; correct named values/sign relationships; no stale “Q unadopted” or “minimal Q header unimplemented” live claim; experimental research clearly unadopted. Verify byte-preserved policy/guardrail/user-prompt content, all four source/ConfigMap value parity, unchanged tested header, and rerun existing focused policy/parity plus workspace gates. No deployment or overall-completion claim before U3 verification.
- L2 authored the single U3 phase and verified the existing all-four literal-block parity test plus both read-only user prompts. No new material code contradiction identified; obstacle is stale factual documentation, not missing feature implementation. Next: main-authorized docs-editor execution after one bounded review and at most one targeted correction; final independent tester confirms workspace status. Planning writes only this parent and U3 phase; no tests/builds/network/Git commands or source/prompt/docs edits ran.

## 5. Evidence and next selection

- Named fixture expectations (old B → Q): Bo 32→1; Fu 1→32; Jie 19→50; Tai 7→56; Pi 56→7; Jiji 21→42; Weiji 42→21; Kun 0 and Qian 63 unchanged.
- Classical Ge→Xian is a physical transformation: `[1,0,1,1,1,0]` Q46/B29 → moving physical line 1 → `[0,0,1,1,1,0]` Q14/B28. **46→14 is Q Original→Q Transformed, not B46→Q14.** Primary 梅花易數 卷一觀梅占 and 卷三變卦式八則 support the physical change (accepted main-session evidence; no web research here).
- Later proof must include actual computed future LLM output with minimal text, not only fabricated forecast rows or snapshots. Historical 603-pass PR b97e baseline is context, not current verification; no tests/builds run for this L0 outline.
- L1 refinement re-read bounded current TA source/tests; confirmed actual codec and cast/fold API names without introducing any new `ComputedFrame` claim. No new material contradiction identified; no tests/builds run. External old-B expectations are deferred, not a blocker to a later focused core-TA gate.
- L2 authored exactly one [U1 phase](phase_iching_quoted_encoding_01.md) for 4.1.b plus 4.1.a; actual signatures/type shapes verified in current TA source and calendar input adapter read-only. No implementation or verification commands run; other goals remain L0 outcomes.
- Next: main authorizes U1 execution after one bounded independent plan/acceptance review and at most one targeted correction. Focused TA RED/GREEN, workspace compile, formatting and diff gates are required; consumer fixture/text/docs units and final workspace validation remain deferred, so do not call the overall migration complete. Planning writes only this parent and U1 phase; no source/prompt/docs edits, Git mutation, cluster/browser/auth access, memory-core writes or changes to review server 18800. Stop on permission denial, unavailable approval or new material contradiction.

## 6. Completion and handoff — 2026-10-04

- **DONE — U1/U2/U3 implemented-verified.** This execution checkpoint supersedes pending/next-action wording in the preserved historical plan above; original unit contracts and scopes remain intact. Evidence supplied by main; no verification rerun by this capture.
- U1: paired Q encoder/inverse (physical bottom weight32, top1, Yang1/Yin0), E=Q−31.5 and six new independent TA tests complete. Physical flip, mutual extraction and calendar unchanged. U2: actual terminal `bin=17 orig=-14.5 ml=2` and minimal two-line header aligned. U3: eight factual document/prompt paths aligned; no new model/framework or policy-guard changes. Local external numeric meaning intentionally changed; API shapes unchanged; no guaranteed psychological/financial prediction.
- Independent final tester `ses_efc9c3b7fffeq7kOdJ8AA6PRKA` actually ran fmt/check/build, full workspace/all-target locked-offline strict test gate, strict Clippy `-D warnings`, `CARGO_NET_OFFLINE=true make duckdb-test`, and diff check: **all exit 0**. Both test gates passed **609 = 215+29+34+9+306+13+3+0**, zero failed/ignored/filtered. Verification took 19.75 seconds using cached artifacts; no live/container/LLM/native image builds. Nonblocking `proc-macro-error2` future-compatibility notice remains.
- Independent implementation reviewer `ses_efca88215ffePyLKH2s2OKBxR5`: **APPROVE** all U1/U2/U3 source/test semantics and protected source/YAML policy.
- Completed migration paths (16, untouched by this checkpoint): `src/ta/iching/types.rs`, `src/ta/iching/plum_blossom.rs`, `src/ta/iching/signal.rs`, `src/ta/iching/trajectory.rs`, `src/ta/iching/forecast.rs`; `bins/cryptobot/src/presentation.rs`, `bins/telegrambot/src/presentation.rs`, `bins/telegrambot/src/llm/mod.rs`; `src/ta/README.md`, `bins/cryptobot/README.md`, `bins/telegrambot/README.md`, `bins/cryptobot/UX-SPEC.md`, `docs/prds/research/iching-datetime-market-cycles.md`, `bins/telegrambot/config/prompts/system.txt`, `bins/telegrambot/config/prompts/system_alert.txt`, `bins/telegrambot/k8s/prompts-configmap.yaml`.
- Handoff remains **local and uncommitted** on `feat/iching-calendar-horizon`; no stage/commit/push/PR update/deploy authorized or performed. Old PR head `b97e68730b98c92e5fbe7897efabf0067cdb6f77` unaffected. Historical planned/archive B documents remain outside the eight-current-document scope and were not retroactively rewritten. Further Git/publication/deployment action requires separate main/user authority.
- Capture writes only this parent and the three existing phase memories; no source edits, commands/tests, new files, core/long-term promotion, consolidation or archive action.
