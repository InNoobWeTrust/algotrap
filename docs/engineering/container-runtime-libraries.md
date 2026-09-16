# Container Runtime Libraries

**Status: Current**

## Scope

This guidance applies to the Rust Docker deployment images for `cryptobot` and
`telegrambot` that bundle `libduckdb.so`.

## Runtime invariants

The runtime image must:

- copy the bundled library to `/usr/local/lib/libduckdb.so`;
- run `ldconfig` after installing the library;
- set `LD_LIBRARY_PATH=/usr/local/lib` for the process environment; and
- retain `DUCKDB_LIBRARY_PATH` for DuckDB crate library discovery.

`DUCKDB_LIBRARY_PATH` and ELF loader lookup serve different purposes. The
DuckDB crate uses `DUCKDB_LIBRARY_PATH` to discover the library for its build
or crate-level linking process. The ELF loader resolves a binary's
`libduckdb.so` dependency through its loader search paths, including
`LD_LIBRARY_PATH` and the `ldconfig` cache. `DUCKDB_LIBRARY_PATH` does not
control ELF loader resolution.

## Build-time verification

During image verification, run `ldd` against the deployed binary and confirm
that the `libduckdb.so` entry resolves exactly to:

```text
/usr/local/lib/libduckdb.so
```

A missing dependency or a resolution to another location is a runtime-image
regression and must be fixed before deployment.

## CI regression symptom

A broken runtime image commonly fails with:

```text
error while loading shared libraries: libduckdb.so
```

Treat this as evidence that the runtime invariants or the bundled library
path have regressed.
