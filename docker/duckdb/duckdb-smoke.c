#include "duckdb.h"

#include <stdio.h>
#include <string.h>

int main(void) {
    duckdb_database database = NULL;
    duckdb_connection connection = NULL;
    duckdb_result result;
    const char *version = duckdb_library_version();

    if (version == NULL || strcmp(version, "v1.5.5") != 0) {
        fprintf(stderr, "unexpected DuckDB library version: %s\n", version == NULL ? "(null)" : version);
        return 1;
    }
    if (duckdb_open(NULL, &database) == DuckDBError) {
        fputs("duckdb_open failed\n", stderr);
        return 1;
    }
    if (duckdb_connect(database, &connection) == DuckDBError) {
        fputs("duckdb_connect failed\n", stderr);
        duckdb_close(&database);
        return 1;
    }
    if (duckdb_query(connection, "SELECT 1", &result) == DuckDBError) {
        fprintf(stderr, "SELECT 1 failed: %s\n", duckdb_result_error(&result));
        duckdb_disconnect(&connection);
        duckdb_close(&database);
        return 1;
    }
    if (duckdb_row_count(&result) != 1 || duckdb_value_int32(&result, 0, 0) != 1) {
        fputs("SELECT 1 returned an unexpected result\n", stderr);
        duckdb_destroy_result(&result);
        duckdb_disconnect(&connection);
        duckdb_close(&database);
        return 1;
    }
    duckdb_destroy_result(&result);

    if (duckdb_query(
            connection,
            "WITH atr_values(atr) AS (VALUES "
            "(NULL::DOUBLE), (0.0), (-0.0), (2.0), "
            "('NaN'::DOUBLE), ('Infinity'::DOUBLE), ('-Infinity'::DOUBLE)) "
            "SELECT CASE WHEN atr - atr = 0 AND atr <> 0.0 "
            "THEN 100.0 / atr ELSE NULL END AS leverage FROM atr_values",
            &result) == DuckDBError) {
        fprintf(stderr, "finite ATR guard query failed: %s\n", duckdb_result_error(&result));
        duckdb_disconnect(&connection);
        duckdb_close(&database);
        return 1;
    }
    /* Row 3 corresponds to the 2.0 test input. */
    if (duckdb_row_count(&result) != 7 || duckdb_value_is_null(&result, 0, 3) ||
        duckdb_value_double(&result, 0, 3) != 50.0) {
        fputs("finite ATR guard did not preserve finite nonzero leverage\n", stderr);
        duckdb_destroy_result(&result);
        duckdb_disconnect(&connection);
        duckdb_close(&database);
        return 1;
    }
    for (idx_t row = 0; row < 7; row++) {
        if (row != 3 && !duckdb_value_is_null(&result, 0, row)) {
            fputs("finite ATR guard did not null an invalid ATR\n", stderr);
            duckdb_destroy_result(&result);
            duckdb_disconnect(&connection);
            duckdb_close(&database);
            return 1;
        }
    }

    duckdb_destroy_result(&result);
    duckdb_disconnect(&connection);
    duckdb_close(&database);
    return 0;
}
