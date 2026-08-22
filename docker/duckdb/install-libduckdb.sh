#!/usr/bin/env bash
# Installs the pinned official DuckDB prebuilt C API distribution (libduckdb)
# for a native Debian/glibc Linux target. No source compilation: downloads the
# checksum-verified release asset, validates the ELF machine against the build
# target, installs library + headers, then compiles and runs the C smoke test.
set -euo pipefail

readonly SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly WORK_ROOT=/tmp/duckdb-install
readonly INSTALL_ROOT=/opt/duckdb
readonly ARTIFACT_ROOT=/opt/duckdb-artifacts

# shellcheck source=duckdb-release.conf
source "${SCRIPT_DIR}/duckdb-release.conf"

require_supported_target() {
    if [[ "${TARGETOS:-}" != linux ]]; then
        printf 'DuckDB installer supports only Linux glibc targets; got TARGETOS=%s\n' "${TARGETOS:-unset}" >&2
        exit 64
    fi

    case "${TARGETARCH:-}" in
        amd64) expected_machine='Advanced Micro Devices X86-64' ;;
        arm64) expected_machine='AArch64' ;;
        *)
            printf 'DuckDB installer supports only TARGETARCH=amd64 or arm64; got %s\n' "${TARGETARCH:-unset}" >&2
            exit 64
            ;;
    esac

    if [[ "${TARGETPLATFORM:-linux/${TARGETARCH}}" != "linux/${TARGETARCH}" ]]; then
        printf 'DuckDB installer requires TARGETPLATFORM=linux/%s; got %s\n' \
            "${TARGETARCH}" "${TARGETPLATFORM}" >&2
        exit 64
    fi

    if ! getconf GNU_LIBC_VERSION >/dev/null 2>&1; then
        printf 'DuckDB installer requires glibc; musl and other C libraries are unsupported\n' >&2
        exit 64
    fi
}

select_release_asset() {
    case "${TARGETARCH}" in
        amd64)
            readonly ASSET_URL="${DUCKDB_ASSET_URL_AMD64}"
            readonly ASSET_SHA256="${DUCKDB_ASSET_SHA256_AMD64}"
            ;;
        arm64)
            readonly ASSET_URL="${DUCKDB_ASSET_URL_ARM64}"
            readonly ASSET_SHA256="${DUCKDB_ASSET_SHA256_ARM64}"
            ;;
    esac
}

require_supported_target
select_release_asset

export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends \
    binutils ca-certificates curl gcc libc6-dev unzip
rm -rf /var/lib/apt/lists/*

rm -rf "${WORK_ROOT}" "${INSTALL_ROOT}" "${ARTIFACT_ROOT}"
install -d -m 0755 "${WORK_ROOT}" "${INSTALL_ROOT}" "${ARTIFACT_ROOT}"

curl --fail --location --retry 3 --retry-all-errors --connect-timeout 20 --max-time 600 \
    --output "${WORK_ROOT}/libduckdb.zip" "${ASSET_URL}"
printf '%s  %s\n' "${ASSET_SHA256}" "${WORK_ROOT}/libduckdb.zip" | sha256sum --check --status

readonly EXTRACT_DIR="${WORK_ROOT}/dist"
install -d -m 0755 "${EXTRACT_DIR}"
unzip -q "${WORK_ROOT}/libduckdb.zip" -d "${EXTRACT_DIR}"
[[ -f "${EXTRACT_DIR}/libduckdb.so" && -f "${EXTRACT_DIR}/duckdb.h" && -f "${EXTRACT_DIR}/duckdb_extension.h" ]] || {
    printf 'DuckDB release asset lacks the expected libduckdb.so + C API headers\n' >&2
    exit 1
}

readonly LIBRARY_PATH="${INSTALL_ROOT}/lib/libduckdb.so"
install -D -m 0755 "${EXTRACT_DIR}/libduckdb.so" "${LIBRARY_PATH}"
install -D -m 0644 "${EXTRACT_DIR}/duckdb.h" "${INSTALL_ROOT}/include/duckdb.h"
install -D -m 0644 "${EXTRACT_DIR}/duckdb_extension.h" "${INSTALL_ROOT}/include/duckdb_extension.h"

actual_machine="$(readelf --file-header "${LIBRARY_PATH}" | awk -F: '/Machine:/{gsub(/^[[:space:]]+/, "", $2); print $2}')"
[[ "${actual_machine}" == "${expected_machine}" ]] || {
    printf 'libduckdb.so machine mismatch: expected %s for linux/%s, got %s\n' \
        "${expected_machine}" "${TARGETARCH}" "${actual_machine:-unknown}" >&2
    exit 1
}

gcc -std=c11 -Wall -Wextra -Werror "${SCRIPT_DIR}/duckdb-smoke.c" \
    -I "${INSTALL_ROOT}/include" -L "${INSTALL_ROOT}/lib" -Wl,-rpath,'$ORIGIN/../lib' -lduckdb \
    -o "${WORK_ROOT}/duckdb-smoke"
install -D -m 0755 "${WORK_ROOT}/duckdb-smoke" "${INSTALL_ROOT}/libexec/duckdb-smoke"
"${INSTALL_ROOT}/libexec/duckdb-smoke"
ldd "${LIBRARY_PATH}" | tee "${ARTIFACT_ROOT}/libduckdb.ldd"
! grep -Fq 'not found' "${ARTIFACT_ROOT}/libduckdb.ldd"

sha256sum "${LIBRARY_PATH}" | tee "${ARTIFACT_ROOT}/libduckdb.so.sha256"
cat > "${ARTIFACT_ROOT}/libduckdb.provenance.json" <<EOF
{
  "duckdb_version": "${DUCKDB_VERSION}",
  "asset_url": "${ASSET_URL}",
  "asset_sha256": "${ASSET_SHA256}",
  "distribution": "official-prebuilt",
  "target_os": "linux",
  "target_arch": "${TARGETARCH}",
  "elf_machine": "${actual_machine}",
  "libduckdb_sha256": "$(cut -d ' ' -f 1 "${ARTIFACT_ROOT}/libduckdb.so.sha256")"
}
EOF
