# Phase 02 — Chart document contracts

| Status | Dependencies | Outcome |
|---|---|---|
| Planned | 01 | Fail-closed v1 DTO, JSON, typed error, validation, and unit-test boundary for calculated data. |

## Scope
| In | Out |
|---|---|
| DTOs/errors/validation/serde/fixtures/unit tests. | Renderer/assets, adapters, publication, Docker, I/O/network, migrations, calculations. |

Producer invariant: binaries own acquisition, periods, profiles, warm-up, signal meaning, gap selection/order, and adaptation. Chartlib calculates, sorts, deduplicates, defaults, and fetches nothing. Telegram must locally provide Structure Directional and I-Ching Original/Transformed/Mutual before Phase 05.

## File operations
```text
[MODIFY] bins/chartlib/Cargo.toml     # serde + serde_json contract dependencies
[MODIFY] Cargo.lock                   # generated lock update
[MODIFY] bins/chartlib/src/lib.rs
[CREATE] bins/chartlib/src/contract.rs
[CREATE] bins/chartlib/src/error.rs
[CREATE] bins/chartlib/tests/contract_validation.rs
[CREATE] bins/chartlib/tests/serialization.rs
[CREATE] bins/chartlib/tests/fixtures/registry-v1.json
[CREATE] bins/chartlib/tests/fixtures/dataset-v1.json
[CREATE] bins/chartlib/tests/fixtures/fixed-document-v1.json
```
No `[DELETE]` or `[CLEANUP]` operations.

## Locked v1 interface
All serialized structs derive `Debug, Clone, PartialEq, Serialize, Deserialize`, use `#[serde(deny_unknown_fields)]`, serialize snake_case without defaults/skips; enums additionally derive `Copy, Eq`; `DatasetKey` additionally derives `Eq, Hash`. `InteractiveDataset` is non-serializable and derives `Debug, Clone, Copy`.
```rust
pub const REGISTRY_SCHEMA_VERSION: u32 = 1;
pub const DATASET_SCHEMA_VERSION: u32 = 1;
pub const FIXED_DOCUMENT_SCHEMA_VERSION: u32 = 1;
pub struct DatasetKey { pub ticker: String, pub timeframe: String }
pub struct DatasetIdentity { pub key: DatasetKey, pub display_symbol: String }
pub struct ChartRegistry { pub schema_version: u32, pub default_selection: DatasetKey, pub entries: Vec<RegistryEntry> }
pub struct RegistryEntry { pub key: DatasetKey, pub display_symbol: String, pub dataset_path: String, pub dataset_schema_version: u32 }
pub struct ChartDataset { pub schema_version: u32, pub identity: DatasetIdentity, pub price: PricePane, pub structure: StructurePane, pub atr_reversion: AtrReversionPane, pub iching: IchingPane, pub gap_zones: Vec<GapZone> }
pub struct FixedChartDocument { pub schema_version: u32, pub dataset: ChartDataset }
pub struct PricePane { pub candles: Vec<Candle>, pub volume_smoothing: ScalarSeries, pub long_term_average: ScalarSeries, pub bias_reversion: ScalarSeries, pub atr_upper_band: ScalarSeries, pub atr_lower_band: ScalarSeries, pub reverse_rsi_neutral: ScalarSeries, pub reverse_rsi_bullish: ScalarSeries, pub reverse_rsi_bearish: ScalarSeries }
pub struct StructurePane { pub primary: ScalarSeries, pub smoothing: ScalarSeries, pub directional: ScalarSeries }
pub struct AtrReversionPane { pub reversion: ScalarSeries }
pub struct IchingPane { pub original: ScalarSeries, pub transformed: ScalarSeries, pub mutual: ScalarSeries }
pub struct Candle { pub time_ms: i64, pub open: f64, pub high: f64, pub low: f64, pub close: f64, pub volume: f64 }
pub struct ScalarSeries { pub points: Vec<ScalarPoint> }
pub struct ScalarPoint { pub time_ms: i64, pub value: Option<f64> }
pub struct GapZone { pub lower: f64, pub upper: f64, pub direction: GapDirection }
#[serde(rename_all = "snake_case")] pub enum GapDirection { Bullish, Bearish, Flat }
pub struct InteractiveDataset<'a> { pub path: &'a str, pub dataset: &'a ChartDataset }
pub fn validate_dataset(dataset: &ChartDataset) -> Result<(), ChartContractError>;
pub fn validate_fixed_document(document: &FixedChartDocument) -> Result<(), ChartContractError>;
pub fn validate_interactive_publication(registry: &ChartRegistry, datasets: &[InteractiveDataset<'_>]) -> Result<(), ChartContractError>;
```
Exactly four panes/no collections: Price (candles plus eight listed overlays), Structure `primary/smoothing/directional`, ATR `reversion`, I-Ching `original/transformed/mutual`. Every slot is required; candle `volume` is the volume slot. Only aligned `ScalarPoint.value: null` is allowed; each series needs one `Some`.
```rust
pub enum DocumentKind { Registry, Dataset, FixedDocument }
pub enum SeriesSlot { PriceVolumeSmoothing, PriceLongTermAverage, PriceBiasReversion, PriceAtrUpperBand, PriceAtrLowerBand, PriceReverseRsiNeutral, PriceReverseRsiBullish, PriceReverseRsiBearish, StructurePrimary, StructureSmoothing, StructureDirectional, AtrReversion, IchingOriginal, IchingTransformed, IchingMutual }
pub enum NumericLocation { CandleOpen, CandleHigh, CandleLow, CandleClose, CandleVolume, Series(SeriesSlot), GapLower, GapUpper }
pub enum ChartContractError { UnsupportedVersion { document: DocumentKind, found: u32, supported: u32 }, EmptyRegistry, EmptyDataset, InvalidIdentity { field: &'static str, value: String }, InvalidDatasetPath { path: String }, DuplicateRegistryKey { key: DatasetKey }, DefaultSelectionNotFound { key: DatasetKey }, DatasetCountMismatch { entries: usize, datasets: usize }, DuplicateSuppliedDataset { path: String }, MissingReferencedDataset { path: String }, UnexpectedDataset { path: String }, ReferenceIdentityMismatch { path: String }, ReferenceDisplayMismatch { path: String }, ReferenceVersionMismatch { path: String, declared: u32, actual: u32 }, NonIncreasingTimestamp { index: usize, previous: i64, current: i64 }, SeriesLengthMismatch { slot: SeriesSlot, expected: usize, actual: usize }, SeriesTimestampMismatch { slot: SeriesSlot, index: usize, expected: i64, actual: i64 }, SeriesContainsNoValue { slot: SeriesSlot }, NonFiniteNumber { location: NumericLocation, index: usize }, NegativeVolume { index: usize, value: f64 }, InvalidCandleRange { index: usize }, InvalidGapBounds { index: usize }, JsonSerialization { message: String } }
```
`ChartContractError` derives `Debug, Clone, PartialEq`, implements `Display, Error`; deterministic order is version, identity, candles, slots, gaps, registry. Serde errors remain `serde_json::Error`.

## Validation lock
| Area | Rule |
|---|---|
| Version/schema | Exact v1; unknown fields rejected; shape/cardinality/path/order change requires bump, fixtures, explicit migration. |
| Identity | Trimmed non-empty; ticker/timeframe safe ASCII `[A-Za-z0-9._-]` segment, never `.`/`..`. |
| Candle/scalar | Nonempty, finite, strict chronological OHLCV; valid range/nonnegative volume; scalar count/times align, finite Some, one Some minimum. |
| Gaps | finite `lower <= upper`; zero-height flat allowed; preserve input order. |
| Registry | nonempty unique key/default; exact `data/{ticker}/{timeframe}.json`, `/` only, no traversal/query/fragment; one matching dataset per entry/no extras. Deterministic paths are injective over unique keys, so duplicate-path validation is intentionally represented by duplicate-key or invalid-path failure rather than a separate unreachable error. |
| Fixed | exactly one resolved dataset; cannot model registry/picker/fetch/fallback/switch. |

## TDD / verification
| RED | GREEN | REFACTOR |
|---|---|---|
| Fixture round-trip/exact keys; negative cases for every rule, refs, nulls. | DTOs/errors/validators only. | Keep rendering, adapters, calculations, I/O out. |
```bash
cargo fmt --all -- --check
CARGO_BUILD_JOBS=1 DUCKDB_DOWNLOAD_LIB=1 cargo test -p chartlib --locked
```
## Acceptance / stop
- Valid/negative v1 fixtures prove all locked validation paths; DTO has no calculation authority.
- **Stop:** later phases link here unchanged. Dispatch [03](03-shared-html-renderer.md).
