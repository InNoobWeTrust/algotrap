# Cryptobot RSSI Gap-Zone Trust Restoration — L2 Plan

## 1. Objective Recap

Repair exactly one regression: restore the former RSSI-derived confidence value for Cryptobot gap zones and expose it as `gapZones[*].trust`, so the existing gap-zone primitive uses that value for fill opacity and border strength.

This is **not** an RSSI feature restoration. RSSI and RSSI-MA must remain absent from chart panes, candle JSON, source/chart SQL projection, and public chart fields. Do not restore an RSSI chart panel, visual tint, EMA(9), candle field, or neutral/default confidence fallback.

The locked deployed semantics are:

```text
raw_rssi = RSI(14) evaluated for each candle at (kline.open + bar_bias)
trust = body_ratio * (0.5 + 0.5 * abs(raw_rssi - 50.0) / 50.0)
```

`RSI(14)` is raw RSI, not an EMA-smoothed value. The historical EMA(9) was display-only and must not be recomputed unless an already-current, non-display consumer is demonstrated; none is established by the inspected surface, so YAGNI applies.

## 2. Discovery Findings (existing patterns, verified paths, constraints)

### Verified ownership and flow

- `bins/cryptobot/src/presentation.rs`
  - `CryptoIndicators::transition` already evaluates `bar_bias`, `BodyRatio`, gap-candidate facts, and ATR Reversion in the Cryptobot app/presentation layer.
  - `collect_crypto_rows` preserves one stamped indicator row per supplied Kline and rejects cardinality/source-position mismatch.
  - `compute_crypto_frame(klines, ticker)` currently returns `Result<(Box<dyn ComputedFrame>, Vec<GapZoneRecord>), MarketError>`.
  - It creates a private source frame from `CryptoIndicatorRow`, projects chart/candle data with `build_crypto_sql`, and obtains raw zones through `recent_gap_zones(source, decision_time_ms, 64)`.
  - Existing tests lock schema absence of `rssi`, `rssi_ma`, `rssi_direction`, and `rssi_color`, source order, candidate geometry, and ATR Reversion semantics.
- `bins/cryptobot/src/main.rs`
  - `compute_crypto_frames` forwards the current `Vec<GapZoneRecord>` to `process_ticker`.
  - `gap_zone_to_json(&GapZoneRecord)` creates flat `gapZones` objects.
  - `GapZoneBandRenderer` presently uses hardcoded `opacity = 0.12` and `borderOpacity = 0.4`; it already consumes per-zone objects and is the correct existing visual consumer.
  - Current template tests establish that RSSI text/artifacts are absent and that Reverse RSI, Structure, ATR Reversion, and I-Ching panes remain present.
- `src/query/gap_zones.rs`
  - `GapZoneRecord` is an intentionally raw query/domain record with zone geometry, direction, and optional `body_ratio`; it is created by `recent_gap_zones` from source-frame gap-candidate columns.
  - It must remain unchanged. No RSSI/trust field, projection, query change, or query-layer wrapper belongs under `src/query/**`.
- `src/ta/ops/rsi.rs`
  - Existing `rsi(period)` is a stateful raw RSI kernel with `Rsi`/`RsiState`; it emits 50 for balanced gain/loss and 100 for gain with zero loss. `rsi` is already re-exported by `src/ta/prelude.rs`.
  - No `src/ta/**` change is required or permitted.

### Locked non-regression constraints

- Preserve raw gap geometry, direction, raw-zone filtering/order (ascending chronology, decision candle exclusion), selection, I-Ching panel, Reverse RSI overlays, Structure panel, and ATR Reversion panel.
- Preserve ATR Reversion exactly: `ATR(42) * 1.618`, centered on `open`, evaluated with `close` as the signal. Do not alter `BandPoint { open, atr, signal: kline.close }`, the multiplier, or existing ATR assertions.
- Keep `rssi`, `rssi_ma`, `rssi_direction`, and `rssi_color` out of `CryptoIndicatorRow`’s source-frame serialization, `crypto_select_expressions`, projected candle records, `TDV_HTML_TEMPLATE`, and public JSON. Internal RSI state/computation is permitted only to derive zone trust.
- Do not introduce a fallback (`0.12`, `0.4`, `0.5`, `1.0`, or any substitute) when a zone lacks an associated computed trust. Association failure is a presentation computation error; malformed/missing values must not be silently rendered as neutral confidence.
- Documentation-only planning scope: no implementation source, configuration, dependency, `.env`, generated artifact, network, browser, Git, command, background-process, host-PID/socket, or secret interaction.

## 3. Plan / Functional Units

### Target file tree

| File | Action | Purpose |
|---|---|---|
| `docs/plans/cryptobot-rssi-gap-trust-restoration/plan.md` | `[CREATE]` | Parent L2 plan (this document) |
| `docs/plans/cryptobot-rssi-gap-trust-restoration/phases/cryptobot-rssi-gap-trust-restoration-01-presentation.md` | `[CREATE]` | App-owned raw-RSI calculation, association, typed wrapper, and presentation tests |
| `docs/plans/cryptobot-rssi-gap-trust-restoration/phases/cryptobot-rssi-gap-trust-restoration-02-json-renderer.md` | `[CREATE]` | Wrapper JSON serialization, trust-driven primitive styling, and template tests |
| `docs/plans/cryptobot-rssi-gap-trust-restoration/phases/cryptobot-rssi-gap-trust-restoration-03-verification.md` | `[CREATE]` | Bounded verification and scope review |
| `bins/cryptobot/src/presentation.rs` | `[MODIFY]` during implementation | Unit 01 only |
| `bins/cryptobot/src/main.rs` | `[MODIFY]` during implementation | Unit 02 only |

No source files under `src/query/**` or `src/ta/**` are writable. No other implementation files are in scope.

### Locked DTO and return-signature contract

Unit 01 must introduce this **Cryptobot presentation-owned** wrapper in `bins/cryptobot/src/presentation.rs` (or an equivalent definition with exactly these members and visibility adequate for `main.rs`):

```rust
pub(crate) struct GapZonePresentation {
    pub(crate) raw: GapZoneRecord,
    pub(crate) trust: f64,
}
```

- `raw` retains the unmodified `GapZoneRecord`; it owns all raw geometry/direction/body metadata.
- `trust` is the finite derived presentation value for that exact raw zone’s source-candle timestamp.
- Do not add fields to `GapZoneRecord`, make a query/TA DTO, nest the public JSON, or expose RSI itself.
- Lock the producer signature to:

```rust
pub async fn compute_crypto_frame(
    klines: Vec<Kline>,
    ticker: ValidatedTicker,
) -> Result<(Box<dyn ComputedFrame>, Vec<GapZonePresentation>), MarketError>
```

- Propagate the corresponding container-type change through `main.rs` only:

```rust
HashMap<Timeframe, (Box<dyn ComputedFrame>, Vec<presentation::GapZonePresentation>)>
```

`gap_zone_to_json` must take `&presentation::GapZonePresentation` and create the same flat raw-zone object as today plus top-level numeric `"trust": zone.trust`.

### Ordered dispatchable units

1. **P01 — Derive and associate app-owned zone trust**
   Detailed handoff: [`cryptobot-rssi-gap-trust-restoration-01-presentation.md`](phases/cryptobot-rssi-gap-trust-restoration-01-presentation.md).
2. **P02 — Serialize trust and render trust-weighted gap zones**
   Detailed handoff: [`cryptobot-rssi-gap-trust-restoration-02-json-renderer.md`](phases/cryptobot-rssi-gap-trust-restoration-02-json-renderer.md).
3. **P03 — Run bounded contract verification**
   Detailed handoff: [`cryptobot-rssi-gap-trust-restoration-03-verification.md`](phases/cryptobot-rssi-gap-trust-restoration-03-verification.md).

## 4. Open Questions & Assumptions

**NONE.**

Resolved assumptions used by this plan:

- Raw RSI(14) at `open + bar_bias` is the confidence source; EMA(9) is not recomputed.
- Trust is sent as one finite top-level `gapZones[*].trust` number and directly controls the existing primitive’s opacity and border strength.
- A missing/ambiguous timestamp association is an error, not a visual fallback.
- The plan keeps the current raw-zone query model unchanged and limits implementation to the two verified Cryptobot files.

## 5. Confidence & Caveats

**Confidence: High.** The required inputs (`bar_bias`, `body_ratio`), existing RSI kernel, raw-zone body ratio/time, serialization seam, and hardcoded renderer strengths are all present in the verified files. The existing test modules already contain the correct schema, association, template-absence, and ATR non-regression conventions to extend without adding a test target.

**Caveat:** The `GapZoneRecord` query contract has only an optional body ratio and intentionally contains no RSI. The repair must therefore retain an app-private per-candle derived trust long enough to associate it with the returned raw zone by exact millisecond timestamp. This mapping must reject missing or duplicate time correspondence rather than substituting an opacity. If a unique exact association cannot be represented inside `presentation.rs` without changing the raw query contract, stop and escalate to `software-architect`; do not broaden into `src/query/**` or `src/ta/**`.

## 6. Done Signal

TASK_COMPLETE
