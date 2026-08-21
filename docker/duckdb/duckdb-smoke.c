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
    duckdb_disconnect(&connection);
    duckdb_close(&database);
    return 0;
}
