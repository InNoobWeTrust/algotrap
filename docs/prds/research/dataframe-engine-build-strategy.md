# Research Brief: DataFrame Engine Build Strategy

> **Created**: 2026-04-05
> **Status**: historical research; outcome implemented

## Historical Research Context

At the time of this research, the repository used the Rust **Polars** crate for
dataframe computation. Cold Docker builds compiled that dependency repeatedly,
so the research considered whether the computation boundary could instead use a
maintained, installable embedded engine.

The alternatives considered were:

| Historical option | Finding |
|---|---|
| Dynamically link Rust Polars as a system library | No supported, stable packaging path was identified. |
| Embed Python Polars through PyO3 | Avoided Rust compilation but added Python runtime, ABI, and operational complexity. |
| Retain Polars and improve caches/features | A short-term build mitigation, not an installed-engine solution. |
| Move the engine boundary to DuckDB | Best fit for the required installed embedded-runtime model. |

These references are retained only as historical terminology: Polars documentation
and crate packaging were the evidence reviewed for the pre-migration state.

## Implemented Outcome

The migration is complete. **DuckDB is the sole dataframe engine. Polars has
been removed, and there is no Polars fallback.**

- The technical-analysis (TA) layer owns pure indicator plans and kernels; it
  does not expose a dataframe-engine expression contract.
- The engine layer owns DuckDB integration, validated SQL/UDF and table-function
  execution, scheduling, and materialization.
- `MarketFrameEngine` and `ComputedFrame` are the consumer-facing interfaces for
  bot/chart/tool access to computed results.
- DuckDB is supplied as the embedded runtime boundary rather than compiled as a
  Rust dataframe crate in application code.

## Build Strategy

The builder must remain reproducible and multi-architecture capable:

1. Pin the official Rust builder by version and multi-architecture manifest-list
   digest while retaining `$TARGETPLATFORM` routing.
2. Copy both workspace manifest and `Cargo.lock` before installing workspace
   binaries.
3. Use `cargo install --locked` for every installed binary.
4. Preserve the separate DuckDB build stage that produces the dynamically loaded
   library for the target Debian/glibc runtime.

## Retained Findings

The historical conclusion remains useful for future dependency decisions:

1. Prefer a documented runtime/package boundary when the product requirement is
   prebuilt compute rather than source compilation.
2. Treat runtime, ABI, and deployment complexity as part of build-cost decisions.
3. Keep pure domain computation separate from engine-specific materialization so
   consumers remain insulated from engine internals.

## Notes

This is a research artifact, not a statement that Polars remains available in
the current repository. The current architecture is DuckDB-only.
