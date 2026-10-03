---
kind: phase
topic: iching-quoted-encoding
unit: U3
depth: L2
status: implemented-verified
created: 2026-10-04
updated: 2026-10-04
parent: plan_iching_quoted_encoding.md
target: "4.4.a minimal Q explanation alignment"
depends_on: [U1, U2]
executor: docs-editor
authority: main-session
memory_backend: serena
---

# U3 — Documentation and prompt factual-coordinate alignment

## 1. Delivery contract and exact file scope

**Outcome:** human operator/reference text and both system Read-Guides accurately describe the implemented local Q encoding, with source/ConfigMap parity and no new interpretation or policy.

| Contract | Lock |
|---|---|
| Approved objective | User 390/391 requested quantizable quoted Q and minimal explanation, not psychological/directional frameworks. Integer encoding changed; physical gua, calendar and existing energy-field shapes did not. |
| Must ship | Five scoped document updates; two factual system Read-Guide updates and exactly their two ConfigMap copies; correct sign/examples/adoption status; preserve repaired provenance, policies and all four prompt-value parity. |
| Never defer | Stale current Q-unadopted/LSB claim, incorrect sign or actual date example, weakened guards, changed user prompt, source/YAML drift, research presented as adopted strength model. |
| May remain | Clearly labeled legacy B comparisons and unadopted research proposals; historical/planned execution documents outside these eight paths. |
| Authority/review | Main authorizes execution and acceptance. One bounded independent plan/scope review, at most one targeted correction; unresolved material issues return to main. |
| Status | Main reports U1/U2 workspace 609 PASS, zero failures/ignored. Current Q future header is implemented and read-verified. This is local uncommitted-source alignment, not deployment, commit, PR or remote status certification. |

Exactly one docs-editor functional unit. Plaintext/Markdown/YAML edits only; **no Rust production or test writes**. Code interfaces: N/A to editing; all interfaces/DTOs/fields/signatures/panes/calendar/policies remain unchanged.

| Operation | Exact future writable file | Allowed scope |
|---|---|---|
| [MODIFY] | `src/ta/README.md` | I-Ching trajectory section's Coordinate, Structural dependence, Sign relationships and narrow numeric-compatibility explanation. Preserve channel table/API note and all other sections. |
| [MODIFY] | `bins/cryptobot/README.md` | I-Ching Energy pane section: concise chosen-Q coordinate/compatibility wording only; retain mappings, formulas and horizon. |
| [MODIFY] | `bins/telegrambot/README.md` | I-Ching factual coordinate/forecast meanings and actual date example only. Context-only/citation-policy/source-parity caveats remain verbatim. |
| [MODIFY] | `bins/cryptobot/UX-SPEC.md` | Numeric-semantic paragraph in §3 only: identify centered Q, not strength. No pane/layout/style/state/series changes. |
| [MODIFY] | `docs/prds/research/iching-datetime-market-cycles.md` | Narrow current/proposed status notices; §2.1 adopted Q/formula/examples/comparison; §4.1 production-versus-experiment and implemented minimal-header clarification, including scoped binary percentage wording. Preserve prior 3-Oct source clarifications and bibliography. |
| [MODIFY] | `bins/telegrambot/config/prompts/system.txt` | Only factual encoder/physical-line clarification in first two Root I-Ching Read-Guide bullets. |
| [MODIFY] | `bins/telegrambot/config/prompts/system_alert.txt` | Same first-two-bullet factual span; no adaptive/policy/guard changes. |
| [MODIFY] | `bins/telegrambot/k8s/prompts-configmap.yaml` | Only the corresponding two factual spans in `data.system.txt` and `data.system_alert.txt` literal blocks. Metadata and both user blocks untouched. |

Out of scope: all other files, planned/archive rewrites, new doc shards, `.serena/core`, U1/U2 code/test edits, whole-page prompt regeneration, pervasive formatting, new activation/personality/scoring layers, dependencies/build infrastructure, models/research requests, new tools, staging/commit/PR/push/deploy, cluster/browser/auth/network actions. No apply/rollout; manifest comments are not execution instructions. Planning writes only this phase and its parent link.

## 2. Locked human and LLM text contracts

### Coordinate, physical structure and intentional compatibility change

- Bottom-first physical array stays `line[0]=line1` bottom through `line[5]=line6` top. Yang=1, Yin=0. Lower trigram remains the first/bottom three physical lines; display remains top→bottom `line[5]..line[0]`.
- Adopted positional integer: `Q=32l1+16l2+8l3+4l4+2l5+l6`, Q∈0..63. This is the selected quoted software encoding, not King Wen numbering or a universally established book rule. `E=Q−31.5` remains the centered encoding coordinate, not market direction or validated energy/psychological strength.
- External numeric semantics intentionally changed with user approval. Preserve API shapes/names/nullability, not old numeric values; no dual DTO/backward codec. Do not claim all integers changed: endpoints and symmetric encodings can remain equal. Physical casts/flips/trigram numbering/mutual extraction/calendar did not change.
- `flip_line(k)` still toggles physical array `[k−1]` within the same cast, not a new calendar cast. If a numeric mask is needed in documentation, Q uses `2^(6−k)`, never bit k−1. No need for extra mask pseudocode in LLM text.
- Existing unconditional Mutual remains unchanged, including Qian/Kun stability. Preserve the unresolved classical variant notice; do not introduce a policy exception.

### Correct per-cast sign and date facts

- Original positive iff physical bottom line1 Yang (Q≥32); negative iff Yin. Individual cast E is a nonzero half-integer; this does not imply aggregate mean cannot be zero.
- Mutual positive iff Original physical line2 Yang because `mutual.lines[0]=original.lines[1]`.
- Transformed sign differs only when physical moving line1 flips the weight32 bit. These are coordinate properties, not bullish/bearish confirmations.
- Named Q: Bo1, Fu32, Jie50, Tai56, Pi7; Kun0/Qian63 unchanged. If retained, Da Zhuang physical `[1,1,1,1,0,0]` has Q60, display `001111`, legacy B15.
- Actual 2024-02-10 00:00–04:00 UTC: opening Original Q33/E+1.5/ml6 → opening Transformed Q32/E+0.5; terminal Original Q17/E−14.5/ml2 → terminal Transformed Q1/E−30.5. Historical `iching_moving_line` remains opening; forecast `ml` remains terminal.
- Ge `[1,0,1,1,1,0]` Q46 → physical line1 → Xian `[0,0,1,1,1,0]` Q14 is same-cast transformation, not B46 recoding. Keep physical text/citations; do not use a King Wen mapping to explain these numbers.
- Maintain `[open,close)` last-owned-cast terminal semantics, ascending calendar time, T+0 after observed anchor open, 10/W5/M2 horizons and single `as_of`. No calendar behavior edits.

### Minimal system Read-Guide insertion contract

Both first factual bullets retain their current sentences/clauses, including literal `binary index minus 31.5` needed by existing policy tests. Add only a concise explanation equivalent to:

> The binary index uses chosen Q: physical bottom-to-top weights 32,16,8,4,2,1 (Yang=1, Yin=0), not King Wen numbering; energy is an encoding coordinate, not strength. Physical moving line 1 is bottom and 6 is top; the within-cast transformation is not a fresh future calendar cast.

This is a factual text contract, not permission to replace the guide with a new framework. Integrate minimally into the existing first/second bullets; preserve current same-cast structural dependence, opening alias versus terminal ml, existing financial caveats and `not a future market state` text. Do not add sign/strength interpretations or new confidence/risk commands to prompts; detailed sign facts belong in human TA documentation.

Byte-preservation locks:

- All source text outside those two factual bullets unchanged; other Read-Guide bullets—including tools, context-only exclusions and precedence—unchanged.
- Both forward-window SAME-sentence specific LIVE-column/numeric-value guardrails and all market-only/attribution rules unchanged verbatim.
- `config/prompts/user.txt` and `config/prompts/user_alert.txt` are **read-only**, byte-unchanged, including terminal newlines; corresponding YAML literal values byte-unchanged.
- All existing placeholder values, adaptive confidence/weights/outcome/timeframe rules and seven-tool registry unchanged. No whole-page normalization or reserialization.
- Current two-line `FORECAST_HEADER` in `bins/telegrambot/src/llm/mod.rs` already satisfies Q/physical/time explanation. It and every Rust test are read-only during U3.

### ConfigMap literal-data contract

Preserve Kubernetes `apiVersion`, `kind`, metadata, labels, `data` keys, indentation and block chomping. Four literal values continue matching their source files:

| YAML `data` key | Source path | U3 status |
|---|---|---|
| `system.txt` | `bins/telegrambot/config/prompts/system.txt` | Same two factual bullet edits copied exactly |
| `system_alert.txt` | `bins/telegrambot/config/prompts/system_alert.txt` | Same two factual bullet edits copied exactly |
| `user.txt` | `bins/telegrambot/config/prompts/user.txt` | Unchanged bytes/value |
| `user_alert.txt` | `bins/telegrambot/config/prompts/user_alert.txt` | Unchanged bytes/value |

## 3. Documentation-only execution sequence

1. Before editing, record the eight-path working-copy baseline and protected spans; preserve pre-existing authorized PRD +34/−5 repair from main context. Existing dirty changes are not permission to overwrite the page. No temporary scripts/new files needed.
2. Correct TA coordinate/sign statements and add narrowly scoped approved numeric-semantic compatibility wording. Add short encoding identification to cryptobot README/UX paragraph, preserving all schema/formulas/styles. Correct Telegram current terminal example without touching policy paragraphs.
3. Repair PRD's blanket stale notices by distinguishing **current local implemented Plum Blossom/Q channels and minimal future-header explanation** from proposed DTOs/additional methods/research stages. Do not mark the whole research program implemented or assert any deployment/remote update. Keep prior 3-Oct status/source explanation, primary passages, mutual variant, one-cast clarification and bibliographic correction intact; add an explicit current Q-adoption update rather than rewriting their evidence history.
4. In §2.1 make Q the current formula and B explicitly legacy comparison; keep physical storage/display/trigram statements unchanged. In §4.1 correct current `E=B−31.5` claim to E=Q−31.5; retain the historical source-review statement as dated evidence about the then-used B formula, clearly separated from current convention. Correct “future minimal LLM explanation not implemented” implication: minimal Q explanation is implemented; proposed psychological interpretation remains deferred and existing context-only policy remains in force.
5. Where §4.1 names the binary percentage proposal, use `100*Q/63` if describing chosen-current-code presentation, or retain B only as explicit legacy comparison. Either way percentage remains an **unadopted presentation proposal**, not a metric being shipped. Do not change provisional Yang-count/weighted-polarity formulas/weights or introduce an activation layer.
6. Preserve Da Liu ISBN-10 `0060616687`, mismatched-ISBN/catalog correction, primary text and validation caveats verbatim. Public Wikisource transcriptions support cited physical/classical statements, not universal-book provenance for chosen integer weights or market prediction. No web research or new bibliographic assertions.
7. Add only the factual Q/physical-order explanation to the two source Read-Guides, then mirror exactly those edits into their YAML blocks. Leave both user files/blocks and every guard byte-unchanged.
8. Perform targeted document/prompt acceptance below, one bounded review and at most one targeted correction. Any out-of-scope need returns to main. No broad formatter pass, tests edited to pass, new machine build requirement or runtime change.

## 4. Acceptance and proof

Commands below are requirements for later authorized execution; **none ran during planning**.

```sh
cargo test --locked --offline -p telegrambot iching_
cargo test --locked --offline -p telegrambot test_active_adaptive_prompt_
cargo test --locked --offline -p telegrambot forecast_prompt_header_defines_calendar_tokens
git --no-optional-locks diff --check
```

- Record real executed/matched test counts and results for each filter; zero matching tests is not success. Existing anchors include `iching_context_only_policy_in_all_prompt_sources`, `test_active_adaptive_prompt_sources_match_configmap_literal_blocks_byte_for_byte` (loops over all four files) and adaptive-contract rules. Do not edit these assertions. Header test is already GREEN from U2 and must remain so; no header change.
- Before/after comparison proves two user source files and YAML literal values byte-unchanged; system text outside the two factual spans unchanged; context-only/citation/policy paragraphs unchanged. Parity alone cannot prove guards were preserved if both copies were changed, so verify protected baselines separately.
- Check all touched local relative links against existing paths; keep provenance URLs/citations unchanged without network fetches. Scan changed live text for stale current bottom-LSB/top-sign/Q-not-adopted/header-unimplemented claims. Explicitly labeled historical B/research claims are allowed; do not make a simplistic global `B` replacement.
- Confirm current documents name Q/centering/non-strength, correct sign triplet, named examples and actual terminal tuple; no API/calendar/schema/style/policy claim changed. All experimental metric proposals remain unadopted. No inferred deployment, universal textual rule or market-validation claim.
- No new full build required for this plaintext/YAML unit. Final independent tester later reruns the whole workspace suite (main's prior baseline: 609 PASS, zero failed/ignored); report actual counts/results, never reuse prior PASS as U3 certification. Any focused failure or parity drift blocks U3 acceptance.

## 5. Handoff and stop rules

- Delivery lists exact eight changed paths/spans, baseline protection evidence, factual/sign/date checks, all-four literal parity, real filter counts, diff/link checks and review disposition. Mention local uncommitted alignment only; commit/PR/deployment state is unchanged and outside authority.
- Stop on denied tool/path, unavailable approval, material contradiction, missing offline dependency, protected-span drift or required write beyond these eight paths. Return INCOMPLETE with exact obstruction, completed work and next safe main action; never seek permission through another tool or silently broaden scope.
- Planned/history documents `docs/plans/iching-energy-chart/**` and archives remain out of scope. Their stale B proposal is recorded in the parent; no new doc shards or history rewrite. Any separately desired supersession note needs a separate main-scoped decision.
- Main owns final migration acceptance after U3 factual/policy parity proof and final tester confirmation. Do not call the overall migration complete while current operator/reference wording remains inconsistent.

## Acceptance checkpoint — 2026-10-04

- **U3 implemented-verified:** eight scoped factual document/prompt paths aligned with current Q; no new model/framework or policy-guard changes. Protected source/YAML policy approved; historical planned/archive B documents remain outside scope. Original contracts/protected spans and dated evidence history preserved.
- Main-supplied independent reviewer `ses_efca88215ffePyLKH2s2OKBxR5`: **APPROVE** all U1/U2/U3 source/test semantics and protected source/YAML policy. Final tester `ses_efc9c3b7fffeq7kOdJ8AA6PRKA`: fmt/check/build, locked-offline workspace/all-target strict tests (609, zero failed/ignored/filtered), strict Clippy `-D warnings`, offline DuckDB tests (609 again), diff check all exit0. Final validation complete; see parent completion/handoff for exact path list, cached-run caveat and unchanged remote state.
- This capture records evidence without rerunning or editing source/docs/prompts. Migration remains local/uncommitted; publication/deployment not authorized/performed. No promotion/consolidation/core/archive action.
