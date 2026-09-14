# docs/engineering/quality-gates.md — Verification Commands

> **Status**: Current (2026-09-09)
> **Scope**: All commands needed to verify the workspace locally before committing or opening a PR
> **Audience**: Contributors; CI pipeline authors

Run these commands from the **repository root** unless otherwise noted.

---

## About `DUCKDB_DOWNLOAD_LIB=1`

The workspace depends on the `duckdb` crate with dynamic DuckDB linkage. When no
system DuckDB shared library is installed, `libduckdb-sys` downloads the version-matched
official DuckDB archive at build time instead of searching for a system installation.

This is configured by default across the workspace in `.cargo/config.toml`
(`DUCKDB_DOWNLOAD_LIB = { value = "1", force = false }`), so developers and CI runners
do not need to prefix commands manually. It can be overridden via `DUCKDB_DOWNLOAD_LIB=0`
or `DUCKDB_LIB_DIR=/path/to/duckdb` when a specific system installation is desired.

`cargo fmt --check` never invokes the compiler and never requires this flag.
All commands that compile, link, or execute tests resolve the library automatically.

---

## 1. Format Check

```bash
cargo fmt --check
```

Fails if any file has formatting drift. Fix with `cargo fmt`.

---

## 2. Compile Check (all workspace crates, locked dependencies)

```bash
cargo check --workspace --locked
```

Verifies that all crates type-check without producing binaries. Uses the locked
`Cargo.lock` to ensure dependency reproducibility.

---

## 3. Clippy (all workspace crates, all targets, locked dependencies, deny warnings)

```bash
DUCKDB_DOWNLOAD_LIB=1 cargo clippy --workspace --all-targets --locked -- -D warnings
```

Runs Clippy lints across all workspace crates. `-D warnings` treats every
warning as an error. `DUCKDB_DOWNLOAD_LIB=1` is required when no system DuckDB
library is installed (see §About above). Fix all diagnostics before merging.

---

## 4. Test Suite (all workspace crates, all targets, locked dependencies)

```bash
DUCKDB_DOWNLOAD_LIB=1 cargo test --workspace --all-targets --locked
```

Runs the full test suite including unit tests in `src/` and all bin crates.
`DUCKDB_DOWNLOAD_LIB=1` is required when no system DuckDB library is installed.
The same flag is set in `bins/cryptobot/deployment/Dockerfile`,
`bins/telegrambot/deployment/Dockerfile`, and `.github/workflows/nightly.yml`;
the `Makefile` target `duckdb-test` uses it as well.

---

## 5. Focused Package and Test Filters

These filters are useful during development to run a targeted subset; they are
not a replacement for the full workspace pass above.

```bash
# BingX futures-kline normalization (src/ext/bingx.rs)
# Substring filter — matches test name containing this string
DUCKDB_DOWNLOAD_LIB=1 cargo test -p algotrap \
  futures_klines_normalize_bingx_newest_first_payload_without_changing_candles

# TA Processor state transitions (src/ta/processor.rs)
DUCKDB_DOWNLOAD_LIB=1 cargo test -p algotrap \
  processor_selects_initial_state_and_returns_output_only

# QueryResultFrame decodes DuckDB result types with nulls (src/engine/frame.rs)
DUCKDB_DOWNLOAD_LIB=1 cargo test -p algotrap \
  decodes_supported_types_with_nulls_at_edges_and_middle
```

---

## 6. Recommended Pre-Commit Sequence

```bash
# 1. Format
cargo fmt --all --check

# 2. Compile check
cargo check --workspace --locked

# 3. Clippy (all targets, deny warnings)
DUCKDB_DOWNLOAD_LIB=1 cargo clippy --workspace --all-targets --locked -- -D warnings

# 4. Full test suite (all targets)
DUCKDB_DOWNLOAD_LIB=1 cargo test --workspace --all-targets --locked
```

All four steps must pass on every commit.

---

## 7. Final Verification Sequence

The complete verification sequence, in exact order, from the repository root:

```bash
# 1. Format check
cargo fmt --all --check

# 2. Compile check (all workspace crates, locked dependencies)
cargo check --workspace --locked

# 3. Clippy (all workspace crates, all targets, locked dependencies, deny warnings)
DUCKDB_DOWNLOAD_LIB=1 cargo clippy --workspace --all-targets --locked -- -D warnings

# 4. Full test suite (all workspace crates, all targets, locked dependencies)
DUCKDB_DOWNLOAD_LIB=1 cargo test --workspace --all-targets --locked

# 5. DuckDB integration tests
make duckdb-test

# 6. Dependency tree (all workspace crates, locked dependencies)
cargo tree --workspace --locked
```

All listed commands run against the same unchanged repository state.

---

## 8. Notes

- **`--locked`** is mandatory for all CI-equivalent checks. It prevents
  accidental dependency drift from a stale `Cargo.lock`.
- **`DUCKDB_DOWNLOAD_LIB=1`** provisions the DuckDB native library at build
  time via the official `libduckdb-sys` downloader. The repository does not
  vendor the native library; container images provision it via this same flag
  in their Dockerfiles.
- There are no `#[ignore]`-marked tests in the workspace. Every test in
  `cargo test --workspace` runs unconditionally.
- For detailed component, container build, and shared-library context see
  [`docs/architecture/stream-and-duckdb-data-flow.md`](../architecture/stream-and-duckdb-data-flow.md).
