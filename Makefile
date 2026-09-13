trigger-nightly-workflow:
	gh workflow run nightly.yml

# Runs the workspace test suite against the dynamically downloaded DuckDB
# client (no local DuckDB install required). The env var is owned here so
# callers never set it by hand.
duckdb-test:
	DUCKDB_DOWNLOAD_LIB=1 cargo test --workspace --locked
