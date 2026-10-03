---
kind: phase
topic: iching-quoted-encoding
unit: U1
depth: L2
status: implemented-verified
created: 2026-10-04
updated: 2026-10-04
parent: plan_iching_quoted_encoding.md
target: "4.1.b coordinated core-TA migration, including 4.1.a TDD prerequisite"
authority: main-session
memory_backend: serena
---

# U1 — Domain Q codec and core-fixture proof

## 1. Delivery contract and scope

**Outcome:** every existing pure-TA cast uses the approved Q integer coordinate and E=Q−31.5, while physical gua, inverse decoding, cast ownership and aggregation remain correct.

| Contract | Lock |
|---|---|
| Objective | Approved bottom-first physical array; Q weights [32,16,8,4,2,1]; Yang=1, Yin=0. Intentional externally visible numeric migration only. |
| Must ship | Paired codec/inverse migration; named physical fixtures; all 64 physical arrays and 384 physical flips; compiled classical cast and fixed-date signal proof; independent per-cast aggregate oracle; real forecast/render proof; RED then GREEN evidence. |
| Never defer | Wrong physical flip, inverse, Transformed, Mutual, or aggregate coordinates; any focused TA failure; validation/nullability regressions. |
| May defer | Already-planned bot/LLM numeric fixtures, minimal text/prompts and documentation work until later consumer units and final full-workspace gate. U1 success is not overall migration completion. |
| Authority | Main session owns implementation authorization, scope exceptions and final delivery acceptance. |
| Review | One bounded independent review of this plan/acceptance coverage, then at most one targeted correction pass; unresolved material findings return to main. Operator review metadata stays separate from the domain contract. No model/psychology research required. |

Exactly one functional unit. TDD steps below are internal sequencing, not additional units or phase files.

### Future code file-operation matrix

| Operation | Exact writable file | Touched boundary |
|---|---|---|
| [MODIFY] | `src/ta/iching/types.rs` | Existing encode/decode bodies and encoding comments; co-located physical/codec/flip/mutual tests. |
| [MODIFY] | `src/ta/iching/plum_blossom.rs` | Co-located fixed cast/channel fixtures, classical fixture, encoding comments; existing cast flow preserved. |
| [MODIFY] | `src/ta/iching/signal.rs` | Co-located physical fixed-date/channel proof and encoding comments; validation and facade behavior preserved. |
| [MODIFY] | `src/ta/iching/trajectory.rs` | Co-located tuple/mask fixtures and independent cast-aggregation oracle; no production fold rewrite expected. |
| [MODIFY] | `src/ta/iching/forecast.rs` | Co-located real materialization/render oracle assertions; production formulas/order/errors preserved. |

No future code writes outside these five files. Read-only calendar source may ground test inputs; `calendar.rs` is not writable. No new DTO/config/policy function or field, dependency, build infrastructure, async/clock behavior, scoring/directional meaning, consumer implementation, prompt/docs/archive edit, commit/push/PR/cluster/browser action. No unapproved external scratch script. This planning invocation writes only this phase file and its parent plan link.

## 2. Locked current contracts

Source verified in the five files above. Preserve all existing visibility, names, signatures, derives, fields and nullability; only `binary_index` semantics and its `from_binary_index` inverse intentionally change. No `from_u8` API exists or is to be added.

### Type shapes — preserve verbatim (comments omitted)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hexagram { lines: [u8; 6] }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trigram { lines: [u8; 3] }
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HexagramEnergy {
    pub hexagram: u8,
    pub energy: f64,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IchingSignal {
    pub original: HexagramEnergy,
    pub transformed: Option<HexagramEnergy>,
    pub mutual: HexagramEnergy,
    pub moving_line: Option<u8>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LeapMonthPolicy {
    #[default]
    Reject,
    Allow,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlumBlossomInput {
    pub year_branch: u8,
    pub lunar_month: u8,
    pub lunar_day: u8,
    pub hour_branch: u8,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlumBlossomRecord {
    pub hexagram: u8,
    pub moving_line: u8,
    pub transformed: u8,
    pub mutual: u8,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PlumBlossomResult {
    pub original: HexagramEnergy,
    pub transformed: HexagramEnergy,
    pub mutual: HexagramEnergy,
    pub moving_line: u8,
}
#[derive(Debug, Clone, PartialEq)]
pub struct IchingBarTrajectory {
    pub energy_open: f64,
    pub energy_high: f64,
    pub energy_low: f64,
    pub energy_close: f64,
    pub transformed_open: f64,
    pub transformed_close: f64,
    pub mutual_open: f64,
    pub mutual_close: f64,
    pub mutual_high: f64,
    pub mutual_low: f64,
    pub mutual_mean: f64,
    pub moving_line_open: Option<u8>,
    pub moving_line: Option<u8>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct IchingForecastBar {
    pub bar_open_ms: i64,
    pub bar_close_ms: i64,
    pub trajectory: IchingBarTrajectory,
}
#[derive(Debug, Clone, PartialEq)]
pub struct IchingRenderedPane3Values {
    pub original: f64,
    pub transformed: f64,
    pub mutual_high: f64,
    pub mutual_low: f64,
    pub mutual_mean: f64,
}
```

### Existing signatures — declaration excerpts, not new interfaces

```rust
// Hexagram methods
pub fn from_trigrams(upper: Trigram, lower: Trigram) -> Self;
pub fn binary_index(self) -> u8;
pub fn bits_top_to_bottom(self) -> String;
pub fn from_binary_index(b: u8) -> crate::ta::TaResult<Self>;
pub fn flip_line(self, line: u8) -> crate::ta::TaResult<Self>;
pub fn mutual(self) -> Self;
// HexagramEnergy method
pub fn new(hexagram: u8) -> crate::ta::TaResult<Self>;
// plum_blossom.rs
pub fn cast(input: PlumBlossomInput) -> crate::ta::TaResult<PlumBlossomRecord>;
pub fn compute_channels(input: PlumBlossomInput) -> crate::ta::TaResult<PlumBlossomResult>;
// signal.rs (DateTime/Utc and TaResult are existing imports)
pub(crate) fn build_signal(original: HexagramEnergy,
    transformed: Option<HexagramEnergy>, mutual: HexagramEnergy,
    moving_line: Option<u8>) -> TaResult<IchingSignal>;
pub fn plum_blossom_signal(datetime: DateTime<Utc>) -> TaResult<IchingSignal>;
pub fn plum_blossom_signal_with_policy(datetime: DateTime<Utc>,
    policy: LeapMonthPolicy) -> TaResult<IchingSignal>;
// trajectory.rs
pub fn iching_bar_trajectory(bar_open_time_ms: i64,
    bar_close_time_ms: i64) -> TaResult<IchingBarTrajectory>;
// forecast.rs
pub fn materialize_iching_forecast(bars: &[(i64, i64)]) -> TaResult<Vec<IchingForecastBar>>;
// IchingForecastBar method
pub fn rendered_pane3_values(&self) -> IchingRenderedPane3Values;
```

- Trigram APIs remain unchanged: `from_num(n: u8)`, `from_num_mod8(s: u32)`, `from_lines(lines: [u8; 3])` return `crate::ta::TaResult<Self>`; `lines(self) -> [u8; 3]`, `display(self) -> String`.
- Physical `lines[0]` remains bottom; `from_trigrams` lower-before-upper concatenation, display `lines[5]..lines[0]`, and `flip_line(k)` array `[k−1]` remain unchanged. Q decoding assigns Q's highest bit to the bottom physical line, not to the top.
- `mutual` stays `[l[1],l[2],l[3],l[2],l[3],l[4]]`, including existing Qian/Kun stability; no alternative mutual policy.
- `HexagramEnergy::new` still validates 0..=63 and subtracts 31.5; `build_signal`/`validate_channel` keep validation and all `None` cases. Preserve `TIME_POLICY_ID`, leap/month/year/hour boundaries and error behavior.
- Actual flow: `cast` encodes each physical channel → `compute_channels` centers Q → facade → `trajectory_at` → `fold_cast`. Extrema and mutual mean operate on already encoded casts. No conversion after aggregation and no need for a production aggregation subsystem.

## 3. Internal TDD sequence

### A. RED — physical and numeric expectations before codec changes

1. In existing co-located tests, define independent physical fixtures and a test-only weighted-sum reference using literal weights `[32,16,8,4,2,1]`. Expected values must not call production `binary_index`, `from_binary_index`, `cast`, `compute_channels`, signal energies or trajectory folding.
2. Lock named gua, inverse arrays and unchanged physical display. Enumerate all 64 physical arrays independently of production decoding; assert encoding matches the reference, Q decode recovers that exact array, and display matches reversed physical array. For all six positions per array, independently toggle `[k−1]`, check only that physical line changed, compare reference Q, and double-flip restoration. This covers 384 physical flips; numeric mask is weight `2^(6−k)`.
3. Amend existing same-physical-gua fixtures and add compiled classical/date/aggregation/forecast assertions below. Do not merely compare two production paths or reverse every constant: generic numeric validation/midpoint cases remain numeric cases.
4. Run the focused TA gate against unchanged old codec and retain actual named-fixture assertion failures as RED evidence. Compilation errors, missing dependencies, or a hypothetical expected failure are not RED certification.

### B. Minimal paired migration

5. Change only the existing encoder/inverse bit placement and encoding comments. Keep physical arrays, display, trigram/flip/mutual logic, shape, errors and energy formula unchanged.
6. Complete same-physical fixture updates across the five files; preserve and strengthen existing branch/calendar/leap/year/trigram/mutual tests. Never delete, ignore or weaken them to obtain GREEN. No second encoding layer in cast/signal/fold.

### C. GREEN — real cast, aggregation and forecast evidence

7. Run focused TA tests; investigate any failure inside scope. Prove the independent reference matches every channel and aggregate before declaring U1 done.
8. Run workspace compile, formatting and diff gates. External bot/LLM old-numeric test failures remain deferred to later U2/consumer selection, not permission to write outside scope or claim overall completion.

## 4. Required fixture and oracle acceptance

### Named physical fixtures

Arrays are bottom→top; B is historical identification only, not an alternate production codec or King Wen mapping.

| Gua | Physical array | Old B | Required Q |
|---|---|---:|---:|
| Kun | `[0,0,0,0,0,0]` | 0 | 0 |
| Bo / Bác | `[0,0,0,0,0,1]` | 32 | 1 |
| Fu / Phục | `[1,0,0,0,0,0]` | 1 | 32 |
| Jie / Tiết | `[1,1,0,0,1,0]` | 19 | 50 |
| Qian | `[1,1,1,1,1,1]` | 63 | 63 |
| Tai | `[1,1,1,0,0,0]` | 7 | 56 |
| Pi | `[0,0,0,1,1,1]` | 56 | 7 |
| Jiji | `[1,0,1,0,1,0]` | 21 | 42 |
| Weiji | `[0,1,0,1,0,1]` | 42 | 21 |

- Existing `hexagram_mutual_mapping`: Jiji input must decode Q42 into its original physical array; Mutual Weiji is Q21 and display remains `101010`.
- Existing min cast/channel fixtures: Li-over-Zhen Shihe `[1,0,0,1,0,1]`, B41→Q37; moving line 4 unchanged. Existing max fixture: Kan-over-Dui Jie, B19→Q50; moving line 6 unchanged. Derive and assert transformed/mutual physical arrays independently for both, not just Original constants.
- Classical input `PlumBlossomInput { year_branch: 5, lunar_month: 12, lunar_day: 17, hour_branch: 9 }`: S1=34, S2=43; upper Dui/lower Li; moving physical line 1. Ge `[1,0,1,1,1,0]` Q46 → Xian `[0,0,1,1,1,0]` Q14. Mutual lower Xun/upper Qian `[0,1,1,1,1,1]` Q31. Assert all channels and energies 14.5, −17.5, −0.5 through compiled `cast`/`compute_channels`. **46→14 is Q Original→Q Transformed, never B46→Q14.**
- Existing `plum_signal_end_to_end_fixed_instant`: preserve physical Yi Original Q33/+1.5, moving line 6; Transformed Fu Q32/+0.5 and Mutual Kun Q0/−31.5.

### Independent per-cast aggregation oracle

- Use real interval 2024-02-10 00:00–04:00 UTC and keep all owned cast observations, not only distinct numeric states. Assert opening Q33/ml6 → Transformed Q32; terminal Q17/ml2 → Transformed Q1. Existing old-terminal B34→B32 is the same physical transformation. This interval supplies changed ordering: old Original 33<34, new Q33>Q17; reversing old extrema individually would give the wrong Q extrema.
- Test oracle independently enumerates the opening plus hourly instants strictly before close, selecting CST midnight and odd CST hours. It may use current read-only `calendar::to_plum_blossom_input(datetime, LeapMonthPolicy::Allow)` to obtain calendar inputs: independence is required from the codec/cast/aggregation, not a duplicate lunar-calendar implementation.
- From each input, compute S1/S2 and moving line in test code; reconstruct raw arrays using hardcoded Xiantian patterns: 1 `[1,1,1]`, 2 `[1,1,0]`, 3 `[1,0,1]`, 4 `[1,0,0]`, 5 `[0,1,1]`, 6 `[0,1,0]`, 7 `[0,0,1]`, 8 `[0,0,0]`, modulo zero→8. Compose lower then upper, toggle physical `[ml−1]`, and extract mutual middle lines in the oracle. Do not invoke production trigram/hexagram/cast transformations for expected arrays.
- Apply literal Q weighted sums to every raw channel array, then center each observation. Independently compute Original open/high/low/close; Transformed opening/terminal; Mutual opening/terminal/high/low/mean and both moving lines. Mean is sum of per-cast Q divided by actual cast count, minus 31.5, not reversed old mean or duration weighting. Compare all 13 trajectory fields; exact integer/half-integer assertions, tight tolerance for non-exact means.
- Extend oracle checks to an off-grid open, an interval crossing CST midnight, exact-close ownership and existing zero-length behavior (opening cast retained). Do not fabricate uncertain date tables: derive inputs from the verified calendar adapter and expected physical arrays as above.
- Forecast tests materialize real intervals including the crossing fixture and compare against this independent oracle. Assert all five rendered values: Original oracle OHLC average, Transformed terminal, Mutual high/low/mean. Retain `materializer_preserves_order_and_half_open_bounds`, empty/error cases and timeline-free behavior. Equality to `iching_bar_trajectory` alone is supplemental, not independent evidence.

## 5. Verification, handoff and stop conditions

Commands below are acceptance requirements for later authorized execution; **none ran while authoring this plan**.

```sh
cargo test --locked --offline -p algotrap ta::iching
cargo check --workspace --locked --offline
cargo fmt --all --check
git --no-optional-locks diff --check
```

- Capture named-fixture RED output before codec edits, focused TA GREEN output afterward, and exit/results for remaining gates. Historical 603-pass PR b97e baseline is not certification.
- Check implementation diff against exactly the five-file boundary; preserve unrelated existing changes. Formatting check is read-only: no broad formatting fix outside scope. Do not refresh lockfiles, fetch dependencies, create scratch binaries/scripts or delete artifacts owned by others.
- Missing offline dependency, native permission denial, unavailable approval, material physical/calendar contradiction or out-of-scope compile/fmt failure: stop, report INCOMPLETE with exact failed tool/path or gate, completed evidence and next safe main-session action. No alternate permission route or scope expansion.
- One independent bounded review checks fixture independence, all-64/all-384 coverage, paired inverse correctness, physical transformations, cast-before-fold aggregation and acceptance evidence. After one targeted correction, unresolved issues return to main rather than an unbounded retry loop.
- Handoff lists changed files, named RED/GREEN evidence, all gate results, review disposition, and deferred consumer/text/docs work. U1 may be accepted as core-domain complete only when its required gates pass; the overall migration remains incomplete pending later consumer units and final workspace validation.

## Acceptance checkpoint — 2026-10-04

- **U1 implemented-verified:** paired Q codec/inverse, bottom32/top1 Yang1/Yin0, E=Q−31.5; six new independent TA tests. Physical flip/mutual/calendar and API shapes preserved. Original planning/acceptance text above remains historical, not a pending-work claim.
- Main-supplied final evidence: tester `ses_efc9c3b7fffeq7kOdJ8AA6PRKA` passed all fmt/check/build, locked-offline workspace/all-target tests (609, zero failed/ignored/filtered), strict Clippy `-D warnings`, offline DuckDB gate (609 again) and diff check; all exit0. Reviewer `ses_efca88215ffePyLKH2s2OKBxR5` **APPROVE** U1/U2/U3 semantics/protected policy. U2/U3 and final validation are now complete; see parent completion/handoff for full evidence and uncommitted-state boundaries.
- Evidence recorded, not rerun by this memory capture; only status/this acceptance record changed, no migration-source write.
