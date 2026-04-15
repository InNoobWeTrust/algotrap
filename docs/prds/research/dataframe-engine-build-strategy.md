# Research Brief: DataFrame Engine Build Strategy

> **Created**: 2026-04-05
> **Purpose**: Document findings that inform a later PRD/TRD for reducing cold-build cost of DataFrame-heavy services in this repo.

## Domain Research

### Problem Space

The repo currently builds DataFrame-heavy Rust binaries on top of the Rust
`polars` crate:

- Core library depends on `polars` with `default-features = true` plus multiple
  extra features in [Cargo.toml](/Volumes/SS850Evo/Developer/InNoobWeTrust/algotrap/Cargo.toml#L18)
- `cryptobot` depends on `polars` with `default-features = true` in
  [bins/cryptobot/Cargo.toml](/Volumes/SS850Evo/Developer/InNoobWeTrust/algotrap/bins/cryptobot/Cargo.toml#L13)
- `telegrambot` depends on `polars` with `default-features = true` in
  [bins/telegrambot/Cargo.toml](/Volumes/SS850Evo/Developer/InNoobWeTrust/algotrap/bins/telegrambot/Cargo.toml#L23)

The user goal is not “make Rust compile slightly faster.” The goal is:

1. Build on top of a beefy dataframe processing engine
2. Avoid repeatedly compiling that engine from source
3. Prefer a system-installed or prebuilt runtime boundary
4. Accept a runtime dependency if it materially simplifies builds

### Current Workflow Pain

- First cold Docker build for Rust `polars` is extremely expensive
- Local cluster deploys pay the full Linux compile tax when caches are cold
- Repeated experimentation on deployment Dockerfiles creates long feedback loops
- The repo currently pays compile cost in multiple binaries, not one isolated
  engine layer

## Technical Feasibility Research

### Candidate A: Keep Rust `polars` and Dynamically Link a System Package

**Research question**: Can Rust `polars` be treated like a preinstalled shared
system library and dynamically linked by the Rust crates in this repo?

**Finding**: No clean official path was found.

Evidence:

- The Rust Polars docs and crate packaging model emphasize crate features and
  compile-time opt-in feature flags, not a stable shared-library deployment
  model:
  - https://docs.pola.rs/docs/rust/dev/polars/
  - https://docs.rs/crate/polars/0.13.0

Inference from sources:

- Rust `polars` is intended to be compiled into the application as a Rust crate
- Compile time is managed by feature reduction, not by dynamically linking to a
  preinstalled system `libpolars.so`
- No official system-package / dynamic-link workflow surfaced in the primary
  documentation reviewed

**Verdict**: Not a viable primary strategy for this repo.

### Candidate B: Use Python `polars` via `pyo3`

**Research question**: Can we avoid Rust `polars` compile cost by embedding
Python and calling the Python `polars` wheel from Rust?

Evidence:

- PyO3 officially supports embedding and distribution, but requires a Python
  environment and explicit packaging/distribution choices:
  - https://pyo3.rs/v0.28.3/building-and-distribution.html
  - https://pyo3.rs/v0.28.2/getting-started.html

Analysis:

- This would likely reduce direct Rust `polars` compile cost
- But it introduces:
  - embedded Python runtime management
  - wheel/platform compatibility constraints
  - Python ABI/runtime coupling
  - more complicated containers and Kubernetes images
  - serialization / marshaling overhead between Rust and Python
  - harder operational debugging than a single-language binary

**Verdict**: Technically possible, architecturally unattractive. This moves
complexity from build time to runtime and packaging.

### Candidate C: Stay on Rust `polars`, Optimize the Build Boundary

This means accepting that Rust `polars` is a source dependency and optimizing
around that fact:

- prebuilt builder/base images
- cache mounts
- `sccache`
- slimmer `polars` feature sets
- fewer binaries depending directly on heavy dataframe code

Evidence:

- Historical Polars docs explicitly describe compile-time strain and recommend
  opt-in features/dtypes to manage cost:
  - https://docs.rs/crate/polars/0.13.0

Analysis:

- This is the least disruptive option
- It does not satisfy the user’s preferred model of “system-installed engine”
- It only reduces build pain; it does not eliminate it

**Verdict**: Best short-term mitigation if we keep the current architecture.

### Candidate D: Switch the Compute Boundary to an Installable Embedded Engine

**Research question**: If the real need is “prebuilt, installed compute,” should
we stop treating the dataframe engine as a Rust crate and instead depend on an
embedded engine with a stable runtime packaging model?

Strongest candidate reviewed: **DuckDB**

Evidence:

- DuckDB is distributed as installable binaries and libraries rather than as a
  Rust-crate-only compute boundary:
  - https://www.duckdb.org/docs/stable/operations_manual/installing_duckdb/install_script

Analysis:

- Better fit for the user’s real requirement:
  - preinstalled engine
  - ready-to-use compute runtime
  - dynamic/runtime boundary is natural
- Trade-offs:
  - query style shifts toward SQL / relational execution
  - migration cost from existing Polars-based transformation code
  - may require Arrow/Parquet/data interchange decisions at boundaries

**Verdict**: Best architectural match for the stated goal.

## Option Comparison

| Option | Matches “preinstalled compute” goal | Build simplicity | Runtime simplicity | Migration cost | Overall fit |
|--------|-------------------------------------|------------------|--------------------|----------------|-------------|
| Rust Polars + system dynamic link | No | Poor | Good | Low | Poor |
| Python Polars via PyO3 | Partial | Medium | Poor | Medium | Weak |
| Rust Polars + better caching/base images | No | Medium | Good | Low | Good short-term |
| Embedded engine like DuckDB | Yes | Good | Medium | Medium/High | Best long-term |

## Recommendation

### Short-Term

Do **not** attempt a “system-installed Polars dynamic link” design.

Instead:

1. Keep Rust `polars` where immediate migration cost is not justified
2. Reduce compile pain using build-boundary improvements:
   - prebuilt builder/base images
   - `sccache`
   - narrower `polars` feature sets

### Long-Term

If the product requirement is truly:

- “the system already has the compute engine installed”
- “I want to build on top of the engine rather than compile it”

then the repo should plan a migration of DataFrame-heavy paths toward an
embedded runtime engine such as **DuckDB**, not toward a forced dynamic-link
model for Rust `polars`.

## Proposed Next-Stage PRD Direction

Title suggestion: **Reduce cold-build cost of dataframe-heavy services by
changing the compute boundary**

Likely child TRDs:

- `docs/trds/polars-feature-slimming.md` — immediate build-cost reductions
- `docs/trds/duckdb-evaluation.md` — validate feasibility of DuckDB replacement
- `docs/trds/dataframe-boundary-refactor.md` — isolate compute-heavy code behind
  a smaller interface

## Key Findings

1. Rust `polars` is packaged and documented as a source crate, not as a
   system-installed dynamic library boundary.
2. Python `polars` via `pyo3` is possible, but it solves the wrong problem by
   trading compile pain for runtime and packaging complexity.
3. The user’s desired operating model matches an embedded engine such as
   DuckDB better than Rust `polars`.
4. If migration is deferred, the best short-term move is build optimization,
   not forced dynamic linking.

## ⚔ Challenge Gate

> **Status**: passed
> **Challenger**: self-review
> **Date**: 2026-04-05

### Debate Record

| # | Vector | Challenge | Response | Verdict |
|---|--------|-----------|----------|---------|
| 1 | Alternatives | Why not just use Python `polars` and stop caring about Rust compile times? | Because that introduces a Python runtime boundary, packaging complexity, and ABI coupling that are materially worse for long-lived Rust/Kubernetes services than the current build pain. | author-won |
| 2 | Assumptions | Are we over-indexing on DuckDB because it is easier to package, even if the workload is dataframe-style rather than SQL-style? | That risk is real. This brief recommends DuckDB only as the best fit for the stated deployment requirement, not as an automatic replacement for all current Polars code. A dedicated TRD should validate workload fit before migration. | author-won |
| 3 | Scope | Does this recommendation solve the immediate pain of current cold builds? | Partially. The short-term recommendation explicitly keeps build-boundary optimizations while deferring architectural migration. | author-won |

### Challenge Summary

- **Challenges raised**: 3
- **Author victories**: 3
- **Challenger victories**: 0
- **Escalated**: 0
- **Overall verdict**: ACCEPTED

## Notes

- This is a first-stage research artifact, not an implementation plan.
- Next useful step is a TRD that evaluates either:
  - aggressive Rust `polars` feature slimming and caching, or
  - a bounded migration of one heavy path to DuckDB.
